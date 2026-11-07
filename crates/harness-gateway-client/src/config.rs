//! Client configuration: the redacted bearer secret, the validated gateway
//! endpoint, and the setup errors a bad setup reports.
//!
//! A setup error happens before the client sends its first request. It is
//! an [`InvalidValue`](ErrorCode::InvalidValue) whose message names the
//! environment variable or the rule that failed, never a key, and whose
//! `Reason` fact is `missing_env`, `invalid_env`, or `invalid_config`.

use std::fmt;

use runvil::{Error, ErrorCode, ErrorFact};
use serde_json::Value;

/// The setup error for a required environment variable that is not set.
pub(crate) fn missing_env(name: &str) -> Error {
    let mut error = setup_error(
        "missing_env",
        format!("missing environment variable: {name}"),
    );
    error.facts.insert(ErrorFact::Name, Value::from(name));
    error
}

/// The setup error for an environment variable whose value is not valid
/// Unicode.
pub(crate) fn invalid_env(name: &str) -> Error {
    let mut error = setup_error(
        "invalid_env",
        format!("environment variable is set but not valid Unicode: {name}"),
    );
    error.facts.insert(ErrorFact::Name, Value::from(name));
    error
}

/// A setup error with `reason`. Its prose is its message, which names the
/// variable or the rule to fix.
fn setup_error(reason: &'static str, message: String) -> Error {
    let mut error = Error::new(ErrorCode::InvalidValue, message.clone(), message);
    error.facts.insert(ErrorFact::Reason, Value::from(reason));
    error
}

/// A bearer credential whose contents never appear in `Debug` or `Display`
/// output or in logs.
///
/// Wrap a secret, such as the gateway bearer key, in a `SecretString` as soon
/// as you read it, so an accidental `{:?}` or log line cannot leak it. The
/// client reads the value only to set the `Authorization` header of its
/// requests.
#[derive(Clone)]
#[non_exhaustive]
pub struct SecretString(String);

impl SecretString {
    /// Wraps a secret so it is redacted everywhere it is formatted.
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error with the
    /// reason `empty_secret` when `secret` is empty, so a client can never be
    /// built to authenticate with a blank bearer credential.
    pub fn new(secret: impl Into<String>) -> std::result::Result<SecretString, Error> {
        let secret = secret.into();
        if secret.is_empty() {
            let mut error = Error::new(
                ErrorCode::InvalidValue,
                "secret must not be empty",
                "The secret is empty. Supply a non-empty secret.",
            );
            error
                .facts
                .insert(ErrorFact::Reason, Value::from("empty_secret"));
            return Err(error);
        }
        Ok(SecretString(secret))
    }

    /// Borrows the raw secret. Crate-internal so no downstream code can read a
    /// credential back out of the type.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// A validated base URL for the gateway's OpenAI-compatible API (its `/v1`
/// root).
///
/// Construction requires an `http` or `https` scheme and a host, so a
/// client always points at a usable endpoint. Any trailing slash is removed
/// so request paths join cleanly.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayEndpoint {
    pub(crate) url: String,
    /// Whether the host names the local machine: `localhost`, or an IP whose
    /// `is_loopback()` holds. Decided at construction from the parsed host.
    loopback: bool,
}

impl GatewayEndpoint {
    /// Validates and normalizes a gateway base URL.
    ///
    /// The URL is checked with a strict URL parser.
    ///
    /// # Errors
    /// Returns an `invalid_config` setup error when `url` fails to parse as
    /// an absolute URL, uses a scheme other than `http` or `https`, omits the
    /// host, embeds credentials (a `user:pass@` component), or has a query or
    /// fragment.
    /// A query or fragment is rejected because an API root is a bare path.
    /// No error includes `url`, because a URL can contain a credential.
    pub fn new(url: &str) -> std::result::Result<GatewayEndpoint, Error> {
        let reject = |message: String| setup_error("invalid_config", message);
        let trimmed = url.trim();
        // The parse failure's own text names the rule, never the input.
        let parsed = url::Url::parse(trimmed)
            .map_err(|error| reject(format!("gateway URL is not a valid URL: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(reject(
                "gateway URL must use the http or https scheme".to_owned(),
            ));
        }
        let loopback = match parsed.host() {
            None | Some(url::Host::Domain("")) => {
                return Err(reject("gateway URL names no host".to_owned()));
            }
            // The URL parser lowercases the host of an http(s) URL, so the
            // literal comparison covers `LOCALHOST` too.
            Some(url::Host::Domain(domain)) => domain == "localhost",
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        };
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(reject(
                "gateway URL must not embed credentials (user:pass@)".to_owned(),
            ));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(reject(
                "gateway URL must not include a query or fragment".to_owned(),
            ));
        }
        Ok(GatewayEndpoint {
            // Normalized by the URL parser; trim the trailing slash so request
            // paths (`{base}/chat/completions`) join cleanly.
            url: parsed.as_str().trim_end_matches('/').to_string(),
            loopback,
        })
    }

    /// Returns the normalized base URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns whether the endpoint's host is the local machine.
    ///
    /// It returns `true` for `localhost`, for `127.0.0.1` and the rest of
    /// `127.0.0.0/8`, and for `::1`. It returns `false` for every other name
    /// or address. By default, a gateway on the local machine accepts callers
    /// that omit the key, so
    /// [`GatewayChat::from_env`](crate::GatewayChat::from_env) makes the
    /// bearer key optional exactly when this returns `true`.
    #[must_use]
    pub fn is_loopback(&self) -> bool {
        self.loopback
    }
}

impl TryFrom<&str> for GatewayEndpoint {
    type Error = Error;

    fn try_from(url: &str) -> std::result::Result<GatewayEndpoint, Error> {
        GatewayEndpoint::new(url)
    }
}
