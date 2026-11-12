//! What raises: a cycle, a path that names no section or does not start
//! at the top, a `jump` during a load, and a Pack prelude that defines or
//! calls `require`.
//! A library that raises while loading runs again on the next `require`,
//! and a raise in library code reports the library's location and
//! prompt-file line.

use runvil_types::error::ErrorCode;
use runvil_types::packs::Prelude;

use crate::execute::tests::preludes::prelude;
use crate::test_support::scenario::Trace;

use super::*;

/// Runs `source` with `preludes` installed in every section VM.
#[track_caller]
fn run_with_preludes(source: &str, preludes: Vec<Prelude>) -> Trace {
    require_case(source.to_owned())
        .params(|ctx| ctx.with_preludes(preludes))
        .run()
}

/// The text of a run that fails with a Lua runtime error.
#[track_caller]
fn lua_failure(trace: &Trace) -> String {
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::LuaRuntime, "{error}");
    error.to_string()
}

#[test]
fn a_cycle_raises_naming_its_sections_in_order() {
    let source = prompt(
        "",
        &[
            ("Main", "require '/a'"),
            ("a", "require '/b'"),
            ("b", "require '/a'"),
        ],
    );
    let message = lua_failure(&require_case(source).run());
    assert!(
        message.contains("require cycle: /a -> /b -> /a"),
        "the cycle names both libraries: {message}"
    );
}

#[test]
fn a_section_that_requires_itself_meets_the_cycle_error() {
    let source = prompt("", &[("Main", "require '/Main'")]);
    let message = lua_failure(&require_case(source).run());
    assert!(
        message.contains("require cycle: /Main -> /Main"),
        "the load of Main requires Main again: {message}"
    );
}

#[test]
fn a_path_that_names_no_section_or_is_not_from_the_top_raises_when_the_call_runs() {
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local messages = {}
            for _, path in ipairs({ '/helpres', '/other/deeper', 'helpers', '../helpers', '## helpers' }) do
              local ok, err = pcall(require, path)
              assert(not ok, path .. ' must raise')
              messages[#messages + 1] = tostring(err)
            end
            return table.concat(messages, '\\n')",
        )
        .prose("other", "")
        .section("### deep", "return 'deep'")
        .section("helpers", "return 'helpers'");
    let trace = require_case(source.build()).run();
    let text = trace.text();
    let messages: Vec<&str> = text.lines().collect();
    let [typo, deeper, child, sibling, marks] = messages.as_slice() else {
        panic!("five messages: {messages:?}");
    };
    assert!(
        typo.contains("section `/helpres` not found: the top level has no section `helpres`"),
        "a misspelled title names where the walk stopped: {typo}"
    );
    assert!(
        deeper.contains("section `/other/deeper` not found: `/other` has no child `deeper`"),
        "a miss below the top names the section that was searched: {deeper}"
    );
    for (path, message) in [
        ("helpers", child),
        ("../helpers", sibling),
        ("## helpers", marks),
    ] {
        assert!(
            message.contains(&format!(
                "section path \"{path}\" does not start with \"/\"; require takes a path \
                 from the top, such as \"/helpers\""
            )),
            "a relative path is refused: {message}"
        );
    }
}

#[test]
fn a_jump_during_a_load_raises() {
    let source = prompt(
        "",
        &[
            ("Main", "require '/jumper'"),
            ("jumper", "jump('../Last')"),
            ("Last", "return 'jumped'"),
        ],
    );
    let message = lua_failure(&require_case(source).run());
    assert!(
        message.contains("jump is unavailable while a library loads"),
        "the load refuses the jump: {message}"
    );
}

#[test]
fn a_library_that_raises_while_loading_runs_again_on_the_next_require() {
    let source = prompt(
        "",
        &[
            (
                "Main",
                "
                var.attempts = 0
                local first_ok = pcall(require, '/flaky')
                local second = require '/flaky'
                return tostring(first_ok) .. '|' .. second .. '|' .. var.attempts",
            ),
            (
                "flaky",
                "
                var.attempts = var.attempts + 1
                if var.attempts == 1 then error('the first load fails') end
                return 'loaded'",
            ),
        ],
    );
    let trace = require_case(source).run();
    assert_eq!(trace.text(), "false|loaded|2", "the second load succeeds");
}

#[test]
fn a_prelude_cannot_define_require_or_call_it_while_it_installs() {
    let source = prompt(
        "",
        &[("Main", "return 'unreachable'"), ("lib", "return 'lib'")],
    );
    let defines = lua_failure(&run_with_preludes(
        &source,
        vec![prelude("shadow", "function require() return 'mine' end")],
    ));
    assert!(
        defines.contains("Pack `shadow`: its prelude defines the global `require`")
            && defines.contains("which is reserved as an Engine global"),
        "require is reserved: {defines}"
    );
    let calls = lua_failure(&run_with_preludes(
        &source,
        vec![prelude("eager", "require '/lib'")],
    ));
    assert!(
        calls.contains("Pack `eager`: its prelude failed to load")
            && calls.contains("attempt to call a nil value (global 'require')"),
        "the prelude phase has no require: {calls}"
    );
}

#[test]
fn a_raise_during_the_load_reports_the_library_line() {
    let source = "\
        ---\n\
        name: t\n\
        description: d\n\
        runvil: 0\n\
        ---\n\
        \n\
        # Require\n\
        \n\
        ## Main\n\
        \n\
        ```lua\n\
        require '/lib'\n\
        ```\n\
        \n\
        ## lib\n\
        \n\
        ```lua\n\
        local ready = true\n\
        error('load fault')\n\
        ```\n";
    let message = lua_failure(&require_case(source.to_owned()).run());
    assert!(
        message.contains("section `lib` prologue:19: load fault"),
        "the raise names the library block and its prompt line 19: {message}"
    );
}

#[test]
fn a_raise_while_a_long_titled_library_loads_reports_the_library_line() {
    let source = "\
        ---\n\
        name: t\n\
        description: d\n\
        runvil: 0\n\
        ---\n\
        \n\
        # Require\n\
        \n\
        ## Main\n\
        \n\
        ```lua\n\
        require '/Shared Research Helper Functions'\n\
        ```\n\
        \n\
        ## Shared Research Helper Functions\n\
        \n\
        ```lua\n\
        local ready = true\n\
        error('load fault')\n\
        ```\n";
    let message = lua_failure(&require_case(source.to_owned()).run());
    assert!(
        message.contains("section `Shared Research Helper Functions` prologue:19: load fault"),
        "the raise names the library block and its prompt line 19: {message}"
    );
}

#[test]
fn a_raise_from_a_library_function_called_later_reports_the_library_line() {
    let source = "\
        ---\n\
        name: t\n\
        description: d\n\
        runvil: 0\n\
        ---\n\
        \n\
        # Require\n\
        \n\
        ## Main\n\
        \n\
        ```lua\n\
        require '/lib'\n\
        boom()\n\
        ```\n\
        \n\
        ## lib\n\
        \n\
        ```lua\n\
        function boom()\n\
        error('later fault')\n\
        end\n\
        ```\n";
    let message = lua_failure(&require_case(source.to_owned()).run());
    assert!(
        message.contains("section `lib` prologue:20: later fault"),
        "the raise names the library block and its prompt line 20: {message}"
    );
    assert!(
        message.contains("section `Main` prologue:13:"),
        "the caller's frame maps to its own prompt line 13: {message}"
    );
}
