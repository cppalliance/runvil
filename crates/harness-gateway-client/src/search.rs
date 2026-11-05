//! The Gateway web search provider: each search is one
//! `POST {api_root}/tools/web_search` with the bearer key under a fixed
//! deadline, a bounded and sanitized error body, and a capped success body
//! parsed into private wire types that mirror the Gateway's request and
//! response, then mapped into the provider's results and errors.

use std::fmt;
use std::time::Duration;

use plugin_web::{
    SearchError, SearchErrorKind, SearchProvider, SearchQuery, SearchResult, SearchResults,
};
use runvil::error::{Error, ErrorCode};
use runvil::tools::tool_error;

use crate::config::{GatewayEndpoint, SecretString};

/// The largest error body kept for diagnostics, in bytes.
const MAX_ERROR_BODY: usize = 2000;

/// The largest successful response body accepted from the gateway, in bytes.
///
/// Search results include third-party web content, so the body is bounded
/// to keep a hostile or misbehaving upstream from returning an unbounded
/// payload. A body past this cap is rejected rather than silently
/// truncated, since a truncated JSON document is not a valid result set.
const MAX_RESPONSE_BODY: usize = 256 * 1024;

/// The deadline applied to every outbound request, body included, so a
/// stalled gateway cannot hang a search (and thus a run) indefinitely.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The body of a Gateway web search, mirroring the Gateway's
/// `POST /v1/tools/web_search` request.
///
/// Only `query` is required. An absent option or an empty domain list is
/// left out of the body, and the Gateway applies its own default.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
struct GatewaySearchRequest {
    /// The search query.
    query: String,
    /// The number of results wanted; the Gateway clamps it to its maximum.
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<u8>,
    /// The freshness filter: `pd`, `pw`, `pm`, `py`, or a
    /// `YYYY-MM-DDtoYYYY-MM-DD` range.
    #[serde(skip_serializing_if = "Option::is_none")]
    freshness: Option<String>,
    /// The country code for the search.
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<String>,
    /// The search language code.
    #[serde(skip_serializing_if = "Option::is_none")]
    search_lang: Option<String>,
    /// The SafeSearch level: `off`, `moderate`, or `strict`.
    #[serde(skip_serializing_if = "Option::is_none")]
    safesearch: Option<String>,
    /// Keep only results from these hostnames.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    include_domains: Vec<String>,
    /// Drop results from these hostnames.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclude_domains: Vec<String>,
}

/// The reply to a Gateway web search, mirroring the Gateway's
/// `POST /v1/tools/web_search` response.
///
/// `query`, `results` and each result's `url` are required, so a reply
/// without them is a malformed response. Every other field defaults when
/// absent, and unknown fields are ignored so the Gateway can grow its reply.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
struct GatewaySearchResponse {
    /// The query the Gateway ran, after its trimming.
    query: String,
    /// The result rows, in the Gateway's order.
    results: Vec<GatewaySearchResult>,
}

/// One row of a [`GatewaySearchResponse`].
///
/// The `url` is required but may be empty; judging an empty `url` is left
/// to the caller.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
struct GatewaySearchResult {
    /// The result's title.
    #[serde(default)]
    title: String,
    /// The result's URL.
    url: String,
    /// A short description or snippet.
    #[serde(default)]
    description: String,
    /// The result's age, when the provider reports one.
    age: Option<String>,
    /// The hostname of `url`, when the Gateway could derive one.
    site_name: Option<String>,
    /// Extra snippets from the provider.
    #[serde(default)]
    extra_snippets: Vec<String>,
}

/// The [`SearchProvider`] a Host supplies to search the web through the
/// Gateway.
///
/// It sends every search to one Gateway API root with the Gateway's shared
/// bearer key. Each search is one `POST {api_root}/tools/web_search`
/// request under a 30-second deadline. The deadline covers the whole
/// exchange, including reading the reply body. The search vendor's
/// credential stays in the Gateway. This provider sends only the Gateway's
/// key.
#[derive(Clone)]
#[non_exhaustive]
pub struct GatewaySearch {
    http: reqwest::Client,
    base_url: String,
    key: SecretString,
    timeout: Duration,
}

impl fmt::Debug for GatewaySearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewaySearch")
            .field("base_url", &self.base_url)
            .field("key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl GatewaySearch {
    /// Builds a search provider from a validated [`GatewayEndpoint`] and a
    /// redacted [`SecretString`] bearer key.
    #[must_use]
    pub fn new(endpoint: GatewayEndpoint, key: SecretString) -> GatewaySearch {
        GatewaySearch::with_timeout(endpoint, key, REQUEST_TIMEOUT)
    }

    fn with_timeout(
        endpoint: GatewayEndpoint,
        key: SecretString,
        timeout: Duration,
    ) -> GatewaySearch {
        GatewaySearch {
            http: reqwest::Client::new(),
            base_url: endpoint.url,
            key,
            timeout,
        }
    }

    /// Runs one web search and returns the Gateway's parsed reply, failing
    /// as the [`SearchProvider`] impl documents: `ToolTransport` where it
    /// says `Transport` and `ToolBackend` where it says `Backend`, with the
    /// text it names as the prose and any cause after it in the message.
    async fn search(&self, request: &GatewaySearchRequest) -> Result<GatewaySearchResponse, Error> {
        let response = self
            .http
            .post(format!("{}/tools/web_search", self.base_url))
            .bearer_auth(self.key.expose())
            .timeout(self.timeout)
            .json(request)
            .send()
            .await
            .map_err(|source| {
                tool_error(ErrorCode::ToolTransport, "request failed").with_cause(&source)
            })?;

        let status = response.status();
        if !status.is_success() {
            let code = status.as_u16();
            // The error body is external gateway content: bound the read and
            // sanitize control characters. If the body itself cannot be read,
            // keep the read failure in the returned error's message.
            match read_bounded(response, MAX_ERROR_BODY).await {
                Ok(body) => {
                    let body = if body.is_empty() {
                        "(empty body)".to_owned()
                    } else {
                        sanitize_diagnostic(&body)
                    };
                    return Err(tool_error(
                        ErrorCode::ToolBackend,
                        format!("backend returned {code}: {body}"),
                    ));
                }
                Err(source) => {
                    return Err(tool_error(
                        ErrorCode::ToolBackend,
                        format!("backend returned {code}, and its error body could not be read"),
                    )
                    .with_cause(&source));
                }
            }
        }

        // Success bodies hold third-party content: bound them (rejecting cap
        // overflow), then parse the promised JSON shape.
        let body = read_capped(response, MAX_RESPONSE_BODY).await?;
        serde_json::from_str(&body).map_err(|source| {
            tool_error(ErrorCode::ToolBackend, "malformed search response").with_cause(&source)
        })
    }
}

#[async_trait::async_trait]
impl SearchProvider for GatewaySearch {
    /// Runs `query` as one Gateway web search and returns the Gateway's
    /// results with every field copied as the Gateway sent it.
    ///
    /// # Errors
    /// Returns a [`SearchError`] whose text is the prose of the [`Error`]
    /// it keeps as its source. The Gateway's error body in that text is
    /// truncated to a size limit and its control characters are escaped,
    /// and the text never holds the bearer key. Its kind is:
    /// - `Transport` when sending the request fails (`request failed`) or
    ///   reading a successful reply fails (`reading response failed`),
    ///   including when the deadline passes;
    /// - `Backend` when the Gateway answers a failure status
    ///   (`backend returned {code}: {body}`, or `backend returned {code},
    ///   and its error body could not be read`), or a success body that is
    ///   over 256 KiB (`response body exceeded {limit} bytes`), is invalid
    ///   UTF-8 (`response body was not valid UTF-8`), or fails to parse as a
    ///   search reply (`malformed search response`).
    async fn search(&self, query: SearchQuery) -> Result<SearchResults, SearchError> {
        let request = gateway_request(query);
        let response = self.search(&request).await.map_err(search_error)?;
        Ok(search_results(response))
    }
}

/// The Gateway's request for a validated query.
fn gateway_request(query: SearchQuery) -> GatewaySearchRequest {
    GatewaySearchRequest {
        query: query.query,
        count: query.count,
        freshness: query
            .freshness
            .map(|freshness| freshness.as_str().to_owned()),
        country: query.country,
        search_lang: query.search_lang,
        safesearch: query.safesearch.map(|level| level.as_str().to_owned()),
        include_domains: query.include_domains,
        exclude_domains: query.exclude_domains,
    }
}

/// The provider's results for the Gateway's reply.
fn search_results(response: GatewaySearchResponse) -> SearchResults {
    SearchResults {
        query: response.query,
        results: response
            .results
            .into_iter()
            .map(|result| SearchResult {
                title: result.title,
                url: result.url,
                description: result.description,
                age: result.age,
                site_name: result.site_name,
                extra_snippets: result.extra_snippets,
            })
            .collect(),
    }
}

/// The provider's error for a failed Gateway search: its kind from the
/// code, its text from the prose, and the Gateway's error as the cause.
fn search_error(error: Error) -> SearchError {
    let kind = if error.code == ErrorCode::ToolTransport {
        SearchErrorKind::Transport
    } else {
        SearchErrorKind::Backend
    };
    SearchError::with_source(kind, error.prose.clone(), error)
}

/// Escapes control characters in an external diagnostic body so a hostile
/// gateway cannot inject terminal/log control sequences or forge multiline
/// records through an error `Display`.
fn sanitize_diagnostic(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for c in body.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{{{:04x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

/// Reads at most `limit` bytes of a diagnostic body, stopping early once the cap
/// is reached. Used for the error path, where a truncated, lossy rendering is an
/// acceptable diagnostic.
async fn read_bounded(mut response: reqwest::Response, limit: usize) -> Result<String, Error> {
    let mut buffer: Vec<u8> = Vec::new();
    while buffer.len() < limit {
        let chunk = response.chunk().await.map_err(|source| {
            tool_error(ErrorCode::ToolTransport, "reading response failed").with_cause(&source)
        })?;
        let Some(chunk) = chunk else { break };
        let take = (limit - buffer.len()).min(chunk.len());
        buffer.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// Reads a success body, rejecting it once it would exceed `limit` bytes rather
/// than truncating (a truncated JSON document is not a valid result set), and
/// requiring valid UTF-8.
async fn read_capped(mut response: reqwest::Response, limit: usize) -> Result<String, Error> {
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|source| {
        tool_error(ErrorCode::ToolTransport, "reading response failed").with_cause(&source)
    })? {
        if buffer.len() + chunk.len() > limit {
            return Err(tool_error(
                ErrorCode::ToolBackend,
                format!("response body exceeded {limit} bytes"),
            ));
        }
        buffer.extend_from_slice(&chunk);
    }
    String::from_utf8(buffer).map_err(|source| {
        tool_error(ErrorCode::ToolBackend, "response body was not valid UTF-8").with_cause(&source)
    })
}

#[cfg(test)]
#[path = "search-tests.rs"]
mod tests;
