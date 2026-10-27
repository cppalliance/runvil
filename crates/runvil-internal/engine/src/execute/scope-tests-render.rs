//! Rule one's checks of a render's answer, one test per check and
//! table-driven where the shape allows, and the order each set keeps
//! around a render that reorders its Pack's tools and adds model-only
//! ones.

use super::*;

/// A failed merge's rule and the tools it names.
#[track_caller]
fn broken(error: Error) -> (SetRule, Vec<SetTool>) {
    match error {
        Error::ToolSets { rule, tools } => (rule, tools),
        other => panic!("expected a broken merge rule, got {other:?}"),
    }
}

/// The tool `id` in `set`, as a failed merge names it.
#[track_caller]
fn site(id: &str, set: OfferMode) -> SetTool {
    SetTool {
        id: ToolId::parse(id).expect("a fixture tool id"),
        set,
    }
}

/// The Pack id `id`.
#[track_caller]
fn pack(id: &str) -> PackId {
    PackId::parse(id).expect("a fixture Pack id")
}

#[test]
fn rule_one_refuses_an_answer_whose_packs_are_not_the_ones_asked_in_offer_order() {
    let tools = catalog([info("web/fetch"), info("gh/one")]);
    let offers = SectionOffers {
        static_tools: vec![listed("web/fetch")],
        dynamic_tools: vec![listed("gh/one")],
        packs: vec![pack("gh"), pack("web")],
    };
    let asked = offered_packs(&offers);
    assert_eq!(
        asked,
        [
            sets("gh", Vec::new(), vec![info("gh/one")]),
            sets("web", vec![info("web/fetch")], Vec::new()),
        ],
        "a Pack first offered into the Dynamic Set is asked before the Static Set's"
    );
    let (gh, web) = (asked[0].clone(), asked[1].clone());
    for (case, answered) in [
        ("the Static Set's Pack first", vec![web.clone(), gh.clone()]),
        ("one missing", vec![gh.clone()]),
        ("one twice", vec![gh.clone(), web.clone(), gh.clone()]),
        (
            "one never asked",
            vec![gh.clone(), sets("kit", Vec::new(), Vec::new())],
        ),
    ] {
        let answered_packs = answered.iter().map(|sets| sets.pack.clone()).collect();
        let error = merge_tool_sets(&offers, answered, &tools).expect_err(case);
        assert_eq!(
            broken(error),
            (
                SetRule::RenderedPacks {
                    asked: vec![pack("gh"), pack("web")],
                    answered: answered_packs,
                },
                Vec::new()
            ),
            "{case}"
        );
    }
}

#[test]
fn rule_one_refuses_a_rendered_tool_outside_what_its_pack_was_asked() {
    let tools = catalog(["web/fetch", "web/search", "web/other", "gh/one"].map(info));
    let offers = SectionOffers::static_first(
        &[listed("web/fetch")],
        &[listed("web/search"), listed("gh/one")],
    );
    let asked = offered_packs(&offers);
    let (web, gh) = (0, 1);
    let cases = [
        (
            "a model-only tool in a set its Pack was not offered into",
            gh,
            OfferMode::Static,
            "gh/new",
            SetRule::UnofferedSet(pack("gh")),
        ),
        (
            "a model-only tool under another Pack's id",
            web,
            OfferMode::Static,
            "gh/new",
            SetRule::ForeignTool(pack("web")),
        ),
        (
            "another Pack's catalog tool",
            web,
            OfferMode::Dynamic,
            "gh/one",
            SetRule::ForeignTool(pack("web")),
        ),
        (
            "a catalog tool the section never offered",
            web,
            OfferMode::Static,
            "web/other",
            SetRule::UnofferedCatalogTool,
        ),
        (
            "a catalog tool the section offered only into the Dynamic Set",
            web,
            OfferMode::Static,
            "web/search",
            SetRule::UnofferedCatalogTool,
        ),
        (
            "a catalog tool the section offered only into the Static Set",
            web,
            OfferMode::Dynamic,
            "web/fetch",
            SetRule::UnofferedCatalogTool,
        ),
    ];
    for (case, index, set, id, rule) in cases {
        let mut answered = asked.clone();
        set_mut(&mut answered[index], set).push(info(id));
        let error = merge_tool_sets(&offers, answered, &tools).expect_err(case);
        assert_eq!(broken(error), (rule, vec![site(id, set)]), "{case}");
    }
}

#[test]
fn rule_one_keeps_the_first_copy_of_a_tool_a_render_repeats_within_a_set() {
    let tools = catalog([info("web/fetch")]);
    let rendered = web(
        vec![
            info("web/fetch").wire_name("first"),
            info("web/only"),
            info("web/fetch").wire_name("second"),
            info("web/only").wire_name("again"),
        ],
        Vec::new(),
    );
    let offers = SectionOffers::static_first(&[listed("web/fetch")], &[]);
    let frozen =
        merge_tool_sets(&offers, rendered, &tools).expect("a repeat is dropped, not refused");
    assert_eq!(
        sited(&frozen.static_set),
        ["first=web/fetch", "web_only=web/only"]
    );
}

#[test]
fn each_set_keeps_offer_order_around_a_render_that_reorders_and_adds_model_only_tools() {
    let tools = catalog(["kit/a", "kit/b", "web/fetch", "gh/one"].map(info));
    let static_tools = [
        listed("kit/b"),
        adhoc("grab"),
        listed("web/fetch"),
        listed("kit/a"),
    ];
    let dynamic_tools = [adhoc("late"), listed("gh/one")];
    let offers = SectionOffers::static_first(&static_tools, &dynamic_tools);
    let mut rendered = offered_packs(&offers);
    let packs: Vec<String> = rendered.iter().map(|sets| sets.pack.to_string()).collect();
    assert_eq!(packs, ["kit", "web", "gh"]);
    rendered[0].static_set = vec![info("kit/extra"), info("kit/a"), info("kit/b")];
    rendered[2].dynamic_set.push(info("gh/added"));
    let frozen = merge_tool_sets(&offers, rendered, &tools).expect("the sets merge");
    let mut expected = vec![
        "kit_extra=kit/extra",
        "kit_a=kit/a",
        "kit_b=kit/b",
        "grab=adhoc/grab",
        "web_fetch=web/fetch",
    ];
    expected.extend(DISCOVERY);
    assert_eq!(
        sited(&frozen.static_set),
        expected,
        "the kit block sits where kit/b was first offered, in the render's order"
    );
    assert_eq!(
        sited(&frozen.dynamic_set),
        ["late=adhoc/late", "gh_one=gh/one", "gh_added=gh/added"]
    );
}
