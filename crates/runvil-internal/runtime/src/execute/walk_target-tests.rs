//! Tests for walk-target resolution: where a relative path lands, what a
//! `require` builds from a section, and the key naming its position.

use runvil_lua::LuaProgram;
use runvil_prompt::test_support::{prose_block, synthetic_section};

use super::*;

/// `Grand` holds `Parent` and a `Helpers`, `Parent` holds `Sibling` and
/// `Self`, `Top` holds a second `Helpers`, and `Lib` is laid out as prose,
/// Lua, Lua, prose, Lua, then trailing prose.
const LIBRARY_TREE: &str = concat!(
    "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n# Title\n\n",
    "## Top\n\nx\n\n",
    "### Helpers\n\nx\n\n",
    "## Grand\n\nx\n\n",
    "### Parent\n\nx\n\n",
    "#### Sibling\n\nx\n\n",
    "##### Niece\n\nx\n\n",
    "#### Self\n\nx\n\n",
    "### Helpers\n\nx\n\n",
    "## Lib\n\nIntro.\n\n",
    "```lua\nfirst = 1\n```\n\n",
    "```lua\nsecond = 2\n```\n\n",
    "Later.\n\n",
    "```lua\nthird = 3\n```\n\n",
    "Trailing.\n",
);

fn library_tree() -> Prompt {
    Prompt::parse(&|_: &str| Ok(LIBRARY_TREE.to_owned()), "prompt").expect("the tree parses")
}

/// The position of the section `path` names from the top.
fn at(prompt: &Prompt, path: &str) -> SectionPos {
    section_path::resolve_from_top(path, prompt).expect("the path resolves")
}

fn program(source: &str) -> LuaProgram {
    LuaProgram::compile_internal(source, "library-test").expect("the block compiles")
}

fn sources(library: &Library) -> Vec<&str> {
    library
        .blocks
        .iter()
        .map(|block| block.program.source())
        .collect()
}

fn templates(library: &Library) -> Vec<&str> {
    library
        .blocks
        .iter()
        .map(|block| block.prose.as_str())
        .collect()
}

/// The prompt line each block's template starts on.
fn template_lines(library: &Library) -> Vec<Option<u32>> {
    library
        .blocks
        .iter()
        .map(|block| block.line.map(std::num::NonZeroU32::get))
        .collect()
}

#[test]
fn a_jump_to_a_child_descends_and_a_jump_to_a_sibling_does_not() {
    let prompt = library_tree();
    let parent = at(&prompt, "/Grand/Parent");
    let child =
        resolve_jump_target("Self", &prompt, Origin::Section(&parent)).expect("a child resolves");
    assert_eq!(child.pos, at(&prompt, "/Grand/Parent/Self"));
    assert!(child.descends, "a child target starts a child-level walk");
    let sibling = resolve_jump_target("../Helpers", &prompt, Origin::Section(&parent))
        .expect("a sibling resolves");
    assert_eq!(sibling.pos, at(&prompt, "/Grand/Helpers"));
    assert!(!sibling.descends, "a sibling stays in the caller's slice");
}

#[test]
fn a_jump_from_the_h1_lands_on_a_top_level_section_and_never_descends() {
    let prompt = library_tree();
    let target = resolve_jump_target("Grand", &prompt, Origin::H1).expect("a section resolves");
    assert_eq!(target.pos, at(&prompt, "/Grand"));
    assert!(!target.descends, "the walk already runs at the top level");
}

#[test]
fn a_read_sees_the_named_sections_own_markdown_or_nothing() {
    let prompt = library_tree();
    let own = at(&prompt, "/Grand/Parent/Self");
    let origin = Origin::Section(&own);
    for path in ["../Sibling", "/Grand/Parent/Sibling", "/Top/Helpers"] {
        let text = read_section(path, &prompt, origin)
            .expect("the path is well-formed")
            .expect("the section is there");
        assert_eq!(text.markdown, "x", "{path}");
    }
    let lib = read_section("/Lib", &prompt, origin)
        .expect("the path is well-formed")
        .expect("the section is there");
    assert!(lib.markdown.starts_with("Intro."), "{}", lib.markdown);
    for path in ["Sibling", "../Missing", "/Grand/Missing"] {
        let missing = read_section(path, &prompt, origin).expect("the path is well-formed");
        assert_eq!(missing, None, "{path}");
    }
}

#[test]
fn a_library_pairs_each_program_with_the_prose_right_before_it() {
    let section = synthetic_section(
        "Lib",
        2,
        vec![
            prose_block("Intro.".to_owned()),
            Block::Lua(program("first = 1")),
            Block::Lua(program("second = 2")),
            prose_block("Later.".to_owned()),
            Block::Lua(program("third = 3")),
        ],
    );
    let key = LibraryKey::new("2".to_owned());
    let library =
        build_library(&section, key.clone(), "/Lib".to_owned()).expect("the library builds");
    assert_eq!(library.key, key);
    assert_eq!(library.path, "/Lib");
    assert_eq!(sources(&library), ["first = 1", "second = 2", "third = 3"]);
    assert_eq!(templates(&library), ["Intro.", "", "Later."]);
    assert_eq!(
        template_lines(&library),
        [Some(1), None, Some(1)],
        "a block with no prose right before it has no template line"
    );
}

#[test]
fn resolve_library_builds_a_parsed_section_without_its_trailing_prose() {
    let prompt = library_tree();
    let library = resolve_library(&prompt, "/Lib").expect("/Lib resolves");
    assert_eq!(library.path, "/Lib");
    assert_eq!(sources(&library), ["first = 1", "second = 2", "third = 3"]);
    assert_eq!(templates(&library), ["Intro.", "", "Later."]);
    let lines = template_lines(&library);
    assert_eq!(
        (lines[0], lines[2]),
        (Some(43), Some(53)),
        "each template keeps the file line it starts on: {lines:?}"
    );
}

#[test]
fn a_library_is_reported_by_its_path_from_the_top_however_the_path_was_spaced() {
    let prompt = library_tree();
    let library = resolve_library(&prompt, " / Grand / Parent / Sibling ").expect("it resolves");
    assert_eq!(library.path, "/Grand/Parent/Sibling");
}

#[test]
fn two_sections_with_one_title_are_two_libraries_with_their_own_keys() {
    let prompt = library_tree();
    let near = resolve_library(&prompt, "/Grand/Helpers").expect("the nested section resolves");
    let far = resolve_library(&prompt, "/Top/Helpers").expect("the other one resolves");
    assert_ne!(near.key, far.key, "each position keys its own library");
    assert_ne!(near.path, far.path);
    let again = resolve_library(&prompt, "/Grand/Helpers").expect("it resolves again");
    assert_eq!(near.key, again.key, "one position has one key");
}

#[test]
fn require_reaches_every_section_and_takes_no_relative_path() {
    let prompt = library_tree();
    for path in [
        "/Top",
        "/Grand",
        "/Grand/Parent",
        "/Grand/Parent/Self",
        "/Grand/Parent/Sibling/Niece",
    ] {
        let library = resolve_library(&prompt, path).expect("every section can be required");
        assert_eq!(library.path, path);
    }
    for path in ["Lib", "../Lib"] {
        let error = resolve_library(&prompt, path).expect_err("a relative path is refused");
        assert_eq!(
            error.to_string(),
            format!(
                "section path \"{path}\" does not start with \"/\"; require takes a path from \
                 the top, such as \"/helpers\""
            )
        );
    }
    let error = resolve_library(&prompt, "/Grand/Missing").expect_err("a miss is an error");
    assert_eq!(
        error.to_string(),
        "section `/Grand/Missing` not found: `/Grand` has no child `Missing`"
    );
}
