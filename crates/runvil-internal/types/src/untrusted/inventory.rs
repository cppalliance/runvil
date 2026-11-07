//! The control-markup delimiter inventory and its single-pass matcher.
//!
//! Chat templates address their models through control markup: role headers,
//! turn boundaries, tool-call envelopes, media placeholders, and document
//! boundaries. Untrusted content quoting one verbatim can forge a turn or
//! close a block the template opened, so the envelope's encoding pass spaces
//! the opener of every delimiter listed here. The inventory is ported from
//! the upstream closed name list and stays closed on purpose: `<div>`,
//! `List<String>`, `[1]`, and lowercase `[inst]` are prose and stay as
//! typed. It includes structural tokens that tokenizers do not flag as
//! special, because a string sweep keyed on the special-token flag misses
//! real delimiters.
//!
//! DeepSeek's fullwidth markers (`<｜name｜>`, U+FF5C bars) are the one open
//! name class - the family keeps adding spellings - so they match as a
//! bounded class rather than as table entries.

#[path = "inventory-table.rs"]
mod table;

#[cfg(test)]
pub(super) use table::{CONTROL_MARKUP, Shape};
#[cfg(not(test))]
use table::{CONTROL_MARKUP, Shape};

/// The fullwidth vertical bar (U+FF5C) framing DeepSeek's open marker class.
const FULLWIDTH_BAR: char = '\u{ff5c}';

/// The most characters allowed after the first letter of a fullwidth marker
/// name, matching the upstream `{0,39}` bound.
const FULLWIDTH_NAME_MAX: usize = 39;

/// The length in bytes of the inventory delimiter opening at the start of
/// `text`, if one does.
///
/// `prev_lt` records whether the character before `text` was a `<`, which
/// anchors the doubled-angle `<<SYS>>` form on its second bracket.
pub(super) fn delimiter_len(text: &str, prev_lt: bool) -> Option<usize> {
    match text.as_bytes().first() {
        Some(b'[') => literal_len(text, b'['),
        Some(b'<') => angle_len(text, prev_lt),
        _ => None,
    }
}

/// The length of the angle-bracket delimiter at the start of `text`.
fn angle_len(text: &str, prev_lt: bool) -> Option<usize> {
    pipe_len(text)
        .or_else(|| bare_tag_len(text))
        .or_else(|| literal_len(text, b'<'))
        .or_else(|| attribute_len(text))
        .or_else(|| fullwidth_len(text))
        .or_else(|| doubled_angle_len(text, prev_lt))
}

/// The length of the `<|name|>`-family delimiter at the start of `text`.
///
/// The closer form `<|/name|>` shares the opener. The name must be followed
/// by `|>` or `>`, so `<|tool_call|>` cannot match as the shorter `tool`.
fn pipe_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix("<|")?;
    let (close, rest) = rest.strip_prefix('/').map_or((0, rest), |r| (1, r));
    for group in CONTROL_MARKUP {
        if !matches!(group.shape, Shape::Pipe) {
            continue;
        }
        for name in group.names {
            let Some(after) = rest.strip_prefix(name) else {
                continue;
            };
            let term = if after.starts_with("|>") {
                2
            } else if after.starts_with('>') {
                1
            } else {
                continue;
            };
            return Some(2 + close + name.len() + term);
        }
    }
    None
}

/// The length of the `<name>`-family bare tag at the start of `text`.
fn bare_tag_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('<')?;
    let (close, rest) = rest.strip_prefix('/').map_or((0, rest), |r| (1, r));
    for group in CONTROL_MARKUP {
        if !matches!(group.shape, Shape::BareTag) {
            continue;
        }
        for name in group.names {
            if let Some(after) = rest.strip_prefix(name)
                && after.starts_with('>')
            {
                return Some(1 + close + name.len() + 1);
            }
        }
    }
    None
}

/// The length of the literal opener at the start of `text` whose first byte
/// is `open`.
fn literal_len(text: &str, open: u8) -> Option<usize> {
    for group in CONTROL_MARKUP {
        if !matches!(group.shape, Shape::Literal) {
            continue;
        }
        for lit in group.names {
            if lit.as_bytes().first() == Some(&open) && text.starts_with(lit) {
                return Some(lit.len());
            }
        }
    }
    None
}

/// The length of the `<function=...` / `<name param="...">` attribute opener
/// at the start of `text`, where the `name=` form allows any whitespace run.
fn attribute_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('<')?;
    for name in ["function", "parameter"] {
        if let Some(after) = rest.strip_prefix(name)
            && after.starts_with('=')
        {
            return Some(1 + name.len() + 1);
        }
    }
    for name in ["function", "parameter", "param"] {
        if let Some(after) = rest.strip_prefix(name) {
            let trimmed = after.trim_start_matches(char::is_whitespace);
            let ws = after.len() - trimmed.len();
            if ws > 0 && trimmed.starts_with("name=\"") {
                return Some(1 + name.len() + ws + "name=\"".len());
            }
        }
    }
    None
}

/// The length of the fullwidth `<｜name｜>` marker at the start of `text`.
///
/// The one open name class: an ASCII letter, then at most
/// [`FULLWIDTH_NAME_MAX`] joiner characters, then the closing bar. The
/// charset restriction keeps the class off real CJK content.
fn fullwidth_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('<')?;
    let mut rest = rest.strip_prefix(FULLWIDTH_BAR)?;
    let first = rest.chars().next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    rest = &rest[first.len_utf8()..];
    let mut total = 1 + FULLWIDTH_BAR.len_utf8() + first.len_utf8();
    let mut extra = 0usize;
    loop {
        let c = rest.chars().next()?;
        if c == FULLWIDTH_BAR {
            let after = &rest[FULLWIDTH_BAR.len_utf8()..];
            return after
                .starts_with('>')
                .then_some(total + FULLWIDTH_BAR.len_utf8() + 1);
        }
        if extra >= FULLWIDTH_NAME_MAX || !is_fullwidth_name_char(c) {
            return None;
        }
        extra += 1;
        total += c.len_utf8();
        rest = &rest[c.len_utf8()..];
    }
}

/// Whether `c` may continue a fullwidth marker name: ASCII letters, the
/// one-quarter-block joiner DeepSeek uses, underscore, space, or backslash
/// (the upstream parser also recognizes backslash-escaped spellings).
fn is_fullwidth_name_char(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '\u{2581}' | '_' | ' ' | '\\')
}

/// The length of the `<<SYS>>` system-block opener at the start of `text`,
/// anchored on the second bracket via `prev_lt`.
fn doubled_angle_len(text: &str, prev_lt: bool) -> Option<usize> {
    if !prev_lt {
        return None;
    }
    for group in CONTROL_MARKUP {
        if !matches!(group.shape, Shape::DoubledAngle) {
            continue;
        }
        for name in group.names {
            let Some(rest) = text.strip_prefix('<') else {
                continue;
            };
            let (close, rest) = rest.strip_prefix('/').map_or((0, rest), |r| (1, r));
            if let Some(after) = rest.strip_prefix(name)
                && after.starts_with(">>")
            {
                return Some(1 + close + name.len() + 2);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! Tests for the control-markup inventory: table sanity, and the matcher's
    //! spellings, bounds, and prose refusals.

    use super::*;

    /// Every full spelling of every Pipe, BareTag, and Literal table entry.
    fn spellings() -> Vec<String> {
        let mut out = Vec::new();
        for group in CONTROL_MARKUP {
            for name in group.names {
                match group.shape {
                    Shape::Pipe => {
                        out.push(format!("<|{name}|>"));
                        out.push(format!("<|{name}>"));
                        out.push(format!("<|/{name}|>"));
                        out.push(format!("<|/{name}>"));
                    }
                    Shape::BareTag => {
                        out.push(format!("<{name}>"));
                        out.push(format!("</{name}>"));
                    }
                    Shape::Literal => out.push((*name).to_owned()),
                    Shape::DoubledAngle => {}
                }
            }
        }
        out
    }

    #[test]
    fn every_table_spelling_matches_in_full() {
        for s in spellings() {
            assert_eq!(
                delimiter_len(&s, false),
                Some(s.len()),
                "inventory spelling {s:?} did not match in full"
            );
        }
    }

    #[test]
    fn inventory_has_no_duplicate_spellings() {
        let all = spellings();
        let mut deduped = all.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            all.len(),
            deduped.len(),
            "duplicate spellings in the inventory"
        );
    }

    #[test]
    fn every_group_names_a_family_and_entries() {
        for group in CONTROL_MARKUP {
            assert!(
                !group.family.is_empty(),
                "a group is missing its family label"
            );
            assert!(
                !group.names.is_empty(),
                "family {:?} has no entries",
                group.family
            );
        }
    }

    #[test]
    fn fullwidth_class_matches_known_and_novel_spellings() {
        let user = "<\u{ff5c}User\u{ff5c}>";
        assert_eq!(delimiter_len(user, false), Some(user.len()));
        let sentence = "<\u{ff5c}begin\u{2581}of\u{2581}sentence\u{ff5c}>";
        assert_eq!(delimiter_len(sentence, false), Some(sentence.len()));
        let novel = "<\u{ff5c}SomeNew\u{ff5c}>";
        assert_eq!(
            delimiter_len(novel, false),
            Some(novel.len()),
            "the open class admits spellings added after the port"
        );
    }

    #[test]
    fn fullwidth_class_stays_bounded_and_off_prose() {
        assert_eq!(
            delimiter_len("<\u{ff5c}\u{ff5c}>", false),
            None,
            "the name must start with a letter"
        );
        let at_bound = format!("<\u{ff5c}a{}\u{ff5c}>", "b".repeat(FULLWIDTH_NAME_MAX));
        assert_eq!(
            delimiter_len(&at_bound, false),
            Some(at_bound.len()),
            "a name at the 39-character bound still matches"
        );
        let past_bound = format!("<\u{ff5c}a{}\u{ff5c}>", "b".repeat(FULLWIDTH_NAME_MAX + 1));
        assert_eq!(
            delimiter_len(&past_bound, false),
            None,
            "the class is bounded"
        );
        assert_eq!(
            delimiter_len("<\u{ff5c} \u{ff5c}>", false),
            None,
            "fullwidth punctuation pairs stay as typed"
        );
    }

    #[test]
    fn doubled_angle_sys_matches_only_at_the_second_bracket() {
        assert_eq!(delimiter_len("<<SYS>>", false), None);
        assert_eq!(delimiter_len("<SYS>>", true), Some("<SYS>>".len()));
        assert_eq!(delimiter_len("</SYS>>", true), Some("</SYS>>".len()));
        assert_eq!(
            delimiter_len("<SYS>>", false),
            None,
            "a single angle stays as typed"
        );
        assert_eq!(
            delimiter_len("< SYS>>", true),
            None,
            "the spaced form is stable"
        );
    }

    #[test]
    fn closed_inventory_leaves_prose_alone() {
        let prose = [
            "<div>",
            "<span class=\"x\">",
            "List<String>",
            "[1]",
            "[inst]",
            "[UNKNOWN]",
            "<s >",
            "<tool>",
            "<|tool|",
            "<|unknown_name|>",
        ];
        for text in prose {
            assert_eq!(
                delimiter_len(text, false),
                None,
                "{text:?} must stay as typed"
            );
        }
    }
}
