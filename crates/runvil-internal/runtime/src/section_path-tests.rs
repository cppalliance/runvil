//! Tests of the section path lookup: which section each form names from
//! each place, and what a path that names none reports.

use super::{Lookup, Origin, find, path_from_top, resolve_from_top, resolve_neighbor};
use crate::prompt::Prompt;
use crate::section_pos::SectionPos;

const FRONT: &str = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n# Title\n\n";

/// `Main` with a child and a grandchild, a sibling, and a sibling that has
/// a child of its own.
const NEIGHBORS: &str = concat!(
    "## Main\n\nx\n\n",
    "### Kid\n\nx\n\n",
    "#### Grand\n\nx\n\n",
    "## Sibling\n\nx\n\n",
    "## Other\n\nx\n\n",
    "### Niece\n\nx\n",
);

/// A tree around `Self`, five levels deep: `Grand` holds `Uncle` and
/// `Parent`, and `Parent` holds `Sibling` and `Self`.
const TREE: &str = concat!(
    "## Top\n\nx\n\n",
    "## Grand\n\nx\n\n",
    "### Uncle\n\nx\n\n",
    "#### Cousin\n\nx\n\n",
    "### Parent\n\nx\n\n",
    "#### Sibling\n\nx\n\n",
    "##### Niece\n\nx\n\n",
    "#### Self\n\nx\n\n",
    "##### Child\n\nx\n\n",
    "###### Grandchild\n\nx\n\n",
    "## Last\n\nx\n",
);

fn prompt(sections: &str) -> Prompt {
    let source = format!("{FRONT}{sections}");
    Prompt::parse(&|_: &str| Ok(source.clone()), "prompt").expect("the prompt parses")
}

/// The position of the section `path` names from the top.
fn at(prompt: &Prompt, path: &str) -> SectionPos {
    resolve_from_top(path, prompt).unwrap_or_else(|error| panic!("{path} must resolve: {error}"))
}

/// What a lookup gave.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The section it named, as a path from the top.
    Found(String),
    /// The message of its miss or its refusal.
    Failed(String),
}

/// What a read of `path` from `origin` names, or the message of its miss.
fn read(prompt: &Prompt, origin: Origin<'_>, path: &str) -> Outcome {
    match find(path, prompt, origin).unwrap_or_else(|error| panic!("{path} is refused: {error}")) {
        Lookup::Found(pos) => Outcome::Found(path_from_top(prompt, &pos)),
        Lookup::Missing(error) => Outcome::Failed(error.to_string()),
    }
}

/// What running `path` from `origin` names, or the message of its error.
fn run(prompt: &Prompt, origin: Origin<'_>, path: &str) -> Outcome {
    match resolve_neighbor(path, prompt, origin) {
        Ok(pos) => Outcome::Found(path_from_top(prompt, &pos)),
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

fn found(path: &str) -> Outcome {
    Outcome::Found(path.to_owned())
}

fn failed(message: &str) -> Outcome {
    Outcome::Failed(message.to_owned())
}

#[test]
fn a_name_is_a_direct_child_and_a_dotted_name_is_a_sibling() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    let origin = Origin::Section(&main);
    assert_eq!(run(&prompt, origin, "Kid"), found("/Main/Kid"));
    assert_eq!(run(&prompt, origin, "../Sibling"), found("/Sibling"));
    assert_eq!(run(&prompt, origin, "../Other"), found("/Other"));
    let kid = at(&prompt, "/Main/Kid");
    assert_eq!(
        run(&prompt, Origin::Section(&kid), "Grand"),
        found("/Main/Kid/Grand")
    );
    let niece = at(&prompt, "/Other/Niece");
    assert_eq!(
        run(&prompt, Origin::Section(&niece), "../Niece"),
        failed("section `../Niece` not found: `/Other/Niece` has no sibling `Niece`")
    );
}

#[test]
fn a_miss_says_which_section_was_searched_and_what_was_wanted_there() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    let origin = Origin::Section(&main);
    for (path, message) in [
        // A grandchild and a sibling's child are not neighbors.
        (
            "Grand",
            "section `Grand` not found: `/Main` has no child `Grand`",
        ),
        (
            "Niece",
            "section `Niece` not found: `/Main` has no child `Niece`",
        ),
        // A sibling needs `../`, and a child takes none.
        (
            "Sibling",
            "section `Sibling` not found: `/Main` has no child `Sibling`",
        ),
        (
            "../Kid",
            "section `../Kid` not found: `/Main` has no sibling `Kid`",
        ),
        // A section is not its own sibling.
        (
            "../Main",
            "section `../Main` not found: `/Main` has no sibling `Main`",
        ),
    ] {
        assert_eq!(run(&prompt, origin, path), failed(message));
        assert_eq!(read(&prompt, origin, path), failed(message));
    }
}

#[test]
fn the_h1s_children_are_the_top_level_sections_and_it_has_no_siblings() {
    let prompt = prompt(NEIGHBORS);
    for name in ["Main", "Sibling", "Other"] {
        assert_eq!(run(&prompt, Origin::H1, name), found(&format!("/{name}")));
    }
    assert_eq!(
        run(&prompt, Origin::H1, "Kid"),
        failed("section `Kid` not found: the top level has no section `Kid`")
    );
    assert_eq!(
        run(&prompt, Origin::H1, "../Main"),
        failed("section `../Main` not found: the H1 has no siblings")
    );
}

#[test]
fn a_relative_path_is_one_name_or_two_dots_and_a_name() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    let more = |path: &str| {
        format!(
            "section path \"{path}\" has more than one name; a relative path is a child's \
             name, or \"../\" and a sibling's name"
        )
    };
    for origin in [Origin::Section(&main), Origin::H1] {
        for path in [
            "Kid/Grand",
            "../Other/Niece",
            "../../Main",
            "../..",
            "Kid/..",
        ] {
            assert_eq!(run(&prompt, origin, path), failed(&more(path)));
            assert_eq!(
                find(path, &prompt, origin).unwrap_err().to_string(),
                more(path)
            );
        }
        assert_eq!(
            run(&prompt, origin, ".."),
            failed("section path \"..\" needs a name after \"..\"")
        );
    }
}

#[test]
fn a_path_from_the_top_names_any_section_and_the_same_one_from_everywhere() {
    let prompt = prompt(TREE);
    let own = at(&prompt, "/Grand/Parent/Self");
    let last = at(&prompt, "/Last");
    for path in [
        // Top-level sections.
        "/Top",
        "/Last",
        // A sibling of the parent, and its child.
        "/Grand/Uncle",
        "/Grand/Uncle/Cousin",
        // A sibling's child, and a grandchild.
        "/Grand/Parent/Sibling/Niece",
        "/Grand/Parent/Self/Child/Grandchild",
        // Ancestors, and the section itself.
        "/Grand/Parent",
        "/Grand",
        "/Grand/Parent/Self",
        // A sibling and a child, which the relative forms name too.
        "/Grand/Parent/Sibling",
        "/Grand/Parent/Self/Child",
    ] {
        for origin in [Origin::Section(&own), Origin::Section(&last), Origin::H1] {
            assert_eq!(read(&prompt, origin, path), found(path));
        }
        assert_eq!(path_from_top(&prompt, &at(&prompt, path)), path);
    }
    assert_eq!(
        read(&prompt, Origin::Section(&own), "../Sibling"),
        found("/Grand/Parent/Sibling")
    );
    assert_eq!(
        read(&prompt, Origin::Section(&own), "Child"),
        found("/Grand/Parent/Self/Child")
    );
}

#[test]
fn a_path_from_the_top_reports_the_step_where_it_stopped() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    for (path, message) in [
        (
            "/Other/Neice",
            "section `/Other/Neice` not found: `/Other` has no child `Neice`",
        ),
        (
            "/Othr/Niece",
            "section `/Othr/Niece` not found: the top level has no section `Othr`",
        ),
        (
            "/Main/Kid/Grand/Deeper",
            "section `/Main/Kid/Grand/Deeper` not found: `/Main/Kid/Grand` has no child `Deeper`",
        ),
        // The H1's title is not a step.
        (
            "/Title/Main",
            "section `/Title/Main` not found: the top level has no section `Title`",
        ),
    ] {
        for origin in [Origin::Section(&main), Origin::H1] {
            assert_eq!(read(&prompt, origin, path), failed(message));
        }
        assert_eq!(
            resolve_from_top(path, &prompt).unwrap_err().to_string(),
            message
        );
    }
}

#[test]
fn the_functions_that_run_a_section_take_no_path_from_the_top() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    for origin in [Origin::Section(&main), Origin::H1] {
        for path in ["/Sibling", "/Main/Kid", "/Missing"] {
            assert_eq!(
                run(&prompt, origin, path),
                failed(&format!(
                    "section path \"{path}\" starts with \"/\"; only require, \
                     prompt.section, and prompt.section_or take a path from the top"
                ))
            );
        }
    }
}

#[test]
fn require_takes_only_a_path_from_the_top() {
    let prompt = prompt(NEIGHBORS);
    assert_eq!(
        path_from_top(&prompt, &at(&prompt, " / Main / Kid ")),
        "/Main/Kid"
    );
    for path in ["Sibling", "../Sibling", "Kid/Grand", "## Sibling", ".."] {
        assert_eq!(
            resolve_from_top(path, &prompt).unwrap_err().to_string(),
            format!(
                "section path \"{path}\" does not start with \"/\"; require takes a path from \
                 the top, such as \"/helpers\""
            )
        );
    }
    assert_eq!(
        resolve_from_top("/Main/../Sibling", &prompt)
            .unwrap_err()
            .to_string(),
        "section path \"/Main/../Sibling\" uses \"..\"; a path from the top takes none"
    );
}

#[test]
fn a_malformed_path_is_refused_whatever_takes_it() {
    let prompt = prompt(NEIGHBORS);
    for (path, message) in [
        ("", "section path \"\" has an empty step"),
        ("Kid/", "section path \"Kid/\" has an empty step"),
        ("../", "section path \"../\" has an empty step"),
        (
            "/Main//Grand",
            "section path \"/Main//Grand\" has an empty step",
        ),
        ("//Grand", "section path \"//Grand\" has an empty step"),
        (
            "Kid[1]",
            "section path \"Kid[1]\" has an index; a path in a prompt takes none",
        ),
        (
            "/Main/Kid[-1]",
            "section path \"/Main/Kid[-1]\" has an index; a path in a prompt takes none",
        ),
        (
            r"Kid\n",
            r#"section path "Kid\n" has an unknown escape "\n"; the escapes are "\/", "\\", and "\[""#,
        ),
    ] {
        assert_eq!(run(&prompt, Origin::H1, path), failed(message));
        assert_eq!(
            find(path, &prompt, Origin::H1).unwrap_err().to_string(),
            message
        );
        assert_eq!(
            resolve_from_top(path, &prompt).unwrap_err().to_string(),
            message
        );
    }
}

#[test]
fn a_title_matches_exactly_and_whitespace_around_each_step_is_ignored() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    let origin = Origin::Section(&main);
    assert_eq!(run(&prompt, origin, "  Kid\t"), found("/Main/Kid"));
    assert_eq!(run(&prompt, origin, " .. / Sibling "), found("/Sibling"));
    assert_eq!(
        read(&prompt, origin, " / Other / Niece "),
        found("/Other/Niece")
    );
    // The error shows the path without the whitespace around it.
    assert_eq!(
        run(&prompt, origin, "  kid "),
        failed("section `kid` not found: `/Main` has no child `kid`")
    );
    assert_eq!(
        run(&prompt, origin, "K id"),
        failed("section `K id` not found: `/Main` has no child `K id`")
    );
}

#[test]
fn a_name_that_starts_with_hash_marks_is_a_name_like_any_other() {
    let prompt = prompt(NEIGHBORS);
    let main = at(&prompt, "/Main");
    let origin = Origin::Section(&main);
    let message = "section `### Kid` not found: `/Main` has no child `### Kid`";
    assert_eq!(run(&prompt, origin, "### Kid"), failed(message));
    assert_eq!(read(&prompt, origin, "### Kid"), failed(message));
}

#[test]
fn a_slash_and_a_trailing_bracketed_number_in_a_title_are_escaped() {
    let prompt = prompt(concat!(
        "## Input/Output\n\nx\n\n",
        "### Appendix [2]\n\nx\n\n",
        "## C:\\\\Users\n\nx\n",
    ));
    assert_eq!(
        run(&prompt, Origin::H1, r"Input\/Output"),
        found(r"/Input\/Output")
    );
    assert_eq!(
        read(&prompt, Origin::H1, r"/Input\/Output/Appendix \[2]"),
        found(r"/Input\/Output/Appendix \[2]")
    );
    assert_eq!(
        read(&prompt, Origin::H1, r"C:\\Users"),
        found(r"/C:\\Users")
    );
    // Unescaped, the slash splits the title in two.
    assert_eq!(
        read(&prompt, Origin::H1, "/Input/Output"),
        failed("section `/Input/Output` not found: the top level has no section `Input`")
    );
    // The searched section is printed as a path that names it.
    let io = at(&prompt, r"/Input\/Output");
    assert_eq!(
        run(&prompt, Origin::Section(&io), "Missing"),
        failed(r"section `Missing` not found: `/Input\/Output` has no child `Missing`")
    );
}
