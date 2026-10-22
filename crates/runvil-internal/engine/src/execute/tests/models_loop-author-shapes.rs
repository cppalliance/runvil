//! Regression guards for the list shapes the shipped prompts use, run
//! against a canned model: the `prompts/research-person.md` section and
//! the shipped chat agent's turn loop.

use runvil_parser::test_support::{PromptSource, one_section};

use super::*;
use crate::model::{ModelBinding, ModelId, ModelInvocation};

#[test]
fn the_research_person_shape_returns_the_last_reply() {
    let gateway = ScriptedChat::new(vec![text("a factual summary")]);
    let source = PromptSource::new().prose(
        "Research",
        "
        Summarize the person.

        ```lua
        messages:user(prose)
        models.loop()
        return messages[#messages].content
        ```",
    );
    let trace = loop_scenario(source.build()).chat(&gateway).run();
    assert_eq!(trace.text(), "a factual summary");
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 1);
    assert!(
        bodies[0].messages[0]
            .content()
            .contains("Summarize the person."),
        "the section prose is the user message: {bodies:?}"
    );
}

/// The shipped chat agent's compactor: it keeps the system block and the
/// last two user turns, and summarizes the rest into one user record from
/// a capped excerpt of each record, cut on a character boundary, newest
/// first within a total budget.
/// With no record before the second-to-last user turn, it fails instead.
const COMPACT: &str = r#"local function compact(reason)
  local first = 1
  while messages[first] and messages[first].role == "system" do
    first = first + 1
  end
  local cut, turns = first, 0
  for i = #messages, first, -1 do
    if messages[i].role == "user" then
      turns = turns + 1
      if turns == 2 then cut = i break end
    end
  end
  if cut == first then return compactors.fail(reason) end
  local function excerpt(value)
    local text = json.encode(value)
    if #text <= 1500 then return text end
    -- Cut before the character holding byte 1501, so no UTF-8 sequence splits.
    local stop = 1501
    while text:byte(stop) >= 0x80 and text:byte(stop) < 0xC0 do
      stop = stop - 1
    end
    return text:sub(1, stop - 1)
  end
  local lines, room = {}, 48000
  for i = cut - 1, first, -1 do
    local line = messages[i].role .. ": " .. excerpt(messages[i].content)
    if #line > room then break end
    room = room - #line
    table.insert(lines, 1, line)
  end
  local summary = models.infer("Summarize this conversation:\n" .. table.concat(lines, "\n"))
  messages:replace(first, cut - 1, {
    role = "user",
    content = "Summary of the earlier conversation: " .. summary,
    meta = { summary = true },
  })
end
"#;

/// The shipped chat agent's turn loop over `inputs`, a Lua list of user
/// texts: each turn appends its text and runs `models.loop(compact)`
/// under the agent's `pcall`.
fn chat_turns(inputs: &str) -> String {
    format!(
        "{COMPACT}
         for _, text in ipairs({inputs}) do
           messages:user(text)
           pcall(function() return models.loop(compact) end)
         end"
    )
}

#[test]
fn the_chat_agent_shape_completes_both_turns() {
    let gateway = ScriptedChat::new(vec![text("first reply"), text("second reply")]);
    let source = one_section(&format!(
        "{}
         return #messages .. '|' .. messages[2].content .. '|' .. messages[4].content",
        chat_turns("{ 'first question', 'second question' }")
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "4|first reply|second reply");
    let bodies = gateway.requests();
    assert_eq!(bodies.len(), 2, "one request per turn");
    let second: Vec<&str> = bodies[1]
        .messages
        .iter()
        .map(crate::model::Message::content)
        .collect();
    assert_eq!(
        second,
        ["first question", "first reply", "second question"],
        "the second turn sends the history the first turn left"
    );
}

#[test]
fn the_chat_agent_shape_summarizes_older_turns_when_a_turn_overflows() {
    // Under the 4,096-token window, turn 3's whole history overflows the
    // precheck, so no request leaves for it; once the first turn is
    // summarized, the retry fits.
    let gateway = ScriptedChat::new(vec![text("a1"), text("a2"), text("summary"), text("a3")]);
    let source = one_section(&format!(
        "{}
         return messages[#messages].content .. '|' .. tostring(messages[1].meta and messages[1].meta.summary)",
        chat_turns("{ string.rep('x', 6000), 'q2', string.rep('y', 9000) }")
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "a3|true");
    assert_eq!(
        gateway.call_count(),
        4,
        "two turns, the summary, and the retried third turn"
    );
}

#[test]
fn the_chat_agent_shape_cuts_a_multibyte_excerpt_on_a_character_boundary() {
    // The first turn's encoded content is a quote and 3,000 two-byte
    // characters, so a 1,500-byte cut falls inside a character; the
    // excerpt ends before it, at the quote and 749 whole characters.
    let gateway = ScriptedChat::new(vec![text("a1"), text("a2"), text("summary"), text("a3")]);
    let source = one_section(&format!(
        "{}
         return messages[#messages].content .. '|' .. tostring(messages[1].meta and messages[1].meta.summary)",
        chat_turns("{ string.rep('é', 3000), 'q2', string.rep('y', 12000) }")
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "a3|true");
    let bodies = gateway.requests();
    let prompts: Vec<&str> = bodies
        .iter()
        .flat_map(|body| &body.messages)
        .map(crate::model::Message::content)
        .filter(|content| content.starts_with("Summarize this conversation:"))
        .collect();
    let [prompt] = prompts[..] else {
        panic!("expected one summary request, got {}", prompts.len());
    };
    assert!(
        prompt.contains(&format!("user: \"{}\nassistant: ", "é".repeat(749))),
        "the excerpt keeps whole characters up to the cap: {prompt:.80}"
    );
}

#[test]
fn the_chat_agent_shape_fails_an_overflowing_first_turn() {
    // With one user turn there is nothing older to summarize, so the turn
    // fails rather than summarizing the question it is answering.
    let gateway = ScriptedChat::new(vec![text("unreachable")]);
    let source = one_section(&format!(
        "{}
         return #messages .. '|' .. #messages[1].content",
        chat_turns("{ string.rep('x', 20000) }")
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "1|20000",
        "the history keeps only the question"
    );
    assert_eq!(
        gateway.call_count(),
        0,
        "the precheck refuses the turn and no summary is requested"
    );
}

#[test]
fn the_chat_agent_shape_fails_an_overflowing_second_turn() {
    let gateway = ScriptedChat::new(vec![text("a1"), text("unreachable")]);
    let source = one_section(&format!(
        "{}
         return #messages .. '|' .. messages[1].content .. '|' .. messages[2].content .. '|' .. #messages[3].content",
        chat_turns("{ 'q1', string.rep('y', 20000) }")
    ));
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "3|q1|a1|20000",
        "the failed turn leaves the history as it was"
    );
    assert_eq!(
        gateway.call_count(),
        1,
        "only the first turn's request left, and no summary was requested"
    );
}

#[test]
fn the_chat_agent_shape_leaves_the_oldest_records_out_of_a_summary_past_its_budget() {
    // Each long turn's user line in the summary prompt is 1,506 characters
    // and its reply's 15. When the short last turn is rejected, the 31
    // newest pairs before the kept turns leave too little of the
    // 48,000-character budget for the oldest user record.
    let long_turns = 33;
    let mut replies = vec![text("ok"); long_turns];
    replies.extend([
        fail(
            crate::model::CompletionErrorKind::ContextOverflow,
            400,
            "This model's maximum context length is 131072 tokens.",
        ),
        text("summary"),
        text("answer"),
    ]);
    let gateway = ScriptedChat::new(replies);
    let inputs: Vec<String> = (1..=long_turns)
        .map(|turn| format!("'old{turn:02}' .. string.rep('x', 1500)"))
        .chain(["'last'".to_owned()])
        .collect();
    let source = one_section(&format!(
        "{}
         return messages[#messages].content .. '|' .. tostring(messages[1].meta and messages[1].meta.summary)",
        chat_turns(&format!("{{ {} }}", inputs.join(", ")))
    ));
    let wide = ModelSet::from_parts(
        vec![ModelBinding::new(
            "writer",
            "A general model for tests",
            ModelId::gateway("test-model").expect("the test model id is valid"),
            ModelInvocation::new(),
            NonZeroU32::new(131_072).expect("the window is non-zero"),
        )],
        Some("writer".to_owned()),
    );
    let trace = loop_scenario(source).models(wide).chat(&gateway).run();
    assert_eq!(trace.text(), "answer|true");
    let bodies = gateway.requests();
    let prompts: Vec<&str> = bodies
        .iter()
        .flat_map(|body| &body.messages)
        .map(crate::model::Message::content)
        .filter(|content| content.starts_with("Summarize this conversation:"))
        .collect();
    let [prompt] = prompts[..] else {
        panic!("expected one summary request, got {}", prompts.len());
    };
    assert!(
        !prompt.contains("old01"),
        "the oldest user record is past the budget"
    );
    assert!(
        prompt.contains("old02"),
        "the newer records fill the budget first"
    );
}
