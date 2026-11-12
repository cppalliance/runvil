//! The frontmatter that may open a text.

use serde_json::Value as Json;

/// The frontmatter split off the front of a text.
pub(super) struct Opening {
    /// The YAML mapping, as a JSON object.
    pub(super) data: Json,
    /// The lines the frontmatter takes, both delimiters included.
    pub(super) lines: usize,
    /// The byte the rest of the text starts at.
    pub(super) end: usize,
}

/// Splits the frontmatter off the front of `text`, whose line endings are
/// all `\n`.
///
/// There is frontmatter only when the first line is `---`, a later line is
/// `---`, and the lines between are a YAML mapping. The delimiter rule is
/// the prompt parser's: the opening line may have spaces on either side of
/// its dashes, and the closing line only after them. In every other case
/// the text has no frontmatter and its first lines are ordinary Markdown.
pub(super) fn split(text: &str) -> Option<Opening> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim() != "---" {
        return None;
    }
    let mut end = first.len();
    let mut yaml = String::new();
    for (index, line) in lines.enumerate() {
        end += line.len();
        if line.trim_end() == "---" {
            let data: Json = serde_yaml_ng::from_str(&yaml).ok()?;
            return data.is_object().then_some(Opening {
                data,
                lines: index + 2,
                end,
            });
        }
        yaml.push_str(line);
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::split;

    #[test]
    fn a_mapping_between_two_delimiters_is_split_off_with_its_lines_counted() {
        let text = "---\ntitle: Notes\ntags: [a, b]\n---\n\nBody.\n";
        let opening = split(text).unwrap();
        assert_eq!(opening.data, json!({"title": "Notes", "tags": ["a", "b"]}));
        assert_eq!(opening.lines, 4);
        assert_eq!(&text[opening.end..], "\nBody.\n");
    }

    #[test]
    fn the_delimiters_follow_the_prompt_parsers_rule() {
        // Spaces may sit on either side of the opening dashes, and only
        // after the closing ones: an indented `---` is YAML text.
        assert!(split("  ---  \na: 1\n---  \n").is_some());
        assert!(split("---\na: 1\n ---\n").is_none());
        assert!(split("---\na: |\n  x\n  ---\n---\n").is_some());
        // The closing delimiter may be the last line, with no line ending.
        let opening = split("---\na: 1\n---").unwrap();
        assert_eq!((opening.lines, opening.end), (3, 12));
    }

    #[test]
    fn text_without_both_delimiters_or_without_a_mapping_has_no_frontmatter() {
        for text in [
            "",
            "# Title\n",
            "\n---\na: 1\n---\n",
            "---\na: 1\n",
            "---\n---\n",
            "---\njust words\n---\n",
            "---\n- a\n- b\n---\n",
            "---\na: [1\n---\n",
        ] {
            assert!(split(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn yaml_values_read_as_json() {
        let data = |yaml: &str| split(&format!("---\n{yaml}\n---\n")).unwrap().data;
        // A key that is not a string reads as its text.
        assert_eq!(data("1: a\ntrue: b"), json!({"1": "a", "true": "b"}));
        // A number JSON cannot hold is null, and a date is a string.
        assert_eq!(
            data("a: .inf\nb: .nan\nc: 2026-11-11\nd: ~"),
            json!({"a": null, "b": null, "c": "2026-11-11", "d": null})
        );
        // A repeated key keeps its last value.
        assert_eq!(data("a: 1\na: 2"), json!({"a": 2}));
        assert_eq!(
            data("a: {b: [1, 2.5, x]}"),
            json!({"a": {"b": [1, 2.5, "x"]}})
        );
    }
}
