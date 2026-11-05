//! The pause that lets a running search be canceled.
//!
//! The Harness polls a call's future inside its effect loop, and a cancel
//! or a stop drops that future. A future that never returns `Pending`
//! cannot be dropped until it finishes, so Grep awaits a [`Pause`] every
//! so often. The pause wakes its own task and returns `Pending` once,
//! which hands control back to the Host's executor, and returns `Ready`
//! on the next poll. It names no async runtime.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A future that is `Pending` for one poll and then `Ready`.
#[derive(Debug, Default)]
pub(super) struct Pause {
    /// Whether the one `Pending` has been returned.
    yielded: bool,
}

impl Future for Pause {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.yielded {
            return Poll::Ready(());
        }
        self.yielded = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
