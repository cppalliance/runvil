//! Chunk names and chunk-line mapping: the `line N` name a prompt block
//! compiles under, and the rewrite of chunk-relative lines to prompt lines.

use std::num::NonZeroU32;

/// The text Lua writes in front of a string chunk's name where it gives a
/// position in that chunk, as in `[string "line 7"]:2:`.
pub(crate) const STRING_CHUNK_OPENING: &str = "[string \"";

/// The start of a prompt block's chunk name, `line N`, where `N` is the
/// prompt line the block starts on.
pub(crate) const PROMPT_CHUNK_PREFIX: &str = "line ";

/// The prompt-source line a compiler `diagnostic` names, for the chunk
/// named `chunk_name` that starts on `source_line`: the chunk-relative
/// line after the chunk's marker, mapped to its absolute line, or
/// `source_line` when the diagnostic names none or the mapping overflows.
pub(crate) fn compile_line(
    chunk_name: &str,
    source_line: NonZeroU32,
    diagnostic: &str,
) -> NonZeroU32 {
    let marker = format!("{STRING_CHUNK_OPENING}{chunk_name}\"]:");
    diagnostic
        .find(&marker)
        .map(|at| &diagnostic[at + marker.len()..])
        .and_then(|rest| {
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            absolute_line(source_line, rest[..digits].parse().ok()?)
        })
        .and_then(NonZeroU32::new)
        .unwrap_or(source_line)
}

/// The prompt line of line `chunk_line` of the chunk named `chunk_name`,
/// when that is a prompt block's name, `line N`: the block's line `N`
/// mapped as [`absolute_line`] maps it. `None` for any other name, or when
/// the line overflows `u32`.
pub(crate) fn prompt_line(chunk_name: &str, chunk_line: u32) -> Option<NonZeroU32> {
    let start = chunk_name.strip_prefix(PROMPT_CHUNK_PREFIX)?;
    if !start.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    absolute_line(start.parse().ok()?, chunk_line).and_then(NonZeroU32::new)
}

/// The prompt line of the first position in `text` that Lua writes for a
/// prompt block's line, `[string "line N"]:M:`, as a traceback names its
/// innermost author frame. The `<[string "line N"]:K>` that names where a
/// function is defined has no colon after its line and is skipped.
pub(crate) fn first_prompt_line(text: &str) -> Option<NonZeroU32> {
    text.match_indices(STRING_CHUNK_OPENING)
        .find_map(|(at, _)| {
            let (chunk_name, rest) = text[at + STRING_CHUNK_OPENING.len()..].split_once("\"]:")?;
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            if !rest[digits..].starts_with(':') {
                return None;
            }
            prompt_line(chunk_name, rest[..digits].parse().ok()?)
        })
}

/// Rewrites chunk-relative line numbers for the named chunks in `owners`,
/// each a chunk name, the display location it maps to, and the
/// prompt-source line its chunk starts at, to absolute prompt-source lines,
/// and returns the rewritten message beside the first line it rewrote.
///
/// Only `[string "{chunk name}"]:N:` occurrences of an owner's chunk name
/// are rewritten, each through the first owner whose chunk name it names, to
/// `{location}:{line}:`, the form Lua gives a file chunk. No later pass
/// matches that form, so a parent VM that surfaces a called or fanout
/// child's mapped error leaves the child's absolute lines alone, even for a
/// library both VMs loaded. The message gains a single leading
/// `{location}:{line}:` tag for the first occurrence rewritten; a message
/// with none passes through unchanged, with no line. A chunk line whose
/// absolute mapping would overflow `u32` keeps its original marker and
/// digits.
pub(crate) fn map_chunk_line_to_absolute(
    message: &str,
    owners: &[(&str, &str, NonZeroU32)],
) -> (String, Option<NonZeroU32>) {
    let markers: Vec<(String, &str, NonZeroU32)> = owners
        .iter()
        .filter(|(chunk_name, location, _)| !chunk_name.is_empty() && !location.is_empty())
        .map(|&(chunk_name, location, source_line)| {
            (
                format!("{STRING_CHUNK_OPENING}{chunk_name}\"]:"),
                location,
                source_line,
            )
        })
        .collect();
    let mut result = String::with_capacity(message.len() + 64);
    let mut rest = message;
    let mut tag: Option<(&str, u32)> = None;
    while let Some(start) = rest.find(STRING_CHUNK_OPENING) {
        result.push_str(&rest[..start]);
        let at = &rest[start..];
        let Some((marker, location, source_line)) = markers
            .iter()
            .find(|(marker, ..)| at.starts_with(marker.as_str()))
        else {
            result.push_str(STRING_CHUNK_OPENING);
            rest = &at[STRING_CHUNK_OPENING.len()..];
            continue;
        };
        let after = &at[marker.len()..];
        let digit_end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        let absolute = after[..digit_end]
            .parse()
            .ok()
            .and_then(|chunk_line| absolute_line(*source_line, chunk_line));
        if let Some(absolute) = absolute {
            tag.get_or_insert((location, absolute));
            result.push_str(location);
            result.push(':');
            result.push_str(&absolute.to_string());
            rest = &after[digit_end..];
        } else {
            result.push_str(marker);
            rest = after;
        }
    }
    result.push_str(rest);

    if let Some((location, absolute)) = tag {
        // Leading `location:line:` tag naming the block's display location
        // and its absolute line: ``section `Web Search` epilog:51: ...``
        (
            format!("{location}:{absolute}: {result}"),
            NonZeroU32::new(absolute),
        )
    } else {
        (result, None)
    }
}

/// The absolute prompt-source line of `chunk_line`, a chunk-relative line,
/// in a chunk starting at `source_line`: the two summed, less one. `None`
/// when the sum overflows `u32`, so a pathological line count cannot wrap
/// into a wrong number.
fn absolute_line(source_line: NonZeroU32, chunk_line: u32) -> Option<u32> {
    source_line.get().checked_add(chunk_line)?.checked_sub(1)
}
