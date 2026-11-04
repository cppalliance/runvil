//! The globals a section reads: the `tasks` namespace, `item`, `var` across
//! the walk, `call`, and fanout arms and the line its failed read-back is
//! placed on, bare globals in prose, and the `sys` metadata.

use super::*;
use crate::RunErrorKind;
use crate::execute::run_error_kind;
use crate::test_support::at_line;

/// The `tasks` global is the task namespace (`tasks.spawn` and the
/// waits), not a control-flow table: indexing it by a heading string reads
/// nil, and control flow takes heading strings only.
#[test]
fn tasks_global_is_the_task_namespace() {
    let source = prompt(
        "",
        &[(
            "Main",
            "
            assert(type(tasks) == 'table', 'the tasks namespace is installed')
            assert(type(tasks.spawn) == 'function', 'tasks.spawn is a namespace function')
            assert(tasks['## Main'] == nil, 'the namespace is not a heading table')
            return 'ok'",
        )],
    );
    assert_eq!(Scenario::new(source).run().text(), "ok");
}

/// A walked section never sees the fanout `item` global: the seed split
/// installs `item` only for an arm, so a regression seeding it on the walk
/// path must fail here.
#[test]
fn item_global_is_absent_in_a_walked_section() {
    let source = one_section(
        "
        assert(item == nil, 'a walked section must not see the fanout item global')
        return 'ok'",
    );
    assert_eq!(Scenario::new(source).run().text(), "ok");
}

/// `{{ item }}` in walked (non-arm) prose is a substitution error at the
/// read site: the walk pins `item: None`, so only a spawned task's first
/// entry (a fanout arm's included) may reference it.
#[test]
fn item_in_walked_prose_is_a_substitution_error() {
    let source = PromptSource::new().prose(
        "Only",
        "
        Ask about {{ item }}.

        ```lua
        return prose
        ```",
    );
    let trace = Scenario::new(source.build()).run();
    let rendered = trace.error().to_string();
    assert!(
        rendered.contains("{{ item }} is nil"),
        "the walked section's prose must reject {{ item }}: {rendered}"
    );
}

/// Each fanout arm seeds `var` from a fresh clone of the caller's `var` (the
/// walk's H1-seeded value), so an arm reads the caller's entries but sibling
/// and caller writes never cross arm boundaries.
#[test]
fn fanout_arms_get_fresh_var_clones() {
    let source = PromptSource::new()
        .h1("var.from_h1 = 'seeded'")
        .section(
            "Parent",
            "
            local r = tasks.fanout('### Worker', {'a', 'b'})
            assert(var.a == nil and var.b == nil, 'arm writes must not reach the caller')
            return r[1].text .. r[2].text",
        )
        .section(
            "### Worker",
            "
            assert(var.from_h1 == 'seeded', 'an arm var clones the caller var in')
            local sibling = item == 'a' and 'b' or 'a'
            assert(var[sibling] == nil, 'each arm gets a fresh clone')
            var[item] = true
            return item",
        );
    assert_eq!(Scenario::new(source.build()).run().text(), "ab");
}

/// The walk's `var` table persists: H1's writes seed the top-level walk, and
/// one section's writes reach the next across both fall-through and a jump.
#[test]
fn var_persists_across_sections_fallthrough_and_jump() {
    let source = PromptSource::new()
        .h1("var.from_h1 = 'seed'")
        .section(
            "A",
            "
            assert(var.from_h1 == 'seed', 'H1 var writes must reach the first H2 section')
            var.from_a = 'a'
            jump('## C')",
        )
        .section("B", "error('the jump must skip B')")
        .section(
            "C",
            "
            assert(var.from_h1 == 'seed', 'the jump shares the walk var')
            assert(var.from_a == 'a', 'the jump keeps the jumper writes')
            var.from_c = 'c'",
        )
        .section(
            "D",
            "
            assert(var.from_c == 'c', 'fall-through after the jumped target keeps var')
            return var.from_h1 .. var.from_a .. var.from_c",
        );
    assert_eq!(Scenario::new(source.build()).run().text(), "seedac");
}

/// `call` clones the caller's `var` in: the contained chain reads the
/// clone, and its writes are discarded when the chain ends.
#[test]
fn call_clones_var_in_and_discards_child_writes() {
    let source = prompt(
        "",
        &[
            (
                "Main",
                "
                var.shared = 'caller'
                local r = call('## Sub')
                assert(r == 'sub saw caller', 'the child reads the cloned var')
                assert(var.child_write == nil, 'child writes must not reach the caller')
                return 'ok'",
            ),
            (
                "Sub",
                "
                var.child_write = 'sub'
                return 'sub saw ' .. var.shared",
            ),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "ok");
}

/// The `var` write guard turns a non-JSON assignment into a run failure at
/// the assigning line (the lua module tests pin the guard's messages).
#[test]
fn var_write_of_a_function_fails_the_run() {
    let trace = Scenario::new(one_section("var.f = function() end")).run();
    let error = trace.error();
    assert_eq!(
        run_error_kind(error.code),
        RunErrorKind::Lua,
        "the guard raises in Lua: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("var.f must be JSON data, got function"),
        "the guard error must name the field and the type: {error}"
    );
}

/// The `var` read-back at a section's fall-through fails on the section's
/// last Lua block, the walk having moved past it and its trailing prose.
#[test]
fn a_var_read_back_failure_at_fall_through_is_placed_on_the_last_lua_block() {
    let source = PromptSource::new().prose(
        "Only",
        "
        ```lua
        var.first = true
        ```

        ```lua
        local later = true
        var = 5
        ```

        Trailing commentary.",
    );
    let error = Scenario::new(source.build()).run().error().clone();
    assert!(
        error.to_string().contains("global was reassigned"),
        "{error}"
    );
    assert_eq!(error.at, at_line(16), "{error:?}");
}

/// The `var` read-back at a jump fails on the jumping block, not on the
/// section's last one.
#[test]
fn a_var_read_back_failure_at_a_jump_is_placed_on_the_jumping_block() {
    let source = PromptSource::new()
        .prose(
            "A",
            "
            ```lua
            local leaving = true
            var = 5
            jump('## B')
            ```

            ```lua
            error('the jump leaves this block unrun')
            ```",
        )
        .section("B", "return 'unreached'");
    let error = Scenario::new(source.build()).run().error().clone();
    assert!(
        error.to_string().contains("global was reassigned"),
        "{error}"
    );
    assert_eq!(error.at, at_line(12), "{error:?}");
}

/// The `var` read-back at a scalar return fails on the returning block,
/// not on the section's last one.
#[test]
fn a_var_read_back_failure_at_a_return_is_placed_on_the_returning_block() {
    let source = PromptSource::new().prose(
        "Only",
        "
        ```lua
        var.first = true
        ```

        ```lua
        local done = true
        var = 5
        return 'early'
        ```

        ```lua
        error('the return leaves this block unrun')
        ```",
    );
    let error = Scenario::new(source.build()).run().error().clone();
    assert!(
        error.to_string().contains("global was reassigned"),
        "{error}"
    );
    assert_eq!(error.at, at_line(16), "{error:?}");
}

/// A bare global (`x = 42` without `local`) resolves in prose read through
/// the lazy `prose` value, with dotted paths indexing into a table global
/// and a whole table rendering as JSON.
#[test]
fn bare_global_resolves_in_prose() {
    let source = PromptSource::new().prose(
        "Only",
        "
        ```lua
        answer = 42
        data = { score = 9 }
        ```

        The answer is {{ answer }}; score {{ data.score }}; raw {{ data }}.

        ```lua
        return prose
        ```",
    );
    assert_eq!(
        Scenario::new(source.build()).run().text(),
        "The answer is 42; score 9; raw {\"score\":9}."
    );
}

/// A `{{ }}` path whose first segment names no known namespace and no bare
/// global is a hard substitution error at the read site.
#[test]
fn missing_bare_global_in_prose_errors() {
    let source = PromptSource::new().prose(
        "Only",
        "
        {{ ghost }} here.

        ```lua
        return prose
        ```",
    );
    let trace = Scenario::new(source.build()).run();
    let error = trace.error();
    assert_eq!(
        run_error_kind(error.code),
        RunErrorKind::Lua,
        "the Lua read of `prose` raises the substitution failure: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("unknown namespace or global 'ghost'"),
        "the error must name the missing global: {error}"
    );
}

/// A bare global holding `json.null` reads as JSON null, so naming it in
/// prose is the null-value substitution failure a null `var` field is.
#[test]
fn bare_global_holding_json_null_in_prose_is_the_null_value_failure() {
    let source = PromptSource::new().prose(
        "Only",
        "
        ```lua
        nothing = json.null
        ```

        Value: {{ nothing }}.

        ```lua
        return prose
        ```",
    );
    let trace = Scenario::new(source.build()).run();
    let error = trace.error();
    assert_eq!(
        run_error_kind(error.code),
        RunErrorKind::Lua,
        "the Lua read of `prose` raises the substitution failure: {error}"
    );
    let rendered = error.to_string();
    assert!(
        rendered.contains("missing {{ nothing }}") && rendered.contains("[NullValue at byte "),
        "the error is the null-value failure naming the global: {rendered}"
    );
}

#[test]
fn sys_exposes_section_metadata() {
    let source = prompt(
        "",
        &[
            (
                "Alpha",
                "
                assert(sys.section_name == 'Alpha')
                assert(sys.execution == 'execute-test')
                assert(sys.section_count == 2)",
            ),
            (
                "Beta",
                "
                assert(sys.section_name == 'Beta')
                assert(sys.section_count == 2)
                assert(sys.execution == 'execute-test')
                return 'done'",
            ),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "done");
}
