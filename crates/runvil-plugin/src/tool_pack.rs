//! A Pack assembled from async tool handlers.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    BoxFuture, Error, ErrorCode, OutputTrust, Pack, PackId, Tool, ToolContext, ToolId, ToolInfo,
    ToolOutput,
};

pub(crate) trait Handler: Send + Sync {
    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>>;
}

impl<F> Handler for F
where
    F: for<'a> Fn(ToolContext<'a>, Value) -> BoxFuture<'a, Result<ToolOutput, Error>> + Send + Sync,
{
    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        self(cx, args)
    }
}

/// One run's Pack, assembled by [`PackRegistry::define`](crate::PackRegistry::define).
/// Each call is dispatched to its own handler. The factory builds fresh
/// handlers for each run; capture shared clients or per-run state there.
pub struct ToolPack {
    id: PackId,
    tools: BTreeMap<ToolId, (ToolInfo, Box<dyn Handler>)>,
}

impl ToolPack {
    pub(crate) fn new(id: PackId) -> Self {
        Self {
            id,
            tools: BTreeMap::new(),
        }
    }

    /// Adds a typed stateful tool. Its async [`Tool::call`] method borrows
    /// this instance, so clients and configuration need not be cloned per
    /// call. Construct the tool inside the registry's factory to give each
    /// run its own instance.
    ///
    /// # Errors
    /// Returns the same registration errors as [`tool`](Self::tool).
    pub fn add<T: Tool>(&mut self, tool: T) -> Result<(), Error> {
        let id = ToolId::parse(&format!("{}/{}", self.id, T::NAME))?;
        let info = ToolInfo::new(
            id,
            T::DESCRIPTION,
            crate::tool_schema::parameters::<T::Args>()?,
        )
        .structured(T::TRUST == OutputTrust::Trusted);
        self.insert(info, Box::new(crate::tool::Typed(tool)))
    }

    /// Adds an async function or closure with typed arguments and output.
    /// Derive `Deserialize` and `JsonSchema` for an argument struct;
    /// field documentation becomes the generated schema's descriptions.
    /// The output must implement `Serialize`. Trust is explicit: choose
    /// `Untrusted` for output containing external or attacker-controlled data.
    ///
    /// Names are relative to the Pack, such as `request` or `files/read`.
    /// Bad arguments are answered with `ToolInvalidArguments` before the
    /// handler runs. Trusted JSON results reach scripts as structured data;
    /// untrusted results retain the Engine's untrusted envelope.
    ///
    /// # Errors
    /// Returns `InvalidValue` for an invalid or duplicate name or an
    /// argument type whose schema is not an object.
    pub fn tool<Args, Output, F, Fut>(
        &mut self,
        name: &str,
        description: impl Into<String>,
        trust: OutputTrust,
        handler: F,
    ) -> Result<(), Error>
    where
        Args: DeserializeOwned + JsonSchema + Send + 'static,
        Output: Serialize + 'static,
        F: Fn(Args) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Output, Error>> + Send + 'static,
    {
        self.tool_with_context(name, description, trust, move |_, args| {
            Box::pin(handler(args))
        })
    }

    /// Adds a typed handler that also borrows the call's context. Return
    /// `Box::pin(async move { ... })` to borrow its filesystem and origin
    /// across awaits. Arguments, output, and trust follow [`tool`](Self::tool).
    ///
    /// # Errors
    /// Returns the same registration errors as [`tool`](Self::tool).
    pub fn tool_with_context<Args, Output, F>(
        &mut self,
        name: &str,
        description: impl Into<String>,
        trust: OutputTrust,
        handler: F,
    ) -> Result<(), Error>
    where
        Args: DeserializeOwned + JsonSchema + Send + 'static,
        Output: Serialize + 'static,
        F: for<'a> Fn(ToolContext<'a>, Args) -> BoxFuture<'a, Result<Output, Error>>
            + Send
            + Sync
            + 'static,
    {
        let parameters = crate::tool_schema::parameters::<Args>()?;
        self.raw_tool_with_context(name, description, parameters, move |cx, args| {
            let handler = &handler;
            // Decode before constructing the async handler's future, so the
            // future borrows only this call's context, never the registry.
            match crate::tool_schema::decode::<Args>(args) {
                Ok(args) => {
                    let answer = handler(cx, args);
                    Box::pin(async move { crate::tool_schema::encode(answer.await?, trust) })
                }
                Err(error) => Box::pin(async move { Err(error) }),
            }
        })?;
        let id = ToolId::parse(&format!("{}/{name}", self.id))?;
        if let Some((info, _)) = self.tools.get_mut(&id) {
            info.structured_output = trust == OutputTrust::Trusted;
        }
        Ok(())
    }

    /// Adds an async function or closure under a Pack-relative name,
    /// such as `request` or `files/read`. The handler receives JSON
    /// arguments and explicitly marks the trust of its output.
    ///
    /// # Errors
    /// Returns `InvalidValue` for an invalid or duplicate name.
    pub fn raw_tool<F, Fut>(
        &mut self,
        name: &str,
        description: impl Into<String>,
        parameters: Value,
        handler: F,
    ) -> Result<(), Error>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolOutput, Error>> + Send + 'static,
    {
        self.raw_tool_with_context(name, description, parameters, move |_, args| {
            Box::pin(handler(args))
        })
    }

    /// Adds a handler that borrows the call's filesystem and origin.
    /// The handler returns `Box::pin(async move { ... })` so its future
    /// can borrow the context for the duration of the call.
    ///
    /// # Errors
    /// Returns `InvalidValue` for an invalid or duplicate name.
    pub fn raw_tool_with_context<F>(
        &mut self,
        name: &str,
        description: impl Into<String>,
        parameters: Value,
        handler: F,
    ) -> Result<(), Error>
    where
        F: for<'a> Fn(ToolContext<'a>, Value) -> BoxFuture<'a, Result<ToolOutput, Error>>
            + Send
            + Sync
            + 'static,
    {
        let id = ToolId::parse(&format!("{}/{name}", self.id))?;
        self.insert(
            ToolInfo::new(id, description, parameters),
            Box::new(handler),
        )
    }

    fn insert(&mut self, info: ToolInfo, handler: Box<dyn Handler>) -> Result<(), Error> {
        let id = &info.id;
        if self.tools.contains_key(id) {
            let message = format!("tool `{id}` is already in this Pack");
            return Err(Error::new(
                ErrorCode::InvalidValue,
                message.clone(),
                message,
            ));
        }
        self.tools.insert(id.clone(), (info, handler));
        Ok(())
    }
}

impl Pack for ToolPack {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.values().map(|(info, _)| info.clone()).collect()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        if let Some((_, handler)) = self.tools.get(cx.tool()) {
            handler.call(cx, args)
        } else {
            let message = format!("tool `{}` is not in this Pack", cx.tool());
            Box::pin(async move { Err(crate::make_tool_error(ErrorCode::ToolOther, message)) })
        }
    }
}

impl fmt::Debug for ToolPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolPack")
            .field("id", &self.id)
            .field("tools", &self.tools.keys())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "tool_pack-tests.rs"]
mod tests;
