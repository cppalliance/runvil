//! A typed tool whose async method borrows its per-run state.

use std::future::Future;

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::tool_pack::Handler;
use crate::{BoxFuture, Error, OutputTrust, ToolContext, ToolOutput};

/// A stateful typed tool added through [`ToolPack::add`](crate::ToolPack::add).
/// Implement `call` as an ordinary `async fn`: it can borrow the tool's
/// configuration and the call's context across awaits. The Harness handles
/// schemas, argument decoding, output serialization, dispatch, and boxing.
///
/// Construct one inside [`PackRegistry::define`](crate::PackRegistry::define)
/// for each run. Calls in the same run may overlap, so shared mutable state
/// needs synchronization. Calls follow [`Pack::call`](crate::Pack::call)'s
/// nonblocking and cancellation rules.
pub trait Tool: Send + Sync + 'static {
    /// The tool's Pack-relative name, such as `request` or `files/read`.
    const NAME: &'static str;
    /// The description the model reads.
    const DESCRIPTION: &'static str;
    /// Whether results may be trusted. Choose `Untrusted` for external data.
    const TRUST: OutputTrust;
    /// A named-field argument struct. Derive `Deserialize` and `JsonSchema`.
    type Args: DeserializeOwned + JsonSchema + Send;
    /// The result, which is serialized as JSON with the declared trust.
    type Output: Serialize;

    /// Performs one call, borrowing this run's instance and the call's context.
    ///
    /// # Errors
    /// Return a tool error when the arguments or operation cannot be served.
    fn call(
        &self,
        cx: ToolContext<'_>,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output, Error>> + Send;
}

pub(crate) struct Typed<T>(pub(crate) T);

impl<T: Tool> Handler for Typed<T> {
    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            let args = crate::tool_schema::decode::<T::Args>(args)?;
            let output = self.0.call(cx, args).await?;
            crate::tool_schema::encode(output, T::TRUST)
        })
    }
}
