//! Tests for a tool call through a Plugin's entry point: what each step
//! is sent, how a result becomes an output or a failure, when the future
//! is pending, and that RV_TOOL_CANCEL comes once for a call whose last
//! step returned RV_PENDING and for no other call.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use runvil_plugin::{ErrorCode, OutputTrust};
use serde_json::json;

use crate::fakes::*;

/// The message of a call that failed because its Plugin misused it.
macro_rules! misuse_reason {
    ($detail:expr) => {
        concat!("the tool's Plugin misused the tool call: ", $detail)
    };
}

/// Counts how often it is woken.
#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

pack_fake!(
    echo,
    {
        /// Whether every step carried the contract's fields.
        pub(super) static WELL_FORMED: AtomicUsize = AtomicUsize::new(1);
    },
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "echo");
        add_tool(call, "page");
        add_tool(call, "fails")
    },
    step: |call| {
        if !contract_holds(call, RV_TOOL_CALL) || pack_run(call) != instance(PACK_TAG) {
            WELL_FORMED.store(0, Ordering::SeqCst);
        }
        let args = step_args(call);
        let (tool, arguments) = (text(args.tool), text(args.arguments));
        match tool.as_str() {
            "echo" => finish(call, RV_RESULT_TRUSTED, &format!("{tool} {arguments}")),
            "page" => finish(call, RV_RESULT_UNTRUSTED, "text from outside"),
            _ => finish(
                call,
                arguments.parse().unwrap_or(RV_RESULT_OTHER),
                &format!("failed as {arguments}"),
            ),
        }
    }
);

#[test]
fn a_call_sends_the_tool_name_and_the_arguments_as_json_text() {
    let pack = made(echo::entry);
    let output = call_tool(&*pack, "echo", json!({ "b": [1, 2], "a": "x" })).expect("the call");
    assert_eq!(output.text(), r#"echo {"a":"x","b":[1,2]}"#);
    assert_eq!(output.trust(), OutputTrust::Trusted);
    assert_eq!(echo::WELL_FORMED.load(Ordering::SeqCst), 1);
    assert_eq!(echo::CANCELS.load(Ordering::SeqCst), 0);
}

#[test]
fn an_untrusted_result_is_an_untrusted_output() {
    let pack = made(echo::entry);
    let output = call_tool(&*pack, "page", json!({})).expect("the call");
    assert_eq!(output.text(), "text from outside");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn each_failure_kind_is_a_tool_error_of_its_code_with_the_text_as_message_and_prose() {
    let pack = made(echo::entry);
    for (kind, code) in [
        (RV_RESULT_INVALID_ARGUMENTS, ErrorCode::ToolInvalidArguments),
        (RV_RESULT_BACKEND, ErrorCode::ToolBackend),
        (RV_RESULT_TRANSPORT, ErrorCode::ToolTransport),
        (RV_RESULT_OTHER, ErrorCode::ToolOther),
    ] {
        let failure = call_tool(&*pack, "fails", json!(kind)).expect_err("the call fails");
        assert_eq!(failure.code, code, "kind {kind}");
        assert_eq!(failure.message, format!("failed as {kind}"));
        assert_eq!(failure.prose, failure.message);
    }
}

pack_fake!(
    slow,
    {
        /// The `tool_call` each step was sent, in order.
        pub(super) static STATES: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    },
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "slow")
    },
    step: |call| {
        let steps = tool_call(call).addr();
        if let Ok(mut states) = STATES.lock() {
            states.push(steps);
        }
        if steps < 2 {
            set_tool_call(call, ptr::without_provenance_mut(steps + 1));
            return RV_PENDING;
        }
        finish(call, RV_RESULT_TRUSTED, "done")
    }
);

#[test]
fn a_pending_step_wakes_its_task_and_the_next_poll_sends_the_next_step() {
    let pack = made(slow::entry);
    let call = test_call("slow");
    let mut future = pin!(pack.call(call.context(), json!({})));
    let count = Arc::new(Wakes::default());
    let waker = Waker::from(Arc::clone(&count));
    let mut cx = Context::from_waker(&waker);

    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(count.0.load(Ordering::SeqCst), 2);
    let Poll::Ready(result) = future.as_mut().poll(&mut cx) else {
        panic!("the third step finishes the call");
    };
    assert_eq!(result.expect("the call succeeds").text(), "done");
    assert_eq!(
        count.0.load(Ordering::SeqCst),
        2,
        "the step that ends the call wakes nothing"
    );
    assert_eq!(
        *slow::STATES.lock().expect("the lock holds"),
        [0, 1, 2],
        "each step gets the state the last one left, and the first gets NULL"
    );
    assert_eq!(
        slow::CANCELS.load(Ordering::SeqCst),
        0,
        "a call that ended gets no cancel"
    );
}

pack_fake!(
    abandoned,
    {},
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "slow")
    },
    step: |call| {
        set_tool_call(call, instance(0x300));
        RV_PENDING
    }
);

#[test]
fn dropping_a_pending_call_sends_one_cancel_with_its_state_before_the_delete() {
    let pack = made(abandoned::entry);
    let call = test_call("slow");
    let mut future = Box::pin(pack.call(call.context(), json!({})));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(abandoned::CANCELS.load(Ordering::SeqCst), 0);
    drop(future);
    assert_eq!(abandoned::CANCELS.load(Ordering::SeqCst), 1);
    assert_eq!(abandoned::CANCELED.load(Ordering::SeqCst), instance(0x300));
    drop(pack);
    assert_eq!(
        *abandoned::ORDER.lock().expect("the lock holds"),
        [RV_TOOL_CANCEL, RV_PACK_DELETE, RV_PLUGIN_UNLOAD]
    );
}

pack_fake!(
    unsent,
    {},
    new: |call| {
        set_pack_run(call, instance(PACK_TAG));
        add_tool(call, "slow")
    },
    step: |_call| RV_PENDING
);

#[test]
fn a_call_that_was_never_polled_sends_nothing() {
    let pack = made(unsent::entry);
    let call = test_call("slow");
    drop(pack.call(call.context(), json!({})));
    assert_eq!(unsent::CANCELS.load(Ordering::SeqCst), 0);
}

#[test]
fn a_pending_call_that_set_no_state_is_canceled_with_null() {
    let pack = made(unsent::entry);
    let call = test_call("slow");
    let mut future = Box::pin(pack.call(call.context(), json!({})));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    drop(future);
    assert_eq!(unsent::CANCELS.load(Ordering::SeqCst), 1);
    assert!(unsent::CANCELED.load(Ordering::SeqCst).is_null());
}

/// Defines a test whose fake step runs `$step`, which returns the step's
/// status. The call must fail as a `ToolOther` error whose message is
/// `$message`, and must get `$cancels` RV_TOOL_CANCEL calls.
macro_rules! failed_call_test {
    ($test:ident, $message:expr, $cancels:expr, |$call:ident| $step:expr) => {
        #[test]
        fn $test() {
            pack_fake!(
                plug,
                {},
                new: |call| {
                    set_pack_run(call, instance(PACK_TAG));
                    add_tool(call, "tool")
                },
                step: |$call| $step
            );
            let pack = made(plug::entry);
            let failure = call_tool(&*pack, "tool", json!({})).expect_err("the call fails");
            assert_eq!(failure.code, ErrorCode::ToolOther);
            assert_eq!(failure.message, $message);
            assert_eq!(plug::CANCELS.load(Ordering::SeqCst), $cancels);
        }
    };
}

failed_call_test!(
    a_step_that_returns_ok_without_finishing_fails_the_call,
    "the tool's Plugin answered RV_TOOL_CALL with RV_OK without finishing the call",
    0,
    |_call| RV_OK
);

failed_call_test!(
    a_step_that_returns_another_status_fails_the_call_with_it,
    "the tool's Plugin answered RV_TOOL_CALL with status 77",
    0,
    |_call| ODD_STATUS
);

failed_call_test!(
    a_status_other_than_ok_wins_over_a_result,
    "the tool's Plugin answered RV_TOOL_CALL with status 77",
    0,
    |call| {
        finish(call, RV_RESULT_TRUSTED, "done");
        ODD_STATUS
    }
);

failed_call_test!(
    a_step_that_finishes_and_returns_pending_fails_the_call_and_is_canceled,
    misuse_reason!("a step called finish and then returned RV_PENDING"),
    1,
    |call| {
        finish(call, RV_RESULT_TRUSTED, "done");
        RV_PENDING
    }
);

failed_call_test!(
    a_misuse_fails_the_call_even_when_a_good_result_follows,
    misuse_reason!("the result's kind is not one of the RV_RESULT constants"),
    0,
    |call| {
        finish(call, 6, "what kind is this");
        finish(call, RV_RESULT_TRUSTED, "done")
    }
);

failed_call_test!(
    a_misuse_in_a_step_that_returns_pending_fails_the_call_and_is_canceled,
    misuse_reason!("finish was handed a NULL result"),
    1,
    |call| {
        finish_as(call, step_args(call).ctx, ptr::null());
        RV_PENDING
    }
);

failed_call_test!(
    a_second_finish_is_misuse,
    misuse_reason!("finish was called twice"),
    0,
    |call| {
        finish(call, RV_RESULT_TRUSTED, "first");
        finish(call, RV_RESULT_TRUSTED, "second")
    }
);

failed_call_test!(
    a_finish_through_another_ctx_is_misuse,
    misuse_reason!("finish was handed a ctx other than the open RV_TOOL_CALL step's"),
    0,
    |call| {
        let done = result(RV_RESULT_TRUSTED, rv_str("done"));
        finish_as(call, ptr::null_mut(), &raw const done);
        RV_OK
    }
);

failed_call_test!(
    a_misaligned_result_is_misuse,
    misuse_reason!("finish was handed a misaligned result"),
    0,
    |call| {
        finish_as(call, step_args(call).ctx, ptr::without_provenance(1));
        RV_OK
    }
);

failed_call_test!(
    a_short_result_size_is_misuse,
    misuse_reason!("the result's size is shorter than RvToolResult"),
    0,
    |call| {
        finish_result(
            call,
            &RvToolResult {
                size: 23,
                ..result(RV_RESULT_TRUSTED, rv_str("done"))
            },
        );
        RV_OK
    }
);

failed_call_test!(
    a_result_text_that_is_not_utf8_is_misuse,
    misuse_reason!("the result's text is not UTF-8"),
    0,
    |call| {
        finish_result(
            call,
            &result(RV_RESULT_TRUSTED, rv_str_bytes(&[0xff, 0xfe])),
        );
        RV_OK
    }
);

failed_call_test!(
    a_result_text_with_a_null_pointer_and_a_length_is_misuse,
    misuse_reason!("the result's text has a NULL pointer and a nonzero length"),
    0,
    |call| {
        finish_result(
            call,
            &result(
                RV_RESULT_TRUSTED,
                RvStr {
                    ptr: ptr::null(),
                    len: 4,
                },
            ),
        );
        RV_OK
    }
);

failed_call_test!(
    a_result_text_over_16_mebibytes_is_misuse,
    misuse_reason!("the result's text is longer than 16777216 bytes"),
    0,
    |call| {
        // The length is refused before any byte is read, so the text
        // behind it need not be that long.
        finish_result(
            call,
            &result(
                RV_RESULT_TRUSTED,
                RvStr {
                    ptr: "x".as_ptr().cast(),
                    len: 16_777_217,
                },
            ),
        );
        RV_OK
    }
);

failed_call_test!(
    a_load_function_called_during_a_step_is_misuse,
    misuse_reason!("a load function was called outside RV_PLUGIN_LOAD"),
    0,
    |call| {
        describe_with(step_args(call).ctx, "acme-url", "1.0.0");
        RV_OK
    }
);

#[test]
fn an_empty_result_text_is_accepted() {
    pack_fake!(
        plug,
        {},
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            add_tool(call, "tool")
        },
        step: |call| finish_result(
            call,
            &result(
                RV_RESULT_TRUSTED,
                RvStr {
                    ptr: ptr::null(),
                    len: 0,
                },
            ),
        )
    );
    let pack = made(plug::entry);
    let output = call_tool(&*pack, "tool", json!({})).expect("the call succeeds");
    assert_eq!(output.text(), "");
}

#[test]
fn a_call_polled_after_it_ended_fails_and_sends_no_step() {
    pack_fake!(
        plug,
        {
            pub(super) static STEPS: AtomicUsize = AtomicUsize::new(0);
        },
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            add_tool(call, "tool")
        },
        step: |call| {
            STEPS.fetch_add(1, Ordering::SeqCst);
            finish(call, RV_RESULT_TRUSTED, "done")
        }
    );
    let pack = made(plug::entry);
    let call = test_call("tool");
    let mut future = pin!(pack.call(call.context(), json!({})));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_ready());
    let Poll::Ready(Err(failure)) = future.as_mut().poll(&mut cx) else {
        panic!("a second poll after the end fails");
    };
    assert_eq!(failure.message, "the tool call was polled after it ended");
    assert_eq!(plug::STEPS.load(Ordering::SeqCst), 1);
}

#[test]
fn a_late_finish_through_a_stashed_ctx_is_misuse() {
    pack_fake!(
        plug,
        {
            pub(super) static STEP_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
        },
        new: |call| {
            set_pack_run(call, instance(PACK_TAG));
            add_tool(call, "tool")
        },
        step: |call| {
            STEP_CTX.store(step_args(call).ctx, Ordering::SeqCst);
            finish(call, RV_RESULT_TRUSTED, "done")
        }
    );
    let pack = made(plug::entry);
    call_tool(&*pack, "tool", json!({})).expect("the call succeeds");
    let stashed = plug::STEP_CTX.load(Ordering::SeqCst);
    let late = result(RV_RESULT_TRUSTED, rv_str("late"));
    // SAFETY: the loader's function compares a `ctx` with the open call's
    // before it reads through it, and no call is open on this thread.
    let status = unsafe { crate::call::finish(stashed, &raw const late) };
    assert_eq!(status, RV_ERR_MISUSE);
}
