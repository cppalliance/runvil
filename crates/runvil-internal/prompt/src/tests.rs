//! Crate-wide parser tests: frontmatter, headings, fences, lists, breaks, and line mapping.

use runvil_lua::LuaProgram;
use runvil_types::error::{Error, ErrorCode, FactKey};
use runvil_types::event::Event;

use super::*;
use crate::test_support::{
    PromptSource, assert_parse_error, one_section, parse_err, parse_ok, prompt,
};

mod breaks;
mod frontmatter;
mod h1_fences;
mod headings;
mod line_mapping;
mod lists;
mod section_fences;

/// `lua` fences whose opening line breaks the Opener rule, each closed by a
/// line that matches its opener.
const NON_EXACT_LUA_OPENERS: [&str; 9] = [
    "```lua \nreturn 1\n```",
    "```lua\t\nreturn 1\n```",
    "``` lua\nreturn 1\n```",
    " ```lua\nreturn 1\n ```",
    "   ```lua\nreturn 1\n   ```",
    "~~~lua\nreturn 1\n~~~",
    "````lua\nreturn 1\n````",
    "> ```lua\n> return 1\n> ```",
    "- ```lua\n  return 1\n  ```",
];

const OPENER_MESSAGE: &str = "`lua` fence opening line must be exactly ```lua";
const CLOSER_MESSAGE: &str = "`lua` fence closing line must be exactly ```";

/// The failure's location as its file, start line, start column, end
/// line, and end column, when it has one.
pub(crate) fn location(error: &Error) -> Option<(&str, u32, u32, u32, u32)> {
    error.at.as_ref().map(|at| {
        (
            at.file.as_str(),
            at.start_line.get(),
            at.start_column.get(),
            at.end_line.get(),
            at.end_column.get(),
        )
    })
}

#[test]
fn frontmatter_syntax_errors_report_a_position_without_a_name() {
    // A malformed-YAML frontmatter is located at the serde_yaml_ng position
    // (1-based, file-absolute), a point at the line and column; the failure
    // predates the prompt's name, so none is reported.
    let error = parse_err("---\nname: p\ndescription: d\n: : :\n---\n\n# T\n\n## S\n\nhi\n");
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
    let Some((file, line, column, end_line, end_column)) = location(&error) else {
        panic!("the malformed line is located: {error:?}");
    };
    assert_eq!((file, line), ("prompt", 4), "the malformed line: {error}");
    assert_eq!((end_line, end_column), (line, column), "{error:?}");
    assert_eq!(error.facts.get(&FactKey::Name), None);
}

#[test]
fn structured_errors_report_the_prompt_name_and_source_position() {
    // A structured failure postdates the frontmatter, so the parse
    // error reports the prompt's frontmatter name plus the offending
    // span's 1-based start and end lines and columns, computed against the
    // source.
    let src = concat!(
        "---\nname: dup\ndescription: d\n---\n", // lines 1-4
        "\n# T\n\n## S\n\np\n\n## S\n\nq\n",     // the second `## S` heads line 12
    );
    let error = parse_err(src);
    assert_eq!(error.code, ErrorCode::StructureInvalid);
    assert_eq!(error.facts.get(&FactKey::Name), Some(&"dup".into()));
    assert_eq!(
        location(&error),
        Some(("prompt", 12, 1, 13, 1)),
        "the duplicate heading's line, through its newline: {error}"
    );
}

#[test]
fn orphan_and_empty_heading_errors_report_a_line() {
    // The frontmatter takes lines 1-4, `# T` is line 6, and the body starts
    // on line 8. Each span is the heading line through its newline.
    for (body, line) in [("## A\n\na\n\n#### D\n\nd\n", 12), ("## \n\na\n", 8)] {
        let error = parse_err(&format!(
            "---\nname: x\ndescription: d\n---\n\n# T\n\n{body}"
        ));
        assert_eq!(error.code, ErrorCode::StructureInvalid, "{error}");
        assert_eq!(
            location(&error),
            Some(("prompt", line, 1, line + 1, 1)),
            "{error}"
        );
    }
}

#[test]
fn parsed_prompt_value_types_are_equatable() {
    // Parsing the same source twice yields equal values, and a differing
    // source yields unequal values, across the finalized parser value types
    // (`Prompt`, `Frontmatter`, `Section`, `Block`).
    let source = |heading: &str| PromptSource::new().prose(heading, "do a thing").build();
    let a = parse_ok(&source("One"));
    let b = parse_ok(&source("One"));
    assert_eq!(a, b, "identical sources must parse equal");
    assert_eq!(a.frontmatter, b.frontmatter);
    assert_eq!(a.sections, b.sections);

    let c = parse_ok(&source("Two"));
    assert_ne!(a, c, "differing section headings must parse unequal");
}

/// The parse-time events read back as `(execution, section, kind)`.
struct Recorder(Vec<Event>);

/// The `kind` labels the parse reports.
mod detail {
    pub(super) const PARSE_STARTED: &str = "parse_started";
    pub(super) const PARSE_SUCCEEDED: &str = "parse_succeeded";
    pub(super) const PARSE_FAILED: &str = "parse_failed";
    pub(super) const LUA_COMPILATION_STARTED: &str = "lua_compilation_started";
    pub(super) const LUA_COMPILATION_SUCCEEDED: &str = "lua_compilation_succeeded";
    pub(super) const LUA_COMPILATION_FAILED: &str = "lua_compilation_failed";
}

/// The `kind` label of one of the events a parse reports.
fn kind(event: &Event) -> String {
    match event {
        Event::ParseStarted { .. } => detail::PARSE_STARTED,
        Event::ParseSucceeded { .. } => detail::PARSE_SUCCEEDED,
        Event::ParseFailed { .. } => detail::PARSE_FAILED,
        Event::LuaCompilationStarted { .. } => detail::LUA_COMPILATION_STARTED,
        Event::LuaCompilationSucceeded { .. } => detail::LUA_COMPILATION_SUCCEEDED,
        Event::LuaCompilationFailed { .. } => detail::LUA_COMPILATION_FAILED,
        other => panic!("a parse reports no {other:?}"),
    }
    .to_owned()
}

impl Recorder {
    fn records(&self) -> Vec<(String, String, String)> {
        self.0
            .iter()
            .map(|event| {
                (
                    event.execution().to_owned(),
                    event.section().to_owned(),
                    kind(event),
                )
            })
            .collect()
    }

    fn observations(&self) -> Vec<(String, String)> {
        self.0
            .iter()
            .map(|event| (event.section().to_owned(), kind(event)))
            .collect()
    }
}

#[test]
fn parses_multi_section_with_all_features() {
    let src = PromptSource::new()
        .name("demo")
        .title("Demo Title")
        .h1_prose("Human-readable intro text.")
        .prose(
            "First",
            "```lua\nlocal x = 1\n```\n\nProse for the first section.",
        )
        .prose("### Child", "Child prose.")
        .prose("Second", "Prose for the second section.")
        .build();

    let p = parse_ok(&src);
    assert_eq!(p.frontmatter.name(), "demo");
    assert_eq!(p.frontmatter.description(), "d");
    assert_eq!(p.title, "Demo Title");
    assert_eq!(
        p.h1_blocks,
        vec![Block::Prose {
            text: BodyText::from_owned("Human-readable intro text.".to_owned()),
        }]
    );

    assert_eq!(p.sections.len(), 2);
    let first = &p.sections[0];
    assert_eq!(first.name, "First");
    assert_eq!(first.level, 2);
    assert_eq!(
        first.prologue().map(LuaProgram::source),
        Some("local x = 1")
    );
    assert_eq!(first.prose(), "Prose for the first section.");
    assert!(first.epilog().is_none());
    assert_eq!(first.children.len(), 1);
    assert_eq!(first.children[0].name, "Child");
    assert_eq!(first.children[0].level, 3);
    assert_eq!(first.children[0].prose(), "Child prose.");

    assert_eq!(p.sections[1].name, "Second");
    assert!(p.sections[1].prologue().is_none());
    assert!(p.sections[1].epilog().is_none());
}

#[test]
fn parses_single_minimal_section() {
    let p = parse_ok(&PromptSource::new().prose("Greet", "Say hi").build());
    assert_eq!(p.sections.len(), 1);
    assert_eq!(p.sections[0].name, "Greet");
    assert_eq!(p.sections[0].prose(), "Say hi");
}

#[test]
fn parse_records_the_file_the_source_came_from() {
    let src = PromptSource::new().prose("Greet", "Say hi").build();
    let prompt = Prompt::parse(&src, "agents/research.md", "test")
        .0
        .expect("the minimal prompt parses");
    assert_eq!(prompt.file(), "agents/research.md");
}
