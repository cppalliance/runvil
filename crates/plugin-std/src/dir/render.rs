//! The render: turns the walk's tree into the text ListDir answers with.
//!
//! The tree prints depth first in the sorted order, one line per entry,
//! each indented two spaces per level under a `- ` bullet, with a trailing
//! `/` on a directory only. The lines are joined with `\n` and there is no
//! newline after the last one.

use super::walk::{Node, Tree};

/// The note a cut listing ends with, after the `    ...` line and a blank
/// line.
const TOO_LARGE: &str = "target_directory is too large (either too many children or their \
     names are too long). Immediate children were truncated. Try a more specific \
     target_directory or ignore_globs.";

/// The line that follows the root line when nothing is left to show.
const NO_CHILDREN: &str = "... no children found ...";

/// The line that marks where a cut listing stops.
const CUT_MARK: &str = "    ...";

/// The text of `tree`.
///
/// A root with nothing to show prints its line and then `... no children
/// found ...`. A tree the walk cut ends with the cut mark, a blank line,
/// and the note that says to narrow the call. The cut mark and the note
/// do not count against the byte cap.
pub(super) fn render(tree: &Tree) -> String {
    // The walk always seeds the root as the first node and only pushes
    // indexes it has added, so indexing the nodes cannot miss. A broken
    // tree panics here instead of printing less than the walk found.
    let root = &tree.nodes[0];
    if root.children.is_empty() && !tree.truncated {
        return format!("{}\n{NO_CHILDREN}", tree.root_line);
    }
    let mut lines = vec![tree.root_line.clone()];
    let mut stack: Vec<(usize, usize)> = root
        .children
        .iter()
        .rev()
        .map(|child| (*child, 1))
        .collect();
    while let Some((index, depth)) = stack.pop() {
        let node = &tree.nodes[index];
        lines.push(line(node, depth));
        stack.extend(node.children.iter().rev().map(|child| (*child, depth + 1)));
    }
    let mut text = lines.join("\n");
    if tree.truncated {
        text.push('\n');
        text.push_str(CUT_MARK);
        text.push_str("\n\n");
        text.push_str(TOO_LARGE);
    }
    text
}

/// The line of `node` at `depth`: the indent, the bullet, the name, a `/`
/// for a directory, and `...` for a directory that was shown but never
/// expanded.
fn line(node: &Node, depth: usize) -> String {
    let slash = if node.directory { "/" } else { "" };
    let unexpanded = if node.directory && !node.expanded {
        "..."
    } else {
        ""
    };
    format!(
        "{}- {}{slash}{unexpanded}",
        " ".repeat(2 * depth),
        node.name
    )
}
