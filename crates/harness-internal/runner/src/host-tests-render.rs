//! The snapshot's renders and its routing by Pack: a render asks each
//! usable Pack once for each set it was offered into, with the section
//! entry and the round's model, and answers every Pack asked in the order
//! asked, a Pack the run cannot use keeping its tools as offered; and a
//! call goes to the Pack its id names when the catalog lacks the tool.

use std::num::NonZeroU32;

use runvil::effect::PackToolSets;
use runvil_plugin::{ModelId, ModelInfo, RenderContext, ThinkingMode};

use super::*;

/// A Host-wide service each [`Rendering`] Pack logs its renders in.
const RENDERS: ServiceKey<Mutex<Vec<Rendered>>> = ServiceKey::new("tests/renders");

/// One render a [`Rendering`] Pack performed.
#[derive(Debug, Clone, PartialEq)]
struct Rendered {
    pack: String,
    entry: String,
    model: ModelInfo,
    offered: Vec<String>,
}

/// A [`Listed`] Pack whose render logs what it is asked in [`RENDERS`]
/// and adds `<pack>/extra`, a tool only the model sees.
struct Rendering {
    name: PackId,
    listed: Arc<dyn Pack>,
    renders: Arc<Mutex<Vec<Rendered>>>,
}

/// Builds a [`Rendering`] Pack from a [`listed`] configuration, failing
/// when the Host provides no [`RENDERS`].
fn rendering(
    name: &PackId,
    config: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let renders = services
        .get(&RENDERS)
        .ok_or_else(|| ToolError::message("rendering needs tests/renders"))?;
    Ok(Arc::new(Rendering {
        name: name.clone(),
        listed: listed(name, config, services)?,
        renders,
    }))
}

impl Pack for Rendering {
    fn tools(&self) -> Vec<ToolInfo> {
        self.listed.tools()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        self.listed.call(cx, args)
    }

    fn render<'a>(
        &'a self,
        cx: RenderContext<'a>,
        mut offered: Vec<ToolInfo>,
    ) -> PackFuture<'a, Vec<ToolInfo>> {
        self.renders.lock().unwrap().push(Rendered {
            pack: self.name.to_string(),
            entry: cx.entry.to_owned(),
            model: cx.model.clone(),
            offered: offered.iter().map(|tool| tool.id.to_string()).collect(),
        });
        offered.push(listed_tool(&format!("{}/extra", self.name)));
        Box::pin(async move { offered })
    }
}

/// The model a render here is asked for.
fn model() -> ModelInfo {
    ModelInfo::new(
        ModelId::gateway("m").unwrap(),
        "The round's model",
        NonZeroU32::new(4096).unwrap(),
        ThinkingMode::Never,
    )
}

/// The Pack `pack`'s offered tools, `static_set` then `dynamic_set`.
fn sets(pack: &str, static_set: &[&str], dynamic_set: &[&str]) -> PackToolSets<ToolInfo> {
    let tools = |ids: &[&str]| ids.iter().map(|id| listed_tool(id)).collect();
    PackToolSets::new(name(pack), tools(static_set), tools(dynamic_set))
}

/// Owned copies of `ids`.
fn owned(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// Each Pack of `answer` with its tool ids per set, in order.
fn ids(answer: &[PackToolSets<ToolInfo>]) -> Vec<(String, Vec<String>, Vec<String>)> {
    let ids = |tools: &[ToolInfo]| tools.iter().map(|tool| tool.id.to_string()).collect();
    answer
        .iter()
        .map(|sets| {
            (
                sets.pack.to_string(),
                ids(&sets.static_set),
                ids(&sets.dynamic_set),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_render_asks_each_usable_pack_once_per_set_it_was_offered_into_and_answers_in_order() {
    let renders = Arc::new(Mutex::new(Vec::new()));
    let mut wide = HostServices::new();
    wide.provide(&RENDERS, Arc::clone(&renders)).unwrap();
    let needy = PackInfo::new("runvil/needy", rendering).needs(NEEDS_SESSION);
    let host = host_with(
        wide,
        &[
            (
                PackInfo::new("runvil/one", rendering),
                offering(&["one/a", "one/b"]),
            ),
            (PackInfo::new("runvil/two", rendering), offering(&["two/a"])),
            (needy, offering(&["needy/a"])),
        ],
    );
    let (run, _requirements) = begin(&host, HostServices::new(), BTreeMap::new(), &prompt(""));
    let offered = vec![
        sets("two", &["two/a"], &[]),
        sets("one", &["one/a"], &["one/b"]),
        sets("needy", &[], &["needy/a"]),
    ];
    let answer = run.render("0.1".to_owned(), model(), offered).await;

    let asked = |pack: &str, offered: &[&str]| Rendered {
        pack: pack.to_owned(),
        entry: "0.1".to_owned(),
        model: model(),
        offered: owned(offered),
    };
    assert_eq!(
        *renders.lock().unwrap(),
        [
            asked("two", &["two/a"]),
            asked("one", &["one/a"]),
            asked("one", &["one/b"]),
        ],
        "each usable Pack is asked once per set it was offered into, with the entry and the \
         model, and never for an empty set"
    );
    assert_eq!(
        ids(&answer),
        [
            ("two".to_owned(), owned(&["two/a", "two/extra"]), owned(&[])),
            (
                "one".to_owned(),
                owned(&["one/a", "one/extra"]),
                owned(&["one/b", "one/extra"])
            ),
            ("needy".to_owned(), owned(&[]), owned(&["needy/a"])),
        ],
        "every Pack asked is answered in the order asked, and the one the run cannot use keeps \
         its tools as offered"
    );
}

#[tokio::test]
async fn a_call_goes_to_the_pack_its_id_names_when_the_catalog_lacks_the_tool() {
    let host = host(&[(info("runvil/one"), offering(&["one/echo"]))]);
    let (run, _requirements) = begin(&host, HostServices::new(), BTreeMap::new(), &prompt(""));
    let extra = ToolId::parse("one/extra").unwrap();
    assert!(
        run.catalog().get(&extra).is_none(),
        "only a render shows one/extra"
    );
    let answer = call(&run, "one/extra")
        .await
        .expect("the call reaches the one Pack");
    assert_eq!(answer.text(), "one:one/extra");
}
