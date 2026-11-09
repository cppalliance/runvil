//! [`TestCall`]: lends a [`ToolContext`] to a Plugin crate's own tests.

use runvil_types::tools::{ToolCallOrigin, ToolCaller, ToolId};
use runvil_vfs::{Access, Origin, VfsRef};

use crate::ToolContext;

/// One tool call a test makes directly, without a Harness.
///
/// It owns everything a [`ToolContext`] borrows: the called tool's id, an
/// access over a fresh in-memory filesystem, and an origin naming the
/// section `Test` and a caller that is the script unless
/// [`with_caller`](TestCall::with_caller) names another. A test calls the
/// Pack's factory for the instance it calls.
#[derive(Debug)]
pub struct TestCall {
    tool: ToolId,
    access: Access,
    origin: ToolCallOrigin,
}

impl TestCall {
    /// A call of `tool` from the script of the section `Test`, over a
    /// fresh in-memory filesystem.
    ///
    /// # Panics
    /// Panics when the fresh in-memory filesystem refuses the access,
    /// which its backend never does.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "a test fixture; the in-memory backend accepts every acquire"
    )]
    pub fn new(tool: ToolId) -> TestCall {
        let access = VfsRef::default()
            .acquire(Origin::new("plugin test"))
            .expect("a fresh memory filesystem acquires");
        let origin = ToolCallOrigin::new("Test", ToolCaller::Script);
        TestCall {
            tool,
            access,
            origin,
        }
    }

    /// Returns the call with `caller` as the origin's caller, keeping its
    /// section.
    #[must_use]
    pub fn with_caller(mut self, caller: ToolCaller) -> TestCall {
        self.origin = ToolCallOrigin::new("Test", caller);
        self
    }

    /// Lends the context to one call.
    #[must_use]
    pub fn context(&self) -> ToolContext<'_> {
        ToolContext::new(&self.tool, &self.access, &self.origin)
    }
}
