//! Layer 3: the flat form folded into sections.

use super::flat::Flat;
use crate::Block;

/// A section whose heading has been read and whose end has not.
struct Open {
    /// The heading's line.
    line: usize,
    /// The heading's source.
    source: String,
    /// The heading's level.
    level: u8,
    /// The heading's title.
    title: String,
    /// The blocks placed in the section so far.
    blocks: Vec<Block>,
}

/// Folds `flat` into a tree and returns it after the blocks of `root`.
///
/// A heading opens a section, which holds every entry up to the next
/// heading of the same or a smaller level. A heading of a larger level
/// opens a subsection inside it, however many levels it skips.
pub(super) fn fold(flat: Vec<Flat>, mut root: Vec<Block>) -> Vec<Block> {
    let mut open: Vec<Open> = Vec::new();
    for entry in flat {
        match entry {
            Flat::Heading {
                line,
                source,
                level,
                title,
            } => {
                close(&mut open, &mut root, level);
                open.push(Open {
                    line,
                    source,
                    level,
                    title,
                    blocks: Vec::new(),
                });
            }
            Flat::Leaf(block) => place(&mut open, &mut root, block),
        }
    }
    close(&mut open, &mut root, 0);
    root
}

/// Closes every open section at `level` or deeper, innermost first.
fn close(open: &mut Vec<Open>, root: &mut Vec<Block>, level: u8) {
    while let Some(section) = open.pop_if(|section| section.level >= level) {
        let block = Block::Section {
            line: section.line,
            source: section.source,
            level: section.level,
            title: section.title,
            blocks: section.blocks,
        };
        place(open, root, block);
    }
}

/// Puts `block` in the innermost open section, or in `root` when none is
/// open.
fn place(open: &mut [Open], root: &mut Vec<Block>, block: Block) {
    match open.last_mut() {
        Some(section) => section.blocks.push(block),
        None => root.push(block),
    }
}

#[cfg(test)]
mod tests {
    use super::super::flat::Flat;
    use super::fold;
    use crate::Block;

    fn heading(level: u8, title: &str) -> Flat {
        Flat::Heading {
            line: 0,
            source: format!("{} {title}\n", "#".repeat(usize::from(level))),
            level,
            title: title.to_owned(),
        }
    }

    fn rule() -> Flat {
        Flat::Leaf(Block::Rule {
            line: 0,
            source: "---\n".to_owned(),
        })
    }

    /// The tree as nested text: a section is `title(members)` and a rule
    /// is `-`.
    fn shape(blocks: &[Block]) -> String {
        blocks
            .iter()
            .map(|block| match block {
                Block::Section { title, blocks, .. } => format!("{title}({})", shape(blocks)),
                _ => "-".to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_heading_closes_every_open_section_at_its_level_or_deeper() {
        let flat = vec![
            rule(),
            heading(1, "A"),
            rule(),
            heading(2, "B"),
            heading(3, "C"),
            rule(),
            heading(2, "D"),
            heading(1, "E"),
            rule(),
        ];
        assert_eq!(shape(&fold(flat, Vec::new())), "- A(- B(C(-)) D()) E(-)");
    }

    #[test]
    fn a_heading_that_skips_levels_is_a_member_of_the_section_above_it() {
        let flat = vec![heading(2, "A"), heading(4, "B"), heading(3, "C")];
        let tree = fold(flat, Vec::new());
        assert_eq!(shape(&tree), "A(B() C())");
        let Some(Block::Section { blocks, .. }) = tree.first() else {
            panic!("the tree opens with a section");
        };
        assert!(matches!(
            blocks.first(),
            Some(Block::Section { level: 4, .. })
        ));
    }

    #[test]
    fn a_shallower_heading_after_a_deeper_one_is_a_section_of_its_own() {
        let flat = vec![heading(3, "A"), heading(1, "B")];
        assert_eq!(shape(&fold(flat, Vec::new())), "A() B()");
    }

    #[test]
    fn the_tree_follows_the_blocks_it_was_given() {
        let root = vec![Block::Rule {
            line: 0,
            source: "---\n".to_owned(),
        }];
        assert_eq!(shape(&fold(vec![heading(1, "A")], root)), "- A()");
    }

    #[test]
    fn a_section_keeps_its_headings_source_and_folding_moves_no_source() {
        let flat = vec![heading(2, "A"), rule(), heading(3, "B"), heading(1, "C")];
        let tree = fold(flat, Vec::new());
        let [
            Block::Section { source, blocks, .. },
            Block::Section { source: last, .. },
        ] = tree.as_slice()
        else {
            panic!("the tree is two sections");
        };
        assert_eq!((source.as_str(), last.as_str()), ("## A\n", "# C\n"));
        let sources: Vec<&str> = blocks.iter().map(Block::source).collect();
        assert_eq!(sources, ["---\n", "### B\n"]);
    }
}
