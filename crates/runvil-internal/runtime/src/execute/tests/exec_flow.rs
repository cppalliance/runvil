//! Tests for section walk control flow: `call`, `jump`, `fanout`, the `prompt` reads, and `var`.

use runvil_prompt::test_support::PromptSource;

use super::context::arm_fanout;
use super::*;

/// Alternating lua/prose blocks run in order; each Lua block that wants the
/// model reads its own pending prose buffer into an explicit infer, and the
/// run result is the final Lua return.
#[test]
fn section_with_alternating_blocks_executes_in_order() {
    let gateway = ScriptedChat::new(vec![text("reply-1"), text("reply-2")]);
    let source = PromptSource::new().prose(
        "Only",
        r"
        ```lua
        store.append('order.txt', 'lua1\n')
        ```

        First ask.

        ```lua
        store.append('order.txt', 'lua2:' .. models.infer(prose) .. '\n')
        ```

        Final ask.

        ```lua
        store.append('order.txt', 'lua3\n')
        return models.infer(prose)
        ```",
    );
    let trace = Scenario::new(source.build()).bound().chat(&gateway).run();

    assert_eq!(trace.text(), "reply-2");
    assert_eq!(gateway.call_count(), 2);
    assert_eq!(
        trace.read("order.txt").expect("order log"),
        "lua1\nlua2:reply-1\nlua3\n"
    );
}

#[test]
fn call_runs_named_section_as_subroutine() {
    let gateway = ScriptedChat::new(vec![text("research-reply")]);

    // The subroutine sits after its run-ending caller: a contained chain
    // falls through like any walk, so a subroutine placed before later
    // sections would pull them into the chain.
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local by_name = call('../Research')
            assert(by_name == 'research-reply')
            assert(store.read('evidence.md') == 'research-reply')
            return by_name",
        )
        .prose(
            "Research",
            "
            Research {{ args }}.

            ```lua
            local answer = models.infer(prose)
            store.write('evidence.md', answer)
            return answer
            ```",
        );
    let trace = Scenario::new(source.build())
        .args("topic")
        .bound()
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "research-reply");
}

/// The `with_args` fork scopes a call's input over its whole chain:
/// a no-input `call` nested inside the chain defaults to the chain's
/// args, not the run's.
#[test]
fn nested_call_without_input_inherits_the_chains_args() {
    let gateway = ScriptedChat::new(vec![text("inner-reply")]);
    let source = PromptSource::new()
        .section(
            "Main",
            "
            local r = call('../Sub', 'chain-args')
            assert(r == 'inner-reply')
            return r",
        )
        .section("Sub", "return call('../Inner')")
        .prose(
            "Inner",
            "
            Args: {{ args }}

            ```lua
            return models.infer(prose)
            ```",
        );
    let trace = Scenario::new(source.build())
        .args("run-args")
        .bound()
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "inner-reply");
    let body = gateway
        .last_request()
        .expect("the inner section's infer must reach the gateway");
    let text = body.messages_json().to_string();
    assert!(
        text.contains("chain-args"),
        "the nested no-input call substitutes the chain's args: {text}"
    );
    assert!(
        !text.contains("run-args"),
        "the run's args must not leak into the chain: {text}"
    );
}

/// [`arm_fanout`] with a `### Worker` whose Lua is `worker`.
fn default_arm(worker: &str) -> String {
    arm_fanout().section("### Worker", worker).build()
}

/// The worker Lua that asks the model through the `default` handle.
const HANDLE_INFER: &str = "return models.infer(models.get('default'), 'ping about ' .. item)";

/// `models.infer(models.get('default'), ...)` works inside an arm, so a worker's Lua can call the model
/// directly.
#[test]
fn fanout_arm_model_infer_works_inside_an_arm() {
    let gateway = ScriptedChat::new(vec![text("pong")]);
    let trace = Scenario::new(default_arm(HANDLE_INFER))
        .bound()
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "pong");

    let body = gateway
        .last_request()
        .expect("infer must reach the gateway");
    assert_eq!(body.options.model(), "claude-sonnet-4-6");
    let messages = body.messages_json();
    let content = messages
        .as_array()
        .expect("messages array")
        .last()
        .expect("a user turn")["content"]
        .as_str()
        .expect("content string");
    assert!(
        content.contains("ping about alpha"),
        "the arm's item must reach the infer prompt: {content}"
    );
}

/// `models.infer` with a leading handle inside an arm handed no client surfaces the lazy-creation
/// error.
#[test]
fn fanout_arm_model_infer_without_a_client_surfaces_the_disabled_gateway() {
    // A scenario without a model answers every `Chat` with the disabled
    // gateway's failure: the round fails with that error and nothing
    // reaches the network, however the process environment is configured.
    let trace = Scenario::new(default_arm(HANDLE_INFER)).bound().run();
    let rendered = trace.error().to_string();
    assert!(
        rendered.contains("model access is turned off or not configured"),
        "an arm's models.infer must surface the disabled-gateway completion error: {rendered}"
    );
}

/// An unknown model alias errors loudly inside an arm, same as on the walk.
#[test]
fn fanout_arm_model_infer_with_an_unknown_alias_errors_loudly() {
    let trace = Scenario::new(default_arm(
        "return models.infer(models.get('ghost'), 'ping')",
    ))
    .bound()
    .run();
    let rendered = trace.error().to_string();
    assert!(
        rendered.contains(
            "models.get label \"ghost\" is not a model role: expected default, strong, small, fast or creative"
        ),
        "the unknown label must be named: {rendered}"
    );
}

#[test]
fn advance_turn_saturates_and_never_wraps_the_stored_counter() {
    // The shared turn counter must saturate at u32::MAX rather than
    // wrapping through fetch_add and reusing a turn index.
    let turns = AtomicU32::new(0);
    assert_eq!(advance_turn(&turns), 1);
    assert_eq!(advance_turn(&turns), 2);
    assert_eq!(turns.load(Ordering::Relaxed), 2);

    // At the boundary, both the presented value and the stored value saturate.
    let maxed = AtomicU32::new(u32::MAX);
    assert_eq!(advance_turn(&maxed), u32::MAX);
    assert_eq!(
        maxed.load(Ordering::Relaxed),
        u32::MAX,
        "the stored counter must not wrap to zero"
    );

    let near = AtomicU32::new(u32::MAX - 1);
    assert_eq!(advance_turn(&near), u32::MAX);
    assert_eq!(advance_turn(&near), u32::MAX);
    assert_eq!(near.load(Ordering::Relaxed), u32::MAX);
}
