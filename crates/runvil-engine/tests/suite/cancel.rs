//! Stopping runs from any thread, by a parent cancel or a handle's
//! `Canceled` future.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::Effect;

use super::support::{Scenario, TestResult, note};

/// A std-only executor's waker.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future + Unpin>(mut future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = Pin::new(&mut future).poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

#[test]
fn one_parent_cancel_ends_every_run_holding_one_of_its_children() -> TestResult {
    let source = note("'hello'");
    let parent = CancelHandle::new();
    let mut drivers = Vec::new();
    for _ in 0..2 {
        let parent = parent.clone();
        let mut driver = Scenario::default()
            .hold(|effect| matches!(effect, Effect::Vfs { .. }))
            .params(move |ctx| ctx.with_cancel(parent.child()))
            .start(&source)?;
        driver.pump_until_held()?;
        drivers.push(driver);
    }

    parent.cancel();

    let mut canceled = Vec::new();
    for driver in drivers {
        canceled.push(driver.finish()?.error()?.is_canceled());
    }
    assert_eq!(canceled, [true, true], "both runs end with the cancel");
    Ok(())
}

#[test]
fn a_canceled_future_wakes_when_another_thread_cancels_the_run() -> TestResult {
    let handle = CancelHandle::new();
    let handed = handle.clone();
    let driver = Scenario::default()
        .params(move |ctx| ctx.with_cancel(handed.clone()))
        .start(&note("'hello'"))?;
    let waiting = handle.canceled();

    let remote = handle.clone();
    let canceller = thread::spawn(move || remote.cancel());

    block_on(waiting);
    canceller
        .join()
        .map_err(|_| "the canceling thread panicked")?;
    assert!(handle.is_canceled());
    assert!(
        driver.finish()?.error()?.is_canceled(),
        "the run reads the flag the caller handed its params"
    );
    Ok(())
}
