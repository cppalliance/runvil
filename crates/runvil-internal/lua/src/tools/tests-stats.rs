//! Tests for `tools.stats`: its `total` and its `packs` and `tools`
//! tables, each computed from the one id-keyed count map at each read,
//! the ids the `tools` table leaves out, the key order `pairs` walks, and
//! its refusals.

use runvil_types::tools::ToolId;

use crate::ToolCallCounts;
use crate::test_vm::{BuiltVm, catalog_set, echo_tool, lua_over, raised, tool_at};

/// A Lua helper that joins `pairs` over a table as `key=value`, in walk
/// order.
const WALK: &str = "local function walk(t)\n\
       local out = {}\n\
       for key, value in pairs(t) do out[#out + 1] = key .. '=' .. value end\n\
       return table.concat(out, ' ')\n\
     end\n";

/// The ending of an unknown-key error when the table holds some key and
/// the key names no tool the run knows.
const TYPO: &str = " - check for typos or offer it with tools.offer";

/// A VM whose run catalog holds `tools/echo`, `tools/other`, and
/// `web/fetch`, with the adhoc tool `grab` defined and counts installed
/// for `tools/echo`, `adhoc/grab`, both discovery tools, and `web/shaped`,
/// which stands for a tool only the model sees: neither the run catalog
/// nor the adhoc tools hold it.
fn seeded() -> (BuiltVm, ToolCallCounts) {
    let vm = lua_over(catalog_set([
        echo_tool(),
        tool_at("tools/other"),
        tool_at("web/fetch"),
    ]));
    vm.exec("tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end)");
    let ids = [
        "tools/echo",
        "adhoc/grab",
        "dynamic/search",
        "dynamic/call",
        "web/shaped",
    ]
    .map(|id| ToolId::parse(id).expect("a fixture id is a tool id"));
    let counts = vm
        .install_tool_call_counts(&ids)
        .expect("the counts install");
    (vm, counts)
}

/// Counts `times` calls of `id`, seeding it first as a dispatch does.
fn call(counts: &ToolCallCounts, id: &str, times: u32) {
    counts.ensure(id).expect("the counts seed");
    for _ in 0..times {
        counts.increment(id).expect("a seeded id counts");
    }
}

#[test]
fn total_packs_and_tools_are_computed_from_the_one_map_at_each_read() {
    let (vm, counts) = seeded();
    let read = || -> (i64, String, String) {
        vm.eval(&format!(
            "{WALK}return tools.stats.total, walk(tools.stats.packs), walk(tools.stats.tools)"
        ))
    };
    assert_eq!(
        read(),
        (
            0,
            "adhoc=0 dynamic=0 tools=0 web=0".to_owned(),
            "adhoc/grab=0 tools/echo=0".to_owned()
        ),
        "every seeded id starts at zero, and tools leaves out the discovery and model-only ids"
    );
    call(&counts, "tools/echo", 2);
    call(&counts, "tools/other", 1);
    call(&counts, "dynamic/search", 1);
    call(&counts, "web/shaped", 3);
    call(&counts, "gh/shaped", 1);
    assert_eq!(
        read(),
        (
            8,
            "adhoc=0 dynamic=1 gh=1 tools=3 web=3".to_owned(),
            "adhoc/grab=0 tools/echo=2 tools/other=1".to_owned()
        ),
        "every call counts toward total and its Pack, packs gains a Pack it did not start \
         with, and tools gains only an id Lua can see"
    );
    let direct: (i64, i64, i64) = vm.eval(
        "return tools.stats.packs.web, tools.stats.packs['tools'], tools.stats.tools['tools/echo']",
    );
    assert_eq!(direct, (3, 3, 2), "an index reads what pairs walks");
    let members: String = vm.eval(
        "local out = {}\n\
         for key, value in pairs(tools.stats) do out[#out + 1] = key .. ':' .. type(value) end\n\
         return table.concat(out, ' ')",
    );
    assert_eq!(
        members, "packs:table tools:table total:number",
        "pairs walks tools.stats's own three keys in order"
    );
}

#[test]
fn tools_stats_and_both_its_tables_refuse_writes_and_protect_their_metatables() {
    let (vm, _) = seeded();
    for (write, refusal) in [
        ("tools.stats.total = 1", "tools.stats is read-only"),
        ("tools.stats.calls = {}", "tools.stats is read-only"),
        (
            "tools.stats.packs.tools = 1",
            "tools.stats.packs is read-only",
        ),
        (
            "tools.stats.tools['tools/echo'] = 1",
            "tools.stats.tools is read-only",
        ),
    ] {
        let error = raised(&vm, write);
        assert!(error.contains(refusal), "{write}: {error}");
    }
    let labels: (String, String, String) = vm.eval(
        "return getmetatable(tools.stats), getmetatable(tools.stats.packs), \
           getmetatable(tools.stats.tools)",
    );
    assert_eq!(
        labels,
        (
            "tools.stats is read-only".to_owned(),
            "tools.stats.packs is read-only".to_owned(),
            "tools.stats.tools is read-only".to_owned()
        )
    );
    for table in ["tools.stats", "tools.stats.packs", "tools.stats.tools"] {
        let error = raised(&vm, &format!("setmetatable({table}, nil)"));
        assert!(
            error.contains("cannot change a protected metatable"),
            "{table}: {error}"
        );
    }
    let still: (i64, i64, i64) = vm
        .eval("return tools.stats.total, tools.stats.packs.tools, tools.stats.tools['tools/echo']");
    assert_eq!(still, (0, 0, 0), "a refused write changes no count");
}

#[test]
fn an_unknown_key_raises_naming_it_and_the_keys_held_never_a_discovery_or_model_only_id() {
    let (vm, counts) = seeded();
    call(&counts, "gh/shaped", 1);
    let stats = raised(&vm, "return tools.stats.calls");
    assert!(
        stats.contains("tools.stats: unknown key \"calls\"; accepted keys are total, packs, tools"),
        "{stats}"
    );
    for key in [
        "dynamic/search",
        "dynamic/call",
        "web/shaped",
        "gh/shaped",
        "tools/ghost",
    ] {
        let error = raised(&vm, &format!("return tools.stats.tools[{key:?}]"));
        assert!(
            error.contains(&format!(
                "tools.stats.tools: {key:?} has no seeded count; seeded names: \
                 [\"adhoc/grab\", \"tools/echo\"]{TYPO}"
            )),
            "{error}"
        );
    }
    let error = raised(&vm, "return tools.stats.packs.mail");
    assert!(
        error.contains(&format!(
            "tools.stats.packs: \"mail\" has no seeded count; seeded names: \
             [\"adhoc\", \"dynamic\", \"gh\", \"tools\", \"web\"]{TYPO}"
        )),
        "{error}"
    );
}

#[test]
fn tool_call_counts_seed_by_id_and_explain_an_unseeded_key() {
    let vm = lua_over(catalog_set([echo_tool(), tool_at("tools/other")]));
    vm.install_tool_call_counts(&[echo_tool().id])
        .expect("the counts install");
    vm.exec("tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end)");
    let seeded: i64 = vm.eval("return tools.stats.tools['tools/echo']");
    assert_eq!(seeded, 0, "counts key a catalog tool by its id");
    let offered = raised(&vm, "return tools.stats.tools['tools/other']");
    assert!(
        offered.contains(
            "tools.stats.tools: \"tools/other\" has no seeded count; seeded names: \
             [\"tools/echo\"] (a catalog tool that was neither offered in this section nor \
             called with tools.call)"
        ),
        "an offered id that was never seeded says so: {offered}"
    );
    let adhoc = raised(&vm, "return tools.stats.tools['adhoc/grab']");
    assert!(
        adhoc.contains(
            "tools.stats.tools: \"adhoc/grab\" has no seeded count; seeded names: \
             [\"tools/echo\"] (an adhoc tool that was neither offered in this section nor \
             called with tools.call)"
        ),
        "a defined adhoc tool's id that was never seeded says so, even when defined \
         after the counts install: {adhoc}"
    );
    for key in ["ghost", "tools_echo", "grab"] {
        let error = raised(&vm, &format!("return tools.stats.tools[{key:?}]"));
        assert!(
            error.contains(&format!("tools.stats.tools: {key:?} has no seeded count"))
                && error.contains(TYPO),
            "an unknown key, a wire name included, names itself and the remedy: {error}"
        );
    }
}

#[test]
fn with_nothing_seeded_an_unknown_key_lists_no_keys_and_takes_no_remedy() {
    let vm = lua_over(catalog_set([echo_tool()]));
    vm.install_tool_call_counts(&[])
        .expect("the counts install");
    for table in ["tools.stats.packs", "tools.stats.tools"] {
        let error = raised(&vm, &format!("return {table}['ghost']"));
        assert!(
            error.contains(&format!(
                "{table}: \"ghost\" has no seeded count; seeded names: []"
            )) && !error.contains(TYPO),
            "{error}"
        );
    }
}
