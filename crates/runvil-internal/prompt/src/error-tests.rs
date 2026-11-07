//! Tests for the failures a parse returns: each code's message, location,
//! facts, and prose.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use runvil_lua::LuaProgram;
use runvil_types::emitter::{Emitter, EventSink};
use runvil_types::error::{Error, ErrorCode, ErrorFact, SourceLoc};

use super::{internal, lua, with_prompt_context};
use crate::tests::location;
use crate::{Frontmatter, Prompt};

/// The file label every source here is parsed from.
const FILE: &str = "agents/demo.md";

/// The failure parsing `src` from [`FILE`] returns.
#[track_caller]
fn failure_of(src: &str) -> Error {
    match Prompt::parse(src, FILE, "test").0 {
        Ok(prompt) => panic!("expected a failure, parsed `{}`", prompt.title()),
        Err(error) => error,
    }
}

/// The prompt `x` with the section `S` holding `content`, which starts on
/// file line 10.
fn in_section(content: &str) -> String {
    format!("---\nname: x\ndescription: d\n---\n\n# T\n\n## S\n\n{content}")
}

/// The prose of a failure with `code` and `message` located at `line`.
fn prose_at(code: ErrorCode, line: u32, message: &str) -> String {
    format!("{FILE}:{line}: {message}. {}", code.prose())
}

#[test]
fn a_yaml_failure_is_frontmatter_invalid_at_its_line_and_column() {
    let yaml = "name: x\ndescription: d\nuses:\n  - not a Pack id\n";
    let error = failure_of(&format!("---\n{yaml}---\n\n# T\n\n## S\n\np\n"));
    let Err(cause) = serde_yaml_ng::from_str::<Frontmatter>(yaml) else {
        panic!("the frontmatter must not decode");
    };
    let message = format!("invalid frontmatter: {cause}");
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
    assert_eq!(error.message, message);
    assert_eq!(location(&error), Some((FILE, 5, 5, 5, 5)));
    assert_eq!(error.facts, BTreeMap::new());
    assert_eq!(
        error.prose,
        prose_at(ErrorCode::FrontmatterInvalid, 5, &message)
    );
}

#[test]
fn a_failure_with_no_line_names_only_the_file_in_its_prose() {
    // A flow mapping declares `uses` on no line of its own, so the refusal
    // has no line to point at.
    let error = failure_of("---\n{ name: x, description: d, uses: [web, web] }\n---\n\n# T\n");
    assert_eq!(error.code, ErrorCode::FrontmatterInvalid);
    assert_eq!(
        error.message,
        "invalid frontmatter: Pack id web is declared more than once under uses"
    );
    assert_eq!(error.at, None);
    assert_eq!(
        error.prose,
        "agents/demo.md: invalid frontmatter: Pack id web is declared more than once under uses. \
         The frontmatter is invalid. Fix the YAML between the `---` lines so it matches the \
         frontmatter schema."
    );
}

#[test]
fn a_failure_that_knows_its_line_is_located_on_it() {
    let prologue = in_section("```lua\nreturn 1\n");
    let epilog = in_section("Prose.\n\n```lua\nreturn 1\n");
    let h1 = "---\nname: x\ndescription: d\n---\n\n# T\n\n```lua\nreturn 1\n";
    for (src, code, message, line) in [
        (
            "# T\n\n## S\n\np\n",
            ErrorCode::FrontmatterInvalid,
            "file must begin with a --- frontmatter delimiter",
            1,
        ),
        (
            "---\nname: x\ndescription: d\n\n# T\n",
            ErrorCode::FrontmatterInvalid,
            "frontmatter was not closed with ---",
            1,
        ),
        (
            "---\nname: x\ndescription: d\n---\n\n# T\n\n## S\n\np\n\n# U\n",
            ErrorCode::StructureInvalid,
            "prompt must contain exactly one H1 title, found 2 at lines 6, 12",
            12,
        ),
        (
            "---\nname: x\ndescription: d\n---\n\n#\n\n## S\n\np\n",
            ErrorCode::StructureInvalid,
            "prompt H1 title must not be empty",
            6,
        ),
        (
            prologue.as_str(),
            ErrorCode::FenceInvalid,
            "section `S` prologue `lua` fence is not closed",
            10,
        ),
        (
            epilog.as_str(),
            ErrorCode::FenceInvalid,
            "section `S` epilog `lua` fence is not closed",
            12,
        ),
        (
            h1,
            ErrorCode::FenceInvalid,
            "section `T` prologue `lua` fence is not closed",
            8,
        ),
    ] {
        let error = failure_of(src);
        assert_eq!((error.code, error.message.as_str()), (code, message));
        assert_eq!(
            location(&error),
            Some((FILE, line, 1, line + 1, 1)),
            "{message}"
        );
        assert_eq!(error.prose, prose_at(code, line, message));
    }
}

#[test]
fn a_refused_uses_list_names_its_reason_and_the_pack_id() {
    for (entries, reason, pack, message) in [
        (
            "[web, acme/web]",
            "pack_duplicate",
            "web",
            "invalid frontmatter: Pack id web is declared more than once under uses",
        ),
        (
            "[web, acme/adhoc]",
            "pack_reserved",
            "adhoc",
            "invalid frontmatter: Pack id adhoc is reserved by the Engine and is never declared \
             under uses",
        ),
    ] {
        let error = failure_of(&format!(
            "---\nname: x\ndescription: d\nuses: {entries}\n---\n\n# T\n\n## S\n\np\n"
        ));
        assert_eq!(error.code, ErrorCode::FrontmatterInvalid, "{entries}");
        assert_eq!(error.message, message);
        assert_eq!(
            location(&error),
            Some((FILE, 4, 1, 5, 1)),
            "the `uses:` line: {entries}"
        );
        assert_eq!(
            error.facts,
            BTreeMap::from([
                (ErrorFact::Reason, reason.into()),
                (ErrorFact::Name, pack.into())
            ]),
            "the Pack id is the name, not the prompt's: {entries}"
        );
        assert_eq!(
            error.prose,
            prose_at(ErrorCode::FrontmatterInvalid, 4, message)
        );
    }
}

#[test]
fn a_duplicate_sibling_is_structure_invalid_at_its_heading() {
    let error = failure_of(&in_section("a\n\n## S\n\nb\n"));
    let message = "duplicate sibling section name `S`: first declared at line 8, again at line \
                   12; sibling section names must be unique";
    assert_eq!(error.code, ErrorCode::StructureInvalid);
    assert_eq!(error.message, message);
    assert_eq!(location(&error), Some((FILE, 12, 1, 13, 1)));
    assert_eq!(error.facts, BTreeMap::from([(ErrorFact::Name, "x".into())]));
    assert_eq!(
        error.prose,
        prose_at(ErrorCode::StructureInvalid, 12, message)
    );
}

#[test]
fn a_fence_violation_names_the_rule_it_breaks() {
    let opener = "`lua` fence opening line must be exactly ```lua";
    let label = "unrecognized fence label `Lua`; the only Lua fence label is `lua`";
    for (content, rule, message, line, end_column) in [
        ("```Lua\nreturn 1\n```\n", "label", label, 10, 7),
        ("~~~lua\nreturn 1\n~~~\n", "opener", opener, 10, 7),
        (
            "```lua\nvar.a = 1\n  ```\n",
            "closer",
            "`lua` fence closing line must be exactly ```",
            12,
            6,
        ),
        (
            "Prose.\n\n    ```lua\n    return 1\n    ```\n",
            "lookalike",
            opener,
            12,
            11,
        ),
    ] {
        let error = failure_of(&in_section(content));
        assert_eq!(error.code, ErrorCode::FenceInvalid, "{rule}");
        assert_eq!(error.message, message, "{rule}");
        assert_eq!(
            location(&error),
            Some((FILE, line, 1, line, end_column)),
            "{rule}"
        );
        assert_eq!(
            error.facts,
            BTreeMap::from([
                (ErrorFact::Name, "x".into()),
                (ErrorFact::Reason, rule.into())
            ])
        );
        assert_eq!(
            error.prose,
            prose_at(ErrorCode::FenceInvalid, line, message)
        );
    }
}

#[test]
fn an_empty_list_item_is_list_empty_item_at_its_marker() {
    let error = failure_of(&in_section("- alpha\n  1.\n- beta\n"));
    let message = "empty bullet item in list section `S`";
    assert_eq!(error.code, ErrorCode::ListEmptyItem);
    assert_eq!(error.message, message);
    assert_eq!(
        location(&error),
        Some((FILE, 11, 3, 11, 5)),
        "the marker itself, past its indent"
    );
    assert_eq!(error.facts, BTreeMap::from([(ErrorFact::Name, "x".into())]));
    assert_eq!(error.prose, prose_at(ErrorCode::ListEmptyItem, 11, message));
}

#[test]
fn a_lua_compile_failure_is_located_at_the_line_its_diagnostic_names() {
    // The block's source starts on file line 11, and the compiler reports
    // its second line, file line 12.
    let source = "local a = 1\nlocal b =";
    let error = failure_of(&in_section(&format!("```lua\n{source}\n```\n")));
    let emitter = Emitter::root(EventSink::default(), "test", false);
    let start = NonZeroU32::new(11).expect("11 is non-zero");
    let Err(compiled) =
        LuaProgram::compile(source, FILE, "section `S` prologue", start, &emitter, "S")
    else {
        panic!("the block must not compile");
    };
    let message = compiled.to_string();
    assert!(
        message.contains("[string \"line 11\"]:2:"),
        "the diagnostic names the chunk line: {message}"
    );
    assert_eq!(error.code, ErrorCode::LuaCompile);
    assert_eq!(error.message, message);
    assert_eq!(location(&error), Some((FILE, 12, 1, 13, 1)));
    assert_eq!(error.facts, BTreeMap::from([(ErrorFact::Name, "x".into())]));
    assert_eq!(error.prose, prose_at(ErrorCode::LuaCompile, 12, &message));
}

#[test]
fn a_parser_invariant_failure_is_located_at_its_rust_call_site_with_the_default_prose() {
    let (fault, site) = (internal("parser: test"), std::panic::Location::caller());
    let error = with_prompt_context(fault, Some("x"), FILE);
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.message, "internal invariant violated: parser: test");
    assert_eq!(
        error.at,
        Some(SourceLoc::line(
            site.file(),
            NonZeroU32::new(site.line()).expect("a line is non-zero")
        )),
        "the call site's Rust file and line survive the prompt context"
    );
    assert_eq!(error.facts, BTreeMap::new());
    assert_eq!(error.prose, ErrorCode::Internal.prose());
}

#[test]
fn an_internal_failure_keeps_its_rust_location_in_the_prompt_context() {
    let fault = runvil_lua::internal_at("a test invariant broke", std::panic::Location::caller());
    assert!(fault.at.is_some(), "an internal failure is located in Rust");
    let error = with_prompt_context(fault.clone(), Some("x"), FILE);
    assert_eq!(
        error.at, fault.at,
        "the prompt's file never replaces the Rust file"
    );
    assert_eq!(error.facts, BTreeMap::new());
    assert_eq!(error.prose, ErrorCode::Internal.prose());
}

#[test]
fn a_lua_compiler_that_cannot_start_is_internal() {
    let message = mlua::Error::MemoryError("not enough memory".to_owned()).to_string();
    let code = ErrorCode::Internal;
    let error = lua(&Error::new(code, message.clone(), code.prose()));
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.message, message);
    assert_eq!(error.at, None);
}
