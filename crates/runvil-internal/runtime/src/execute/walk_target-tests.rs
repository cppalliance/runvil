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
    let child = resolve_jump_target("Self", &prompt, Origin::Section(&parent), None)
        .expect("a child resolves");
    assert_eq!(child.pos, at(&prompt, "/Grand/Parent/Self"));
    assert!(child.descends, "a child target starts a child-level walk");
    let sibling = resolve_jump_target("../Helpers", &prompt, Origin::Section(&parent), None)
        .expect("a sibling resolves");
    assert_eq!(sibling.pos, at(&prompt, "/Grand/Helpers"));
    assert!(!sibling.descends, "a sibling stays in the caller's slice");
}

#[test]
fn a_jump_from_the_h1_lands_on_a_top_level_section_and_never_descends() {
    let prompt = library_tree();
    let target =
        resolve_jump_target("Grand", &prompt, Origin::H1, None).expect("a section resolves");
    assert_eq!(target.pos, at(&prompt, "/Grand"));
    assert!(!target.descends, "the walk already runs at the top level");
}

/// The Markdown a read of `path` from `origin` finds, or the message of
/// its not-found error.
fn read(prompt: &Prompt, origin: Origin<'_>, path: &str) -> std::result::Result<String, String> {
    match read_section(path, prompt, origin, None).expect("the path is well-formed") {
        SectionRead::Found(text) => Ok(text.markdown().to_owned()),
        SectionRead::Missing(error) => Err(error.to_string()),
    }
}

#[test]
fn a_read_finds_the_named_sections_own_markdown_or_the_not_found_error() {
    let prompt = library_tree();
    let own = at(&prompt, "/Grand/Parent/Self");
    let origin = Origin::Section(&own);
    for path in ["../Sibling", "/Grand/Parent/Sibling", "/Top/Helpers"] {
        assert_eq!(read(&prompt, origin, path).as_deref(), Ok("x"), "{path}");
    }
    let lib = read(&prompt, origin, "/Lib").expect("the section is there");
    assert!(lib.starts_with("Intro."), "{lib}");
    for (path, message) in [
        (
            "Sibling",
            "section `Sibling` not found: `/Grand/Parent/Self` has no child `Sibling`",
        ),
        (
            "../Missing",
            "section `../Missing` not found: `/Grand/Parent/Self` has no sibling `Missing`",
        ),
        (
            "/Grand/Missing",
            "section `/Grand/Missing` not found: `/Grand` has no child `Missing`",
        ),
    ] {
        assert_eq!(
            read(&prompt, origin, path),
            Err(message.to_owned()),
            "{path}"
        );
    }
}

#[test]
fn a_path_in_a_librarys_code_starts_at_the_library_whatever_section_is_running() {
    let prompt = library_tree();
    let library = resolve_library(&prompt, "/Grand").expect("the library resolves");
    let from = Some(&library.key);
    for origin in [Origin::H1, Origin::Section(&at(&prompt, "/Top"))] {
        let child = resolve_jump_target("Parent", &prompt, origin, from).expect("a child");
        assert_eq!(child.pos, at(&prompt, "/Grand/Parent"));
        let sibling = resolve_jump_target("../Lib", &prompt, origin, from).expect("a sibling");
        assert_eq!(sibling.pos, at(&prompt, "/Lib"));
        let text = read_section("Helpers", &prompt, origin, from).expect("a well-formed path");
        assert!(matches!(text, SectionRead::Found(text) if text.markdown() == "x"));
        let miss = read_section("Niece", &prompt, origin, from).expect("a well-formed path");
        let SectionRead::Missing(error) = miss else {
            panic!("the library has no such child");
        };
        assert_eq!(
            error.to_string(),
            "section `Niece` not found: `/Grand` has no child `Niece`",
            "the miss names the library"
        );
    }
}

#[test]
fn a_key_names_its_sections_position_and_a_foreign_key_is_an_internal_error() {
    let prompt = library_tree();
    for path in ["/Top", "/Grand/Parent/Self", "/Grand/Parent/Sibling/Niece"] {
        let library = resolve_library(&prompt, path).expect("the library resolves");
        assert_eq!(
            library_position(&library.key).expect("the key parses"),
            at(&prompt, path),
            "{path}"
        );
    }
    let invariant = "internal invariant violated: a library key names no section position: ";
    for identity in ["", "x", "1.", "1..2"] {
        let error = library_position(&LibraryKey::new(identity.to_owned()))
            .expect_err("a foreign key must not parse");
        assert_eq!(
            error.code,
            runvil_types::error::ErrorCode::Internal,
            "{identity:?} gave {error}"
        );
        assert!(
            error.message.len() > invariant.len() && error.message.starts_with(invariant),
            "required the index parse failure after the invariant, actual {:?} for {identity:?}",
            error.message
        );
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
