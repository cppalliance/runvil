//! The rules a compaction after an overflow must meet: the projection's
//! structural rules, the shrink rule, and the retried round's precheck.
//! Each broken rule raises its error and puts the records back.

use runvil_prompt::test_support::one_section;

use super::*;

/// The section every case runs: a system block, a user turn, one answered
/// tool-call batch, and a user record too large for the window, then a
/// loop whose compactor runs `body`. It returns the failure's kind and
/// message, the record count, and the large record's length.
fn overflowing_with(body: &str) -> String {
    one_section(&format!(
        "messages:system('be brief')
         messages:user('q1')
         messages:assistant('', {{ {{ id = 'c1', name = 'echo', arguments = {{ value = 'v' }} }} }})
         messages:tool('r1', 'c1')
         messages:user(string.rep('x', 100000))
         local ok, err = pcall(models.loop, function(reason)
           {body}
         end)
         assert(not ok, 'the compaction is refused')
         return table.concat({{ err.kind, tostring(err), #messages, #messages[5].content }}, '|')"
    ))
}

#[test]
fn each_broken_rule_raises_its_error_and_restores_the_records() {
    // A refusal of the compacted list is placed on the loop's call, prompt
    // line 17, and one the compactor's own `append` raises on that call's
    // line, 19.
    let exhausted = format!(
        "context_exhausted|prompt:17: {}",
        ErrorCode::ContextExhausted.prose()
    );
    let lua = |line: u32, what: &str| format!("lua|{}", refused(line, what));
    let cases = [
        (
            "an unanswered batch",
            "messages:replace(5, 5, { role = 'assistant', content = '', tool_calls = { \
             { id = 'c2', name = 'echo', arguments = { value = 'v' } } } })",
            lua(17, "messages[5] tool call \"c2\" has no tool result"),
        ),
        (
            "a split batch",
            "messages:replace(4, 3, { role = 'user', content = 'between' })",
            lua(17, "messages[3] tool call \"c1\" has no tool result"),
        ),
        (
            "a duplicate call id",
            "messages:replace(5, 5, { role = 'assistant', content = '', tool_calls = { \
             { id = 'c1', name = 'echo', arguments = { value = 'v' } } } }, \
             { role = 'tool', content = 'r2', tool_call_id = 'c1' })",
            lua(
                17,
                "messages[5] tool call id \"c1\" duplicates an earlier tool call",
            ),
        ),
        (
            "an orphan tool record",
            "messages:replace(3, 3)",
            lua(
                17,
                "messages[3] is an orphan tool record: no pending assistant tool call with id \
                 \"c1\"",
            ),
        ),
        (
            "a late system record",
            "messages:replace(5, 5, { role = 'user', content = 'short' })
             messages:append({ role = 'system', content = 'late' })",
            lua(
                19,
                "messages[6] is a system message after a non-system message; system messages \
                 lead the list, and only a compactor can change the leading block",
            ),
        ),
        (
            "a non-text composed system block",
            "messages:replace(1, 0, { role = 'system', content = { { type = 'text', text = 'rules' } } })",
            lua(
                17,
                "messages[1] is a system message with content parts; only plain text system \
                 messages can be composed",
            ),
        ),
        ("no shrink", "", exhausted.clone()),
        (
            "a retried precheck still over",
            "messages:replace(5, 5, { role = 'user', content = string.rep('x', 50000) })",
            exhausted,
        ),
    ];
    for (rule, body, refusal) in cases {
        let gateway = ScriptedChat::new(vec![text("unreachable")]);
        let trace = loop_scenario(overflowing_with(body)).chat(&gateway).run();
        assert_eq!(
            trace.text(),
            format!("{refusal}|5|100000"),
            "{rule} raises its error and restores the records"
        );
        assert_eq!(gateway.call_count(), 0, "{rule}: no request leaves");
    }
}
