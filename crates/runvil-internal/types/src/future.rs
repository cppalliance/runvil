//! The boxed future a trait method returns where the trait is used as a
//! trait object.
//!
//! [`BoxFuture`] is an alias over std alone, so it adds no dependency
//! and names no async runtime.

use std::future::Future;
use std::pin::Pin;

/// A boxed, `Send` future that may borrow for `'a`, the same type as
/// `futures::future::BoxFuture`.
///
/// A future that owns everything it uses is a `BoxFuture<'static, T>`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
