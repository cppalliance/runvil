//! Detects list-only sections and extracts their bullet items.

use runvil_types::error::Error;

use crate::Result;

/// Returns the items of a list section's prose, or `None` when the prose is
/// not a list. Prose is a list when every nonblank line is a bullet marker
/// and at least one is, so one incidental bullet in ordinary prose leaves it
/// prose.
///
/// # Errors
/// Returns the failure `empty_item` builds from the first marker with no
/// content, trimmed and sliced from `prose`, when the prose is a list.
pub(crate) fn bullet_items<'a>(
    prose: &'a str,
    empty_item: impl FnOnce(&'a str) -> Error,
) -> Result<Option<Vec<&'a str>>> {
    let mut items = Vec::new();
    let mut empty_marker = None;
    for line in prose.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match classify_list_line(trimmed) {
            ListLine::Item(content) => items.push(content),
            ListLine::EmptyMarker => empty_marker = empty_marker.or(Some(trimmed)),
            ListLine::NotAMarker => return Ok(None),
        }
    }
    match empty_marker {
        Some(marker) => Err(empty_item(marker)),
        None => Ok((!items.is_empty()).then_some(items)),
    }
}

/// The classification of a single nonblank prose line as a list marker.
enum ListLine<'a> {
    /// A well-formed marker with nonblank content (the returned item text).
    Item(&'a str),
    /// A marker with no content: `-`, `*`, `1.`, `1. `, `1)`.
    EmptyMarker,
    /// Not a list marker at all (ordinary prose).
    NotAMarker,
}

/// Classifies one already-trimmed, nonblank line as a list marker.
fn classify_list_line(trimmed: &str) -> ListLine<'_> {
    // Unordered: `- item` / `* item`, or a bare `-` / `*`.
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return if rest.trim().is_empty() {
            ListLine::EmptyMarker
        } else {
            ListLine::Item(rest)
        };
    }
    if trimmed == "-" || trimmed == "*" {
        return ListLine::EmptyMarker;
    }

    // Ordered: `N. item` / `N) item`, or a bare/empty `N.` / `N. ` / `N)`.
    let digits = trimmed
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return ListLine::NotAMarker;
    }
    let after_digits = &trimmed[digits..];
    let Some(after_punct) = after_digits
        .strip_prefix('.')
        .or_else(|| after_digits.strip_prefix(')'))
    else {
        return ListLine::NotAMarker;
    };
    if after_punct.is_empty() {
        // `1.` / `1)` - a bare marker with no separating space or content.
        return ListLine::EmptyMarker;
    }
    // A valid ordered item requires a space after the marker punctuation, so
    // `1.foo` is prose, not a list item.
    if let Some(content) = after_punct.strip_prefix(' ') {
        return if content.trim().is_empty() {
            ListLine::EmptyMarker
        } else {
            ListLine::Item(content)
        };
    }
    ListLine::NotAMarker
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_bare_markers_are_classified() {
        assert!(matches!(
            classify_list_line("- item"),
            ListLine::Item("item")
        ));
        assert!(matches!(
            classify_list_line("1. item"),
            ListLine::Item("item")
        ));
        assert!(matches!(classify_list_line("-"), ListLine::EmptyMarker));
        assert!(matches!(classify_list_line("1."), ListLine::EmptyMarker));
        assert!(matches!(classify_list_line("1. "), ListLine::EmptyMarker));
        assert!(matches!(classify_list_line("1)"), ListLine::EmptyMarker));
        assert!(matches!(classify_list_line("1.foo"), ListLine::NotAMarker));
        assert!(matches!(
            classify_list_line("plain prose"),
            ListLine::NotAMarker
        ));
    }

    #[test]
    fn all_markers_predicate() {
        fn items(prose: &str) -> Option<Vec<&str>> {
            bullet_items(prose, |marker| panic!("no empty marker, got {marker:?}"))
                .expect("a prose with no empty marker is never refused")
        }
        assert_eq!(items("- a\n1. b"), Some(vec!["a", "b"]));
        assert_eq!(items("- a\nplain line"), None);
        assert_eq!(items("   \n  "), None);
    }
}
