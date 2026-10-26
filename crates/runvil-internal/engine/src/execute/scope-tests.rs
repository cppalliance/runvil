//! Tests for the merge that builds a section entry's frozen sets: one per
//! rule, in the order the merge applies them, the search tool's list of
//! Dynamic Set names, and the order each set keeps. The checks rule one
//! makes of a render's answer, and the order around a render that adds a
//! tool, are in the `render` child.

use runvil_lua::ToolOutputKind;
use runvil_types::tools::ToolId;
use serde_json::json;

use super::*;

#[path = "scope-tests-render.rs"]
mod render;

/// The catalog tool `id`, described as `<id> tool`, under its default wire
/// name.
#[track_caller]
fn info(id: &str) -> ToolInfo {
    ToolInfo::new(
        ToolId::parse(id).expect("a fixture id is a tool id"),
        format!("{id} tool"),
        json!({ "type": "object" }),
    )
}

/// The catalog tool `id`, offered.
#[track_caller]
fn listed(id: &str) -> OfferedTool {
    OfferedTool::Catalog(info(id))
}

/// The adhoc tool `name`, offered.
#[track_caller]
fn adhoc(name: &str) -> OfferedTool {
    let id = ToolId::parse(&format!("adhoc/{name}")).expect("a fixture adhoc id is a tool id");
    OfferedTool::Adhoc(ToolBinding::from_info(
        name,
        &ToolInfo::new(id, "mine", json!({ "type": "object" })),
    ))
}

/// The run catalog of `tools`.
#[track_caller]
fn catalog(tools: impl IntoIterator<Item = ToolInfo>) -> ToolCatalog {
    ToolCatalog::new(tools).expect("the fixture ids differ")
}

/// Merges the offers of a section that made its static offers first, with
/// each Pack's offered tools passed through as its rendered tools.
fn merged(
    static_tools: &[OfferedTool],
    dynamic_tools: &[OfferedTool],
    catalog: &ToolCatalog,
) -> Result<RenderedTools> {
    let offers = SectionOffers::static_first(static_tools, dynamic_tools);
    merge_tool_sets(&offers, offered_packs(&offers), catalog)
}

/// Each binding of `set` as `<wire name>=<id>`, in order.
fn sited(set: &[ToolBinding]) -> Vec<String> {
    set.iter()
        .map(|binding| format!("{}={}", binding.alias(), binding.id()))
        .collect()
}

/// The Pack `pack`'s rendered tools: `static_set`, then `dynamic_set`.
#[track_caller]
fn sets(
    pack: &str,
    static_set: Vec<ToolInfo>,
    dynamic_set: Vec<ToolInfo>,
) -> PackToolSets<ToolInfo> {
    PackToolSets::new(
        PackId::parse(pack).expect("a fixture Pack id"),
        static_set,
        dynamic_set,
    )
}

/// The `web` Pack's rendered tools: `static_set`, then `dynamic_set`.
#[track_caller]
fn web(static_set: Vec<ToolInfo>, dynamic_set: Vec<ToolInfo>) -> Vec<PackToolSets<ToolInfo>> {
    vec![sets("web", static_set, dynamic_set)]
}

#[test]
fn rule_one_binds_each_tool_under_its_wire_name_and_keeps_a_catalog_ids_flags() {
    let search = info("web/search").structured(true).describes_tools(true);
    let fetch = info("web/fetch").wire_name("fetch");
    let tools = catalog([search.clone(), fetch.clone()]);
    let rendered = web(
        vec![
            info("web/search").wire_name("find"),
            fetch.clone(),
            info("web/only").structured(true),
        ],
        Vec::new(),
    );
    let offers = [OfferedTool::Catalog(search), OfferedTool::Catalog(fetch)];
    let offers = SectionOffers::static_first(&offers, &[]);
    let frozen = merge_tool_sets(&offers, rendered, &tools).expect("the sets merge");
    let bound: Vec<(&str, String, ToolOutputKind, bool)> = frozen
        .static_set
        .iter()
        .map(|binding| {
            (
                binding.alias(),
                binding.id().to_string(),
                binding.output_kind(),
                binding.describes_tools(),
            )
        })
        .collect();
    assert_eq!(
        bound,
        [
            (
                "find",
                "web/search".to_owned(),
                ToolOutputKind::Structured,
                true
            ),
            (
                "fetch",
                "web/fetch".to_owned(),
                ToolOutputKind::Plain,
                false
            ),
            (
                "web_only",
                "web/only".to_owned(),
                ToolOutputKind::Structured,
                false
            ),
        ],
        "a renamed catalog tool keeps the catalog's flags, a set wire name is used, \
         and a model-only tool keeps its own flags under its default name"
    );
}

#[test]
fn rule_two_drops_a_pack_tool_whose_wire_name_an_adhoc_tool_of_its_set_takes() {
    let tools = catalog([info("web/fetch"), info("web/search")]);
    let shadowing = vec![
        listed("web/fetch"),
        listed("web/search"),
        adhoc("web_search"),
    ];
    let kept = ["web_fetch=web/fetch", "web_search=adhoc/web_search"];
    for (set, static_tools, dynamic_tools) in [
        ("Static Set", shadowing.clone(), Vec::new()),
        ("Dynamic Set", Vec::new(), shadowing),
    ] {
        let frozen = merged(&static_tools, &dynamic_tools, &tools).expect("the sets merge");
        let held = if static_tools.is_empty() {
            &frozen.dynamic_set
        } else {
            &frozen.static_set
        };
        assert_eq!(sited(held), kept, "{set}: the adhoc tool stays in place");
    }
    let renamed = web(
        vec![
            info("web/fetch").wire_name("grab"),
            info("web/extra").wire_name("extra"),
        ],
        Vec::new(),
    );
    let offers =
        SectionOffers::static_first(&[listed("web/fetch"), adhoc("grab"), adhoc("extra")], &[]);
    let frozen = merge_tool_sets(&offers, renamed, &tools).expect("the sets merge");
    assert_eq!(
        sited(&frozen.static_set),
        ["grab=adhoc/grab", "extra=adhoc/extra"],
        "a renamed tool and a model-only tool are dropped the same way"
    );
}

#[test]
fn rule_three_drops_a_dynamic_set_tool_whose_id_the_static_set_holds() {
    let tools = catalog([info("web/fetch"), info("web/search")]);
    let both = web(
        vec![info("web/search"), info("web/only")],
        vec![info("web/only"), info("web/fetch")],
    );
    let offers = SectionOffers::static_first(&[listed("web/search")], &[listed("web/fetch")]);
    let frozen =
        merge_tool_sets(&offers, both, &tools).expect("the dropped tool clashes with nothing");
    let mut expected = vec!["web_search=web/search", "web_only=web/only"];
    expected.extend(DISCOVERY);
    assert_eq!(sited(&frozen.static_set), expected);
    assert_eq!(sited(&frozen.dynamic_set), ["web_fetch=web/fetch"]);
    let frozen = merged(&[adhoc("grab")], &[adhoc("grab")], &tools).expect("the sets merge");
    assert_eq!(sited(&frozen.static_set), ["grab=adhoc/grab"]);
    assert!(frozen.dynamic_set.is_empty(), "the adhoc tool sits once");
}

/// The two discovery tools as [`sited`] shows them, in the order they
/// join the Static Set.
const DISCOVERY: [&str; 2] = [
    "GetDynamicTools=dynamic/search",
    "CallDynamicTool=dynamic/call",
];

#[test]
fn rule_four_adds_both_discovery_tools_last_only_when_the_dynamic_set_keeps_a_tool() {
    let tools = catalog([info("web/fetch"), info("web/search"), info("gh/one")]);
    let none = merged(&[listed("web/fetch")], &[], &tools).expect("the sets merge");
    assert_eq!(sited(&none.static_set), ["web_fetch=web/fetch"]);
    let emptied =
        merged(&[listed("web/fetch")], &[listed("web/fetch")], &tools).expect("the sets merge");
    assert_eq!(
        sited(&emptied.static_set),
        ["web_fetch=web/fetch"],
        "a Dynamic Set that rule three empties adds nothing"
    );
    let frozen = merged(
        &[listed("web/fetch"), adhoc("grab")],
        &[listed("web/search"), adhoc("late"), listed("gh/one")],
        &tools,
    )
    .expect("the sets merge");
    let mut expected = vec!["web_fetch=web/fetch", "grab=adhoc/grab"];
    expected.extend(DISCOVERY);
    assert_eq!(sited(&frozen.static_set), expected);
    let search = &frozen.static_set[2];
    assert!(
        search
            .description()
            .ends_with(" (gh_one, late, web_search)"),
        "the search tool lists the Dynamic Set's wire names, sorted: {}",
        search.description()
    );
    let call = &frozen.static_set[3];
    for (binding, keys) in [
        (search, ["pattern", "toolName"]),
        (call, ["arguments", "toolName"]),
    ] {
        let mut declared: Vec<&String> = binding.schema()["properties"]
            .as_object()
            .expect("a discovery tool declares its parameters")
            .keys()
            .collect();
        declared.sort();
        assert_eq!(declared, keys, "{}", binding.alias());
    }
    assert_eq!(call.schema()["required"], json!(["toolName"]));
    assert!(
        search.schema().get("required").is_none(),
        "both search parameters are optional"
    );
}

#[test]
fn the_search_tools_description_lists_at_most_one_hundred_names_then_counts_the_rest() {
    let names = |count: usize| -> Vec<String> { (0..count).map(|n| format!("t{n:03}")).collect() };
    let described = |count: usize| {
        let offers: Vec<OfferedTool> = names(count).iter().rev().map(|name| adhoc(name)).collect();
        let frozen = merged(&[], &offers, &catalog([])).expect("the sets merge");
        frozen.static_set[0].description().to_owned()
    };
    let hundred = described(100);
    assert!(
        hundred.ends_with(&format!(" ({})", names(100).join(", "))),
        "a hundred names are listed whole: {hundred}"
    );
    let more = described(102);
    assert!(
        more.ends_with(&format!(
            " ({}, and 2 more; call GetDynamicTools to search)",
            names(100).join(", ")
        )),
        "past a hundred names the list says how many more: {more}"
    );
}

#[test]
fn rule_five_fails_a_wire_name_used_twice_within_one_set_or_across_the_sets() {
    let tools = catalog([info("dup/a.b"), info("dup/a/b"), info("web/search")]);
    let clash = |error: Error| match error {
        Error::ToolMerge {
            rule: MergeRule::UniqueWireName(wire_name),
            tools,
        } => (
            wire_name,
            tools
                .iter()
                .map(|tool| (tool.id.to_string(), tool.set))
                .collect::<Vec<_>>(),
        ),
        other => panic!("expected a wire-name clash, got {other:?}"),
    };
    let within = merged(&[listed("dup/a/b"), listed("dup/a.b")], &[], &tools)
        .expect_err("two tools of one set clash");
    assert_eq!(
        clash(within),
        (
            "dup_a_b".to_owned(),
            vec![
                ("dup/a/b".to_owned(), FrozenSet::Static),
                ("dup/a.b".to_owned(), FrozenSet::Static),
            ]
        )
    );
    let across = merged(&[adhoc("web_search")], &[listed("web/search")], &tools)
        .expect_err("a static adhoc tool and a dynamic Pack tool of its name clash");
    assert_eq!(
        clash(across),
        (
            "web_search".to_owned(),
            vec![
                ("adhoc/web_search".to_owned(), FrozenSet::Static),
                ("web/search".to_owned(), FrozenSet::Dynamic),
            ]
        )
    );
    let taker = info("web/discover").wire_name("GetDynamicTools");
    let tools = catalog([taker.clone(), info("web/search")]);
    let offers = [OfferedTool::Catalog(taker)];
    merged(&offers, &[], &tools).expect("with no Dynamic Set the name is free");
    let discovery = merged(&offers, &[listed("web/search")], &tools)
        .expect_err("a Pack tool takes the search tool's wire name");
    assert_eq!(
        clash(discovery),
        (
            "GetDynamicTools".to_owned(),
            vec![
                ("web/discover".to_owned(), FrozenSet::Static),
                ("dynamic/search".to_owned(), FrozenSet::Static),
            ]
        )
    );
}

#[test]
fn rule_six_fails_a_bad_wire_name_or_a_non_object_schema_alike_in_either_set() {
    let digit = info("9lives/fetch");
    let shapeless = ToolInfo::new(
        ToolId::parse("tools/shape").expect("a fixture id"),
        "shape",
        json!("scalar"),
    );
    let tools = catalog([digit.clone(), shapeless.clone()]);
    for (tool, alias) in [(digit, "9lives_fetch"), (shapeless, "tools_shape")] {
        let offered = vec![OfferedTool::Catalog(tool)];
        for (static_tools, dynamic_tools) in [(offered.clone(), Vec::new()), (Vec::new(), offered)]
        {
            let error = merged(&static_tools, &dynamic_tools, &tools)
                .expect_err("no round could advertise the tool");
            assert!(
                matches!(&error, Error::BindSchema { alias: refused, .. } if refused == alias),
                "{alias}: {error:?}"
            );
        }
    }
}

#[test]
fn each_set_keeps_offer_order_with_each_packs_tools_where_it_was_first_offered() {
    let tools = catalog(
        [
            "kit/a",
            "kit/b",
            "web/fetch",
            "web/search",
            "gh/one",
            "gh/two",
        ]
        .map(info),
    );
    let static_tools = [
        listed("kit/b"),
        listed("web/fetch"),
        adhoc("grab"),
        listed("kit/a"),
    ];
    let dynamic_tools = [
        listed("gh/one"),
        adhoc("late"),
        listed("web/search"),
        listed("gh/two"),
    ];
    let frozen = merged(&static_tools, &dynamic_tools, &tools).expect("the sets merge");
    let mut expected = vec![
        "kit_b=kit/b",
        "kit_a=kit/a",
        "web_fetch=web/fetch",
        "grab=adhoc/grab",
    ];
    expected.extend(DISCOVERY);
    assert_eq!(sited(&frozen.static_set), expected);
    assert_eq!(
        sited(&frozen.dynamic_set),
        [
            "gh_one=gh/one",
            "gh_two=gh/two",
            "late=adhoc/late",
            "web_search=web/search"
        ]
    );
}
