//! Checks the strict `lua` fence rule and splits content into Lua and prose blocks.
//!
//! H1 and section content is an alternating sequence of `lua` fences and
//! prose. [`Prompt::parse`](crate::Prompt::parse) states the strict fence
//! rule. This module finds the first fence or line in the body that breaks
//! its Label, Opener, Closer, or Lookalike case, so the parse can reject it,
//! and splits content into [`RawBlock`]s, which this crate compiles into
//! [`Block`](crate::Block)s.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag};
use runvil_types::error::{Error, ErrorCode, FactKey};

use crate::{Result, error, position};

/// The message for a `lua` fence whose opening line breaks the Opener rule,
/// and for a Lookalike line.
const OPENER_MESSAGE: &str = "`lua` fence opening line must be exactly ```lua";

/// Uncompiled block produced while scanning section content.
pub(crate) enum RawBlock<'a> {
    Lua {
        source: String,
        /// Lines before the first line of Lua source within section content.
        line_offset: u32,
        /// The block's place in its section, which names it in its compile
        /// location.
        role: LuaRole,
    },
    Prose(&'a str),
}

/// A Lua block's place among its section's blocks.
pub(crate) enum LuaRole {
    /// The first block.
    Prologue,
    /// Neither first nor last, with its 1-based place among the section's
    /// Lua blocks.
    Middle(usize),
    /// The last block, when it is not also the first.
    Epilog,
}

/// Removes complete leading lines that contain only whitespace.
pub(crate) fn trim_leading_blank_lines(content: &str) -> &str {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        if line.trim().is_empty() {
            offset += line.len();
        } else {
            break;
        }
    }
    &content[offset..]
}

/// Extracts source through the first closing line, the line the Markdown parser
/// closes the fence on. The validator refuses any closing line that is not
/// exactly three backticks before a split runs, so a split that meets one has
/// broken an invariant, and it reports an internal error.
fn extract_exact_fence<'a>(content: &'a str, location: &str) -> Result<(String, &'a str)> {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        let text = text.strip_suffix('\r').unwrap_or(text);
        if closes_three_backtick_fence(text) {
            if text != "```" {
                return Err(error::internal(
                    "parser: fence split met a closing line the validator refuses",
                ));
            }
            let source = content[..offset].trim_end_matches(['\r', '\n']).to_string();
            return Ok((source, &content[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(error::failure(
        ErrorCode::FenceInvalid,
        format!("{location} fence is not closed"),
    ))
}

/// Removes one exact unindented `lua` opening line from `content`.
fn strip_exact_lua_opening(content: &str) -> Option<&str> {
    content
        .strip_prefix("```lua\r\n")
        .or_else(|| content.strip_prefix("```lua\n"))
        .or_else(|| (content == "```lua").then_some(""))
}

/// Yields the extent and the label of every fence the Markdown parser
/// reports, in source order.
///
/// Each extent starts at the opening line's first byte and ends where the
/// parser ends the block. That includes fences inside block quotes and list
/// items, and never a fence inside another code block's body, so fence-looking
/// lines in a longer fence stay prose rather than being reserved or rejected.
fn fence_openings(content: &str) -> impl Iterator<Item = (Range<usize>, CowStr<'_>)> {
    Parser::new_ext(content, Options::empty())
        .into_offset_iter()
        .filter_map(|(event, range)| {
            let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(label))) = event else {
                return None;
            };
            let line_start = content[..range.start]
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            Some((line_start..range.end, label))
        })
}

/// Returns the byte offsets of exact `lua` opening lines.
fn exact_lua_openings(content: &str) -> Vec<usize> {
    fence_openings(content)
        .filter_map(|(extent, _)| {
            (content[extent.start..].lines().next() == Some("```lua")).then_some(extent.start)
        })
        .collect()
}

/// Returns the first violation in `body` in source order, as a fence
/// failure located at its line without the terminator, for a body that
/// starts in the file after `frontmatter_lines` lines.
///
/// Each fence the Markdown parser reports is checked against the Label rule,
/// then the Opener rule, then the Closer rule. A fence the parser leaves
/// unclosed reports nothing here; [`split_section_blocks`] reports it. Every
/// other line is checked against the Lookalike rule: after leading spaces and
/// tabs, three or more backticks or tildes, optional spaces or tabs, then
/// `lua` in any letter case. Such a line sits where the parser reports no
/// fence, such as in an indented code block or a raw HTML block, and fails
/// with the Opener rule's message. Lines inside a fence's extent are never
/// checked against the Lookalike rule, so a fence nested in another fence's
/// body never fails. The failure's `Reason` fact names the rule: `label`,
/// `opener`, `closer`, or `lookalike`.
pub(crate) fn first_fence_violation(body: &str, frontmatter_lines: u32) -> Option<Error> {
    let mut extents = Vec::new();
    let mut fence = None;
    for (extent, label) in fence_openings(body) {
        fence = fence.or_else(|| fence_violation(body, extent.clone(), &label));
        extents.push(extent);
    }
    let lookalike = body
        .split_inclusive('\n')
        .scan(0, |next, line| {
            let start = *next;
            *next += line.len();
            Some(start)
        })
        .filter(|start| !extents.iter().any(|extent| extent.contains(start)))
        .map(|start| line_at(body, start))
        .find(|line| is_lua_lookalike(&body[line.clone()]))
        .map(|line| (line, "lookalike", OPENER_MESSAGE.to_owned()));
    let (line, rule, message) = [fence, lookalike]
        .into_iter()
        .flatten()
        .min_by_key(|(line, ..)| line.start)?;
    let mut violation = error::failure_at(
        ErrorCode::FenceInvalid,
        message,
        body,
        line,
        frontmatter_lines,
    );
    violation.facts.insert(FactKey::Reason, rule.into());
    Some(violation)
}

/// Checks one reported fence against the Label, Opener, and Closer rules, in
/// that order, and returns the offending line's byte range, without its
/// terminator, the rule it breaks, and the message.
fn fence_violation(
    body: &str,
    extent: Range<usize>,
    label: &str,
) -> Option<(Range<usize>, &'static str, String)> {
    let opening = line_at(body, extent.start);
    if label != "lua" {
        return is_lua_like(label).then(|| {
            let message =
                format!("unrecognized fence label `{label}`; the only Lua fence label is `lua`");
            (opening, "label", message)
        });
    }
    if &body[opening.clone()] != "```lua" {
        return Some((opening, "opener", OPENER_MESSAGE.to_owned()));
    }
    let closing = closing_line(body, extent)?;
    (&body[closing.clone()] != "```").then(|| {
        let message = "`lua` fence closing line must be exactly ```".to_owned();
        (closing, "closer", message)
    })
}

/// Byte range of the line starting at `start`, without its terminator.
fn line_at(content: &str, start: usize) -> Range<usize> {
    let rest = &content[start..];
    let line = rest.find('\n').map_or(rest, |end| &rest[..end]);
    let line = line.strip_suffix('\r').unwrap_or(line);
    start..start + line.len()
}

/// Byte range of the closing line of an exact `lua` fence whose block covers
/// `extent`, or `None` when the block is unclosed.
///
/// The closing line is the block's own last line, so this always agrees
/// with the Markdown parser on where the fence ends. A block the parser
/// leaves unclosed ends on a body line, which never has a closing line's
/// shape, because such a line would have closed it.
fn closing_line(body: &str, extent: Range<usize>) -> Option<Range<usize>> {
    let block = body[extent.clone()].trim_end_matches(['\r', '\n']);
    let last = block.rfind('\n')? + 1;
    let line = line_at(body, extent.start + last);
    closes_three_backtick_fence(&body[line.clone()]).then_some(line)
}

/// Whether `line` has the shape of a line that closes a fence opened by three
/// backticks: at most three spaces of indent, three or more backticks, then
/// only spaces or tabs. The splitter and the validator both use this one rule.
fn closes_three_backtick_fence(line: &str) -> bool {
    let unindented = line.trim_start_matches(' ');
    let marker = unindented.trim_start_matches('`');
    line.len() - unindented.len() <= 3
        && unindented.len() - marker.len() >= 3
        && marker.trim_matches([' ', '\t']).is_empty()
}

/// Whether `line` breaks the Lookalike rule.
fn is_lua_lookalike(line: &str) -> bool {
    let text = line.trim_start_matches([' ', '\t']);
    let Some(marker) = text.chars().next().filter(|c| matches!(c, '`' | '~')) else {
        return false;
    };
    let after_marker = text.trim_start_matches(marker);
    text.len() - after_marker.len() >= 3
        && is_lua_like(after_marker.trim_start_matches([' ', '\t']))
}

/// Whether `text` starts with `lua` in any letter case.
fn is_lua_like(text: &str) -> bool {
    text.get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("lua"))
}

/// Byte offset where leading blank lines end.
fn leading_content_start(content: &str) -> usize {
    content.len() - trim_leading_blank_lines(content).len()
}

/// Byte ranges of every genuine thematic break in `content`.
///
/// Pulldown reports a `Rule` only for a real CommonMark thematic break: a
/// `---` inside a fenced code block is code, and a text line immediately
/// followed by `---` is a setext heading underline, never a rule.
fn rule_ranges(content: &str) -> Vec<Range<usize>> {
    Parser::new_ext(content, Options::empty())
        .into_offset_iter()
        .filter_map(|(event, range)| matches!(event, Event::Rule).then_some(range))
        .collect()
}

/// Returns the pending Markdown of one prose segment: everything after the
/// segment's last thematic break.
///
/// The break itself is never included, and commentary above it is excluded.
fn pending_prose<'a>(content: &'a str, segment: Range<usize>, rules: &[Range<usize>]) -> &'a str {
    let start = rules
        .iter()
        .filter(|rule| rule.start >= segment.start && rule.end <= segment.end)
        .map(|rule| rule.end)
        .max()
        .unwrap_or(segment.start);
    &content[start..segment.end]
}

/// Splits a section into alternating exact `lua` fences and prose segments.
///
/// Every exact `lua` fence the Markdown parser reports becomes a Lua block.
/// Text between fences becomes prose (including an empty segment between
/// consecutive fences, so a prologue and an epilog around empty prose stay
/// `[Lua, Prose, Lua]`). Leading blank lines before a leading fence are
/// discarded. Each prose segment is the pending Markdown buffer for the
/// fence that follows it: a thematic break resets the buffer, so only the
/// Markdown below the segment's last break is captured.
///
/// The parse runs [`first_fence_violation`] on the whole body first, so the
/// first exact closing line after an exact opener is the line the Markdown
/// parser closes the fence on.
///
/// # Errors
/// Returns a `FenceInvalid` failure when an exact `lua` fence is not
/// closed, and an `Internal` failure when an opening the Markdown parser
/// reports falls inside the previous fence's extracted source.
pub(crate) fn split_section_blocks<'a>(
    content: &'a str,
    section: &str,
) -> Result<Vec<RawBlock<'a>>> {
    let openings = exact_lua_openings(content);
    let rules = rule_ranges(content);
    if openings.is_empty() {
        let pending = pending_prose(content, 0..content.len(), &rules);
        return Ok(vec![RawBlock::Prose(pending.trim())]);
    }

    let leading_start = leading_content_start(content);
    let mut blocks = Vec::new();
    let mut pos = 0usize;

    for (index, &opening) in openings.iter().enumerate() {
        let after_open = if opening < pos {
            None
        } else {
            strip_exact_lua_opening(&content[opening..])
        };
        let Some(after_open) = after_open else {
            return Err(error::internal(
                "parser: section fence split disagrees with the Markdown parser",
            ));
        };
        // An unclosed fence runs to the end of its content, so it is always the last block.
        let location = if index == 0 && opening == leading_start {
            format!("section `{section}` prologue `lua`")
        } else {
            format!("section `{section}` epilog `lua`")
        };
        let (source, rest) = extract_exact_fence(after_open, &location)?;
        let fence_end = content.len() - rest.len();

        if !(pos == 0 && opening == leading_start) {
            let pending = pending_prose(content, pos..opening, &rules);
            blocks.push(RawBlock::Prose(pending.trim()));
        }

        let line_offset = position::line_add(position::newlines_before(content, opening)?, 1)?;
        blocks.push(RawBlock::Lua {
            source,
            line_offset,
            role: LuaRole::Middle(index + 1),
        });
        pos = fence_end;
    }

    let trailing = pending_prose(content, pos..content.len(), &rules).trim();
    if !trailing.is_empty() {
        blocks.push(RawBlock::Prose(trailing));
    }

    if let Some(RawBlock::Lua { role, .. }) = blocks.first_mut() {
        *role = LuaRole::Prologue;
    }
    if blocks.len() > 1
        && let Some(RawBlock::Lua { role, .. }) = blocks.last_mut()
    {
        *role = LuaRole::Epilog;
    }

    Ok(blocks)
}

/// The compile location of one Lua block, from its section's location and
/// its role.
///
/// A middle block is named by its 1-based place among the section's Lua
/// blocks. The location is the display name mapped runtime errors give the
/// block; mapping itself matches the block's line-based chunk name, so the
/// location need not be unique or short.
pub(crate) fn lua_block_location(section_location: &str, role: &LuaRole) -> String {
    match role {
        LuaRole::Prologue => format!("{section_location} prologue"),
        LuaRole::Epilog => format!("{section_location} epilog"),
        LuaRole::Middle(n) => format!("{section_location} lua {n}"),
    }
}
