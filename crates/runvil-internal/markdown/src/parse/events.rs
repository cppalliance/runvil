//! Layer 1: what `pulldown-cmark` reports.
//!
//! This is the only module that names the library. It makes one pass over
//! the events and records each top-level heading, fence, list, and rule
//! as a [`Mark`]: what the construct is and the bytes it covers. Anything
//! nested in another construct is left alone, and so is everything the
//! library reports that is none of the four.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

/// One top-level construct of a body.
pub(super) struct Mark {
    /// The bytes of the body the construct covers.
    pub(super) range: Range<usize>,
    /// What the construct is.
    pub(super) what: What,
}

/// What a [`Mark`] records.
pub(super) enum What {
    /// A heading.
    Heading {
        /// The heading's level, 1 to 6.
        level: u8,
        /// The heading's text and code spans, with a line break read as a
        /// space.
        title: String,
    },
    /// A fenced code block.
    Fence {
        /// The info string as written.
        info: String,
        /// The lines between the fence lines, each with its line ending.
        text: String,
    },
    /// A bullet or numbered list.
    List {
        /// Whether the list is numbered.
        ordered: bool,
        /// The list's own items, in order.
        items: Vec<Item>,
    },
    /// A thematic break.
    Rule,
}

/// One item of a top-level list.
pub(super) struct Item {
    /// The bytes of the body the item covers, its marker included.
    pub(super) range: Range<usize>,
    /// The byte the item's content starts at, or `None` for an item that
    /// has none.
    pub(super) content: Option<usize>,
}

/// Returns the top-level headings, fences, lists, and rules of `body`, in
/// file order. No two overlap.
///
/// The body is read as CommonMark with no extensions, the options the
/// prompt parser uses.
pub(super) fn marks(body: &str) -> Vec<Mark> {
    let mut reader = Reader::default();
    for (event, range) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        reader.read(event, range);
    }
    reader.marks
}

/// The state of one pass over the events.
#[derive(Default)]
struct Reader {
    /// The marks recorded so far.
    marks: Vec<Mark>,
    /// How many constructs are open around the next event.
    depth: usize,
    /// The top-level construct being read, when it is one a mark records.
    open: Option<Mark>,
}

impl Reader {
    /// Takes the next event and the bytes it covers.
    fn read(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => {
                if self.depth == 0 {
                    self.open = opened(&tag).map(|what| Mark { range, what });
                } else {
                    self.inside(self.depth == 1 && matches!(tag, Tag::Item), range);
                }
                self.depth += 1;
            }
            Event::End(_) => {
                self.depth = self.depth.saturating_sub(1);
                if self.depth == 0 {
                    self.marks.extend(self.open.take());
                }
            }
            Event::Rule if self.depth == 0 => self.marks.push(Mark {
                range,
                what: What::Rule,
            }),
            Event::Text(text) | Event::Code(text) => self.text(&text, range),
            Event::SoftBreak | Event::HardBreak => self.text(" ", range),
            _ => self.inside(false, range),
        }
    }

    /// Takes a run of text inside the open construct.
    fn text(&mut self, piece: &str, range: Range<usize>) {
        match self.open.as_mut().map(|mark| &mut mark.what) {
            Some(What::Heading { title, .. }) => title.push_str(piece),
            Some(What::Fence { text, .. }) => text.push_str(piece),
            _ => self.inside(false, range),
        }
    }

    /// Takes an event inside the open construct that is not a heading's
    /// or a fence's text. In a list, an event that `opens_item` starts
    /// the next item, and any other event is content of the last one.
    fn inside(&mut self, opens_item: bool, range: Range<usize>) {
        let Some(What::List { items, .. }) = self.open.as_mut().map(|mark| &mut mark.what) else {
            return;
        };
        if opens_item {
            items.push(Item {
                range,
                content: None,
            });
        } else if let Some(item) = items.last_mut() {
            item.content.get_or_insert(range.start);
        }
    }
}

/// Returns what a mark records for the top-level construct `tag` opens,
/// or `None` when it is not one of the four.
fn opened(tag: &Tag<'_>) -> Option<What> {
    match tag {
        Tag::Heading { level, .. } => Some(What::Heading {
            level: *level as u8,
            title: String::new(),
        }),
        Tag::CodeBlock(CodeBlockKind::Fenced(info)) => Some(What::Fence {
            info: info.to_string(),
            text: String::new(),
        }),
        Tag::List(first) => Some(What::List {
            ordered: first.is_some(),
            items: Vec::new(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Mark, What, marks};

    /// Each mark of `body` as the text it covers and a word for its kind.
    fn covered(body: &str) -> Vec<(&str, &'static str)> {
        marks(body)
            .iter()
            .map(|Mark { range, what }| {
                let kind = match what {
                    What::Heading { .. } => "heading",
                    What::Fence { .. } => "fence",
                    What::List { .. } => "list",
                    What::Rule => "rule",
                };
                (&body[range.clone()], kind)
            })
            .collect()
    }

    #[test]
    fn each_top_level_construct_is_one_mark_and_a_paragraph_is_none() {
        let body = "Intro.\n\n# Title\n\n- a\n- b\n\n```lua\nx\n```\n\n---\n\nOutro.\n";
        // A mark covers whole lines of its construct, and may take the
        // line ending or the blank line after it.
        assert_eq!(
            covered(body),
            [
                ("# Title\n", "heading"),
                ("- a\n- b\n\n", "list"),
                ("```lua\nx\n```", "fence"),
                ("---\n", "rule"),
            ]
        );
    }

    #[test]
    fn a_dashed_line_after_a_bullet_marker_is_a_rule_not_an_item() {
        assert_eq!(
            covered("- a\n- ---\n- b\n"),
            [("- a\n", "list"), ("- ---\n", "rule"), ("- b\n", "list")]
        );
    }

    #[test]
    fn a_construct_inside_a_block_quote_or_a_list_item_is_not_a_mark() {
        assert!(marks("> # Quoted\n>\n> - a\n>\n> ```\n> x\n> ```\n>\n> ---\n").is_empty());
        let body = "- a\n\n  # Inner\n\n  ---\n\n  ```\n  x\n  ```\n";
        assert_eq!(covered(body), [(body, "list")]);
    }

    #[test]
    fn a_heading_reports_its_level_and_its_text_without_markup() {
        let titles: Vec<(u8, String)> =
            marks("## The *big* `foo` [API](x) ##\n\nA\nB\n===\n\n###\n")
                .into_iter()
                .filter_map(|mark| match mark.what {
                    What::Heading { level, title } => Some((level, title)),
                    _ => None,
                })
                .collect();
        assert_eq!(
            titles,
            [
                (2, "The big foo API".to_owned()),
                (1, "A B".to_owned()),
                (3, String::new()),
            ]
        );
    }

    #[test]
    fn a_fence_reports_its_info_string_and_its_lines() {
        let fences: Vec<(String, String)> = marks("~~~json strict\n{ }\n\n# not a heading\n~~~\n")
            .into_iter()
            .filter_map(|mark| match mark.what {
                What::Fence { info, text } => Some((info, text)),
                _ => None,
            })
            .collect();
        assert_eq!(
            fences,
            [(
                "json strict".to_owned(),
                "{ }\n\n# not a heading\n".to_owned()
            )]
        );
    }

    #[test]
    fn a_list_reports_each_of_its_own_items_and_where_its_content_starts() {
        let body = "1. first\n   - nested\n2.\n3. third\n";
        let marks = marks(body);
        let [
            Mark {
                what: What::List { ordered, items },
                ..
            },
        ] = marks.as_slice()
        else {
            panic!("the body is one list");
        };
        assert!(*ordered);
        // Each item as the text it covers, and its content when it has any.
        let items: Vec<(&str, Option<&str>)> = items
            .iter()
            .map(|item| {
                (
                    &body[item.range.clone()],
                    item.content.map(|at| &body[at..item.range.end]),
                )
            })
            .collect();
        assert_eq!(
            items,
            [
                ("1. first\n   - nested\n", Some("first\n   - nested\n")),
                ("2.\n", None),
                ("3. third\n", Some("third\n")),
            ]
        );
    }
}
