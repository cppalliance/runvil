//! [`ToolContext`], what one tool call receives, and [`RenderContext`],
//! what a render learns about its section entry.

use runvil_types::models::ModelInfo;
use runvil_types::tools::{ToolCallOrigin, ToolId};
use runvil_vfs::Access;

/// What one tool call lends its Pack: facts about that call alone, the
/// called tool's id, the call's filesystem access, and who made the call.
///
/// The access is the call's own identity, forked from the calling chain's,
/// so the chain's earlier work happens before the call's operations and
/// the call's operations before the chain's next step. Calls a model asks
/// for in one round fork their identities from the chain together, so they
/// are concurrent with each other: one call's write to a path conflicts
/// with a sibling's read or write of it, as between fanout arms. It is
/// rooted at `/`. The context borrows everything for the call alone, so a
/// Pack cannot store the access, move it into a spawned task or thread,
/// or keep it past the call.
///
/// A call gets no services. A Plugin's `install` keeps the Host-wide ones
/// its Pack needs in the Pack's [`PackFactory`](crate::PackFactory), and
/// each run's instance takes what it needs of the run's own at
/// [`new_pack`](crate::PackFactory::new_pack).
#[derive(Debug)]
pub struct ToolContext<'a> {
    tool: &'a ToolId,
    access: &'a Access,
    origin: &'a ToolCallOrigin,
}

impl<'a> ToolContext<'a> {
    /// Facts about one call: the tool id, its access, and its origin. The
    /// Harness builds one per call.
    #[must_use]
    pub fn new(tool: &'a ToolId, access: &'a Access, origin: &'a ToolCallOrigin) -> Self {
        Self {
            tool,
            access,
            origin,
        }
    }

    /// The tool that was called. A Pack with several tools matches on
    /// its [`name`](ToolId::name) to pick one.
    #[must_use]
    pub fn tool(&self) -> &'a ToolId {
        self.tool
    }

    /// The call's filesystem access, rooted at `/`.
    #[must_use]
    pub fn access(&self) -> &'a Access {
        self.access
    }

    /// Who made the call and where.
    #[must_use]
    pub fn origin(&self) -> &'a ToolCallOrigin {
        self.origin
    }
}

/// What a Pack learns about the section entry it renders for, in
/// [`Pack::render`](crate::Pack::render).
///
/// A section entry is one execution of a section: re-entering a section,
/// a fanout arm, a task, and a child started with `call` are each their
/// own entry. The context holds plain data only, free of Rust-only handles
/// such as [`HostServices`](crate::HostServices), so a later C verb can
/// pass it serialized. The Harness builds it with [`RenderContext::new`],
/// and later facts join as fields without breaking existing Packs.
#[derive(Debug)]
#[non_exhaustive]
pub struct RenderContext<'a> {
    /// The section entry, the value Lua reads as `sys.id`.
    pub entry: &'a str,
    /// The model of the section's first round.
    pub model: &'a ModelInfo,
}

impl<'a> RenderContext<'a> {
    /// A context naming the section entry `entry` and the model of its
    /// first round, `model`. The Harness builds one per render.
    #[must_use]
    pub fn new(entry: &'a str, model: &'a ModelInfo) -> Self {
        Self { entry, model }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the contexts a Pack receives: a tool call's getters, and
    //! the facts a render learns.

    use std::num::NonZeroU32;

    use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};
    use runvil_types::tools::{ToolCallOrigin, ToolCaller, ToolId};
    use runvil_vfs::{Access, Origin, VfsRef};

    use super::{RenderContext, ToolContext};

    fn fetch() -> ToolId {
        ToolId::parse("web/fetch").expect("a two-segment tool id parses")
    }

    fn fresh_access() -> Access {
        VfsRef::default()
            .acquire(Origin::new("context test"))
            .expect("a fresh memory filesystem acquires")
    }

    fn model_origin() -> ToolCallOrigin {
        ToolCallOrigin::new("Fetch", ToolCaller::Model)
    }

    #[test]
    fn each_getter_returns_the_value_the_context_was_built_from() {
        let (tool, access, origin) = (fetch(), fresh_access(), model_origin());
        let cx = ToolContext::new(&tool, &access, &origin);
        assert!(std::ptr::eq(cx.tool(), &raw const tool));
        assert!(std::ptr::eq(cx.access(), &raw const access));
        assert!(std::ptr::eq(cx.origin(), &raw const origin));
        assert_eq!(cx.tool().name(), "fetch");
        assert_eq!(cx.origin().caller, ToolCaller::Model);
    }

    #[test]
    fn a_render_context_holds_the_entry_and_the_model_it_was_built_with() {
        let model = ModelInfo::new(
            ModelId::gateway("analyst").expect("a valid model id"),
            "A careful analysis model",
            NonZeroU32::new(8_192).expect("a non-zero context window"),
            ThinkingMode::Switchable,
        );
        let entry = String::from("chat-1");
        let cx = RenderContext::new(&entry, &model);
        assert_eq!(cx.entry, "chat-1");
        assert!(std::ptr::eq(cx.model, &raw const model));
        assert_eq!(cx.model.id().name(), "analyst");
    }
}
