//! `{{ }}` prose substitution.
//!
//! When a section's Lua first reads the lazy `prose` value, the Engine
//! resolves `{{ path }}` placeholders in the pending Markdown template. Lua
//! source is never substituted. Five sources are available: `args` (the single
//! raw input string), `argv` (the chain's `argv`: the derived value in the H1
//! pass, the value the H1 left behind on the walk; nil when it holds no
//! value), `item` (the item seeded into this section - a fanout arm's or a
//! `tasks.spawn` task's first entry - nil otherwise), `var` (values the
//! section's Lua wrote), and `sys` (runtime-provided metadata). An unknown
//! first segment resolves as a bare global: a section-local Lua global
//! (`x = 42` without `local`) read through a caller-supplied lookup, with
//! dotted paths indexing into its JSON form. Resolution is a single pass with
//! no recursion: scalars render as strings, tables/arrays as JSON, and a
//! missing path is a hard error. `{{ item }}` with no item seeded into this
//! section is a hard error. A missing bare global, or one holding a function
//! or userdata, is a hard error. Substitution does no arithmetic - compute in
//! Lua and reference the result.
//!
//! # Escape grammar
//!
//! A backslash escapes the following character when it is `{`, `}`, or `\\`:
//! the backslash is consumed and the next character is emitted literally.
//! Everywhere else a backslash is an ordinary literal. This lets prose
//! include a literal opening delimiter (`\{{` emits `{{`), a literal closing
//! delimiter (`\}}` emits `}}`), and a literal backslash (`\\` emits `\`).
//! Escapes compose, so adjacent escaped delimiters resolve independently.
//!
//! Substitution is a single left-to-right pass over the *input* prose only:
//! resolved output is appended to a separate buffer and never rescanned, so a
//! replacement that itself contains `{{ ... }}` is emitted verbatim and never
//! triggers a second round of substitution.

use std::num::NonZeroU32;

use runvil_types::error::{Error, ErrorCode, ErrorFact, place};
use serde_json::Value;

use crate::Result;

/// Builds the failure of the placeholder that opens at byte `offset` of the
/// prose. `detail` says what failed, with a bounded, control-escaped preview
/// of the placeholder path; the message follows it with the documented
/// kind name, the reason in CamelCase, and the offset, as
/// `{detail} [MissingKey at byte 4]`, and the prose with the code's default
/// text. `reason` is the `Reason` fact, one of:
///
/// - `unclosed`: a `{{` was opened but never closed with `}}`.
/// - `bad_path`: a bare namespace with no `.key` suffix where one is
///   required.
/// - `empty_segment`: a path segment was empty or whitespace-padded
///   (`var.`, `var..x`).
/// - `unknown_namespace`: the leading namespace is not one of the known
///   roots and names no bare global.
/// - `not_a_table`: a key path after `args` or `item`, which render whole
///   only.
/// - `missing_key`: a key path found no value: under `var`, `sys`, `argv`,
///   or a bare global.
/// - `null_value`: the resolved value was JSON null, for a `var`, `sys`,
///   `argv`, or bare global source. A null `item` is not this: `{{ item }}`
///   renders it as the text `null`.
/// - `nil_item`: `{{ item }}` was used with no item seeded into this
///   section.
/// - `nil_argv`: an `argv` placeholder, bare or dotted, was used while
///   `argv` was nil.
/// - `serialize`: the bare-global lookup failed: the global cannot become
///   JSON data, the name is `prose` itself, or the lookup raised.
fn failure(reason: &str, offset: usize, detail: String) -> Error {
    let code = ErrorCode::Substitution;
    let kind: String = reason
        .split('_')
        .flat_map(|word| {
            let mut letters = word.chars();
            letters
                .next()
                .map(|first| first.to_ascii_uppercase())
                .into_iter()
                .chain(letters)
        })
        .collect();
    let message = format!("{detail} [{kind} at byte {offset}]");
    let mut error = Error::new(code, message, detail + ". " + code.prose());
    error.facts.insert(ErrorFact::Reason, Value::from(reason));
    error
}

type SubstResult<T> = std::result::Result<T, Error>;

/// The value sources `{{ }}` placeholders resolve against.
///
/// `var` and `sys` are JSON objects.
pub(crate) struct Sources<'a> {
    /// The raw args string: `{{ args }}`.
    pub(crate) args: &'a str,
    /// The chain's `argv`, which the H1 pass may repair: `{{ argv }}` and
    /// dotted paths.
    pub(crate) argv: Option<&'a Value>,
    /// The item seeded into this section on a fanout arm's or a
    /// `tasks.spawn` task's first entry, rendered per
    /// [`render_item`](crate::lua::render_item): `{{ item }}`. `None` on
    /// every other entry.
    pub(crate) item: Option<&'a Value>,
    /// The section's `var` table, read back from its Lua.
    pub(crate) var: &'a Value,
    /// The runtime-provided metadata: `{{ sys.key }}`.
    pub(crate) sys: &'a Value,
    /// The bare-global lookup for `{{ name }}` resolution: `Ok(None)` when
    /// unset, `Ok(Some(_))` with its JSON form when set, and `Err` when the
    /// lookup fails, which a placeholder reports as `serialize`.
    pub(crate) globals: &'a dyn Fn(&str) -> Result<Option<Value>>,
    /// The prompt file the prose comes from, which a failure is placed in.
    pub(crate) file: &'a str,
    /// The prompt line the prose starts on, when known.
    pub(crate) line: Option<NonZeroU32>,
}

/// Resolves every `{{ path }}` in `prose` against the [`Sources`].
///
/// This function receives prose only; the compiled Lua phases are
/// untouched.
///
/// # Errors
/// Returns a `Substitution` failure whose `Reason` fact is one of the
/// reasons `failure` lists, which is the complete list of what a
/// placeholder can fail with. Its message names the reason and the byte
/// offset of the offending placeholder after a detail with its bounded,
/// control-escaped path preview, and a `serialize` message then holds the
/// lookup failure. It is placed in the sources' file on the offending
/// placeholder's line, when the line the prose starts on is known.
pub(crate) fn substitute(prose: &str, sources: &Sources<'_>) -> Result<String> {
    let mut out = String::with_capacity(prose.len());
    let bytes = prose.as_bytes();
    let mut i = 0;
    while i < prose.len() {
        // Escape grammar: a backslash consumes itself and emits a literal `{`,
        // `}`, or `\` when one immediately follows.
        if bytes[i] == b'\\' && i + 1 < prose.len() {
            let next = bytes[i + 1];
            if matches!(next, b'{' | b'}' | b'\\') {
                out.push(next as char);
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'{' && i + 1 < prose.len() && bytes[i + 1] == b'{' {
            let start = i;
            let placed = |error| {
                place(
                    error,
                    sources.file,
                    placeholder_line(sources.line, &prose[..start]),
                )
            };
            let after = &prose[i + 2..];
            let end = after.find("}}").ok_or_else(|| {
                placed(failure(
                    "unclosed",
                    start,
                    "unclosed '{{' in prose".to_string(),
                ))
            })?;
            let path = after[..end].trim();
            out.push_str(&resolve(path, start, sources).map_err(placed)?);
            i += 2 + end + 2;
            continue;
        }
        let Some(ch) = prose[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    Ok(out)
}

/// The prompt line of a placeholder that `before` comes ahead of, in prose
/// that starts on `line`; `None` when `line` is unknown or the sum
/// overflows.
fn placeholder_line(line: Option<NonZeroU32>, before: &str) -> Option<NonZeroU32> {
    let newlines = u32::try_from(before.matches('\n').count()).ok()?;
    line?.checked_add(newlines)
}

/// The nil-`argv` failure shared by the bare and dotted `{{ argv }}` paths.
fn nil_argv<'a>(argv: Option<&'a Value>, path: &str, offset: usize) -> SubstResult<&'a Value> {
    argv.ok_or_else(|| {
        failure(
            "nil_argv",
            offset,
            format!(
                "{{{{ {} }}}} is nil (argv holds no value)",
                path_preview(path)
            ),
        )
    })
}

/// Resolves an unknown first segment as a bare global: the caller's lookup
/// reads the section-local Lua global and converts it to JSON.
fn bare_global_root(
    name: &str,
    path: &str,
    offset: usize,
    globals: &dyn Fn(&str) -> Result<Option<Value>>,
) -> SubstResult<Value> {
    globals(name)
        .map_err(|error| {
            failure(
                "serialize",
                offset,
                format!(
                    "global '{}' in {{{{ {} }}}} is not JSON data",
                    path_preview(name),
                    path_preview(path)
                ),
            )
            .with_cause(&error)
        })?
        .ok_or_else(|| {
            failure(
                "unknown_namespace",
                offset,
                format!(
                    "unknown namespace or global '{}' in {{{{ {} }}}}",
                    path_preview(name),
                    path_preview(path)
                ),
            )
        })
}

/// Resolves a single `{{ }}` path to its rendered string.
fn resolve(path: &str, offset: usize, sources: &Sources<'_>) -> SubstResult<String> {
    if path == "args" {
        return Ok(sources.args.to_string());
    }
    if path == "item" {
        return sources.item.map(crate::lua::render_item).ok_or_else(|| {
            failure(
                "nil_item",
                offset,
                "{{ item }} is nil (no item is seeded into this section)".to_string(),
            )
        });
    }
    if path == "argv" {
        return render(nil_argv(sources.argv, path, offset)?, path, offset);
    }

    // Validate the complete segment grammar before any lookup: every segment
    // (namespace included) must be nonempty and free of leading or trailing
    // whitespace, so `var.`, `var..x`, and `var. .x` are rejected up front even
    // when a matching JSON key happens to exist.
    for segment in path.split('.') {
        if segment.is_empty() || segment.trim() != segment {
            return Err(failure(
                "empty_segment",
                offset,
                format!(
                    "empty or padded path segment in {{{{ {} }}}}",
                    path_preview(path)
                ),
            ));
        }
    }

    let (namespace, keys) = match path.split_once('.') {
        Some((namespace, keys)) => (namespace, Some(keys)),
        None => (path, None),
    };

    let global_value;
    let root = match namespace {
        "var" => sources.var,
        "sys" => sources.sys,
        "argv" => nil_argv(sources.argv, path, offset)?,
        "args" => {
            return Err(failure(
                "not_a_table",
                offset,
                "args is a string, not a table".to_string(),
            ));
        }
        "item" => {
            return Err(failure(
                "not_a_table",
                offset,
                "item takes no key path; {{ item }} renders the whole item".to_string(),
            ));
        }
        other => {
            global_value = bare_global_root(other, path, offset, sources.globals)?;
            &global_value
        }
    };

    let mut current = root;
    match keys {
        Some(keys) => {
            for key in keys.split('.') {
                current = current.get(key).ok_or_else(|| {
                    failure(
                        "missing_key",
                        offset,
                        format!("missing {{{{ {} }}}}", path_preview(path)),
                    )
                })?;
            }
        }
        // A bare `var`/`sys` with no key stays an error; a bare global with
        // no keys renders its whole JSON value.
        None if matches!(namespace, "var" | "sys") => {
            return Err(failure(
                "bad_path",
                offset,
                format!("bad path: {{{{ {} }}}}", path_preview(path)),
            ));
        }
        None => {}
    }
    render(current, path, offset)
}

/// Renders a prompt-controlled placeholder path for a diagnostic.
///
/// Control characters are escaped and the text is truncated to a bounded length
/// so a hostile or malformed placeholder cannot forge multiline log records,
/// leak an oversized span, or smuggle control characters through `Display`.
fn path_preview(path: &str) -> String {
    use std::fmt::Write as _;
    const MAX_PREVIEW_CHARS: usize = 80;
    let mut out = String::with_capacity(path.len().min(MAX_PREVIEW_CHARS));
    for ch in path.chars().take(MAX_PREVIEW_CHARS) {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:04x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    if path.chars().count() > MAX_PREVIEW_CHARS {
        out.push_str("...");
    }
    out
}

/// Renders a resolved JSON value as its substituted string, per
/// [`render_item`](crate::lua::render_item); a null is an error.
fn render(value: &Value, path: &str, offset: usize) -> SubstResult<String> {
    if value.is_null() {
        return Err(failure(
            "null_value",
            offset,
            format!("missing {{{{ {} }}}}", path_preview(path)),
        ));
    }
    Ok(crate::lua::render_item(value))
}

#[cfg(test)]
#[path = "subst-tests.rs"]
mod tests;
