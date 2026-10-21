//! Tests for the `models.loop` adapter over a fresh VM: Lua's type names,
//! both entries' argument checks, the envelope failure, the chat answer's
//! userdata, and each step's action.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::{Function, Lua, MultiValue, Value};
use runvil_model_client::model::{ModelBinding, ModelId, ModelInvocation};
use runvil_types::metrics::ToolCallEvent;
use runvil_types::tools::ToolId;
use serde_json::json;

use super::machine::{Input, Phase, Then};
use super::{LOOP_RUNNING, act, envelope_failure, loop_begin, lua_type_name, read_input};
use crate::compactors::OverflowReason;
use crate::error::Error;
use crate::error_value::{ErrorField, ErrorKind, Raised, error_table, raised_from};
use crate::hardening::InstructionBudget;
use crate::messages::MessageList;
use crate::models::LuaModelHandle;
use crate::protocol::{Answer, ChatOutcome, ChatResult, LocalToolOutcome, Request, YieldParse};

/// A requested call named `echo`, its arguments naming its id.
pub(super) fn echo_call(id: &str) -> ToolCallEvent {
    ToolCallEvent::new(id, "echo", json!({ "id": id }))
}

/// A served round with no product, under turn 7.
pub(super) fn round() -> ChatResult {
    ChatResult {
        outcome: ChatOutcome::Empty(None),
        finish_reason: None,
        turn: 7,
    }
}

/// A model handle's userdata over a stub binding.
fn handle(lua: &Lua) -> Value {
    let binding = ModelBinding::new(
        "fast",
        "a test model",
        ModelId::gateway("m1").expect("a static model id is valid"),
        ModelInvocation::new(),
        std::num::NonZeroU32::MIN,
    );
    Value::UserData(
        lua.create_userdata(LuaModelHandle::from_binding(binding))
            .unwrap(),
    )
}

#[test]
fn lua_type_name_is_luas_type_with_an_integer_named_integer() {
    let lua = Lua::new();
    let lua_type: Function = lua.load("return type").eval().unwrap();
    let values = [
        Value::Nil,
        Value::Boolean(true),
        Value::Integer(1),
        Value::Number(1.5),
        Value::String(lua.create_string("s").unwrap()),
        Value::Table(lua.create_table().unwrap()),
        Value::Function(lua_type.clone()),
        Value::Thread(lua.create_thread(lua_type.clone()).unwrap()),
        handle(&lua),
        Value::NULL,
        Value::Error(Box::new(mlua::Error::runtime("boom"))),
    ];
    for value in values {
        let expected = match value {
            Value::Integer(_) => "integer".to_owned(),
            _ => lua_type.call::<String>(value.clone()).unwrap(),
        };
        assert_eq!(lua_type_name(&value), expected, "{value:?}");
    }
}

/// Installs the `_G` guard on `lua` and an empty list in its `messages`
/// slot, as a section VM's injection does, and returns that list.
fn install_section_list(lua: &Lua) -> MessageList {
    crate::globals::install(lua).unwrap();
    let list = MessageList::default();
    crate::messages::install_messages(lua, &list).unwrap();
    list
}

/// `loop_begin` over a new flag on `lua`, capped at `max_rounds`, with an
/// empty `compactors` table, and that flag. The guard and the section's
/// list must already be installed.
fn begin_with_flag(lua: &Lua, max_rounds: usize) -> (Function, Arc<AtomicBool>) {
    let running = Arc::new(AtomicBool::new(false));
    let compactors = lua.create_table().unwrap();
    let budget = InstructionBudget::default();
    let begin = loop_begin(lua, max_rounds, compactors, &running, &budget).unwrap();
    (begin, running)
}

#[test]
fn both_entries_check_their_arguments_in_order_with_the_shims_texts() {
    let lua = Lua::new();
    install_section_list(&lua);
    let (begin, _) = begin_with_flag(&lua, 2);
    let outcomes: Vec<String> = lua
        .load(
            "local begin, h, null = ...
             local function probe(...)
               local step, tag, value = begin(...)
               if step then step(false, 'ended') end
               return type(step) .. '|' .. tag .. '|' .. tostring(value.op or value)
             end
             return {
               probe(false, h), probe(false, h, nil), probe(false, nil, nil),
               probe(false, 5, nil), probe(false, 5), probe(false, null), probe(false),
               probe(false, nil), probe(true), probe(true, 5), probe(true, h, nil, nil),
               probe(true, h, 1.5, nil), probe(true, h, 1.5), probe(true, h),
               probe(true, h, nil),
             }",
        )
        .call((begin, handle(&lua), Value::NULL))
        .unwrap();
    let handle_first = "nil|raise|models.loop takes (compactor?); call handle:loop(compactor?) \
                        to run on a model handle";
    let loop_arity = "nil|raise|models.loop takes (compactor?)";
    let no_receiver = "nil|raise|call loop on a model handle with a colon: handle:loop(compactor?)";
    let method_arity = "nil|raise|handle:loop takes (compactor?)";
    let started = "function|yield|chat";
    assert_eq!(
        outcomes,
        [
            handle_first,
            handle_first,
            loop_arity,
            loop_arity,
            "nil|raise|compactor must be a function, got integer",
            "nil|raise|compactor must be a function, got userdata",
            started,
            started,
            no_receiver,
            no_receiver,
            method_arity,
            method_arity,
            "nil|raise|compactor must be a function, got number",
            started,
            started,
        ]
    );
}

#[test]
fn every_started_loop_yields_the_sections_one_list() {
    let lua = Lua::new();
    let list = install_section_list(&lua);
    let (begin, _) = begin_with_flag(&lua, 2);
    lua.load("messages:user('u')").exec().unwrap();
    let (sent, served): (Value, Value) = lua
        .load(
            "local begin, h = ...
             local function messages_sent(...)
               local step, _, request = begin(...)
               step(false, 'ended')
               return request.messages
             end
             return messages_sent(false), messages_sent(true, h)",
        )
        .call((begin, handle(&lua)))
        .unwrap();
    let global: Value = lua.globals().get("messages").unwrap();
    assert_eq!(sent, global);
    assert_eq!(served, global);
    assert_eq!(list.to_records(), vec![crate::tests::user("u")]);
}

#[test]
fn a_running_loop_refuses_another_until_its_step_ends_or_fails() {
    let lua = Lua::new();
    install_section_list(&lua);
    let (begin, running) = begin_with_flag(&lua, 2);
    let capped = loop_begin(
        &lua,
        0,
        lua.create_table().unwrap(),
        &running,
        &InstructionBudget::default(),
    )
    .unwrap();
    let flag = Arc::clone(&running);
    let is_running = lua
        .create_function(move |_, ()| Ok(flag.load(Ordering::Relaxed)))
        .unwrap();
    let seen: String = lua
        .load(
            "local begin, capped, running, h = ...
             local seen = {}
             local function note(label) seen[#seen + 1] = label .. '=' .. tostring(running()) end
             local function refusal(...)
               local refused, tag, err = begin(...)
               return type(refused) .. '|' .. tag .. '|' .. tostring(err)
             end
             note('idle')
             local step = begin(false)
             note('started')
             note('loop ' .. refusal(false))
             note('method ' .. refusal(true, h))
             note(step(false, 'ended'))
             local _, exhausted = capped(false)
             note('capped ' .. exhausted)
             local failing = begin(false)
             note('restarted')
             note(tostring(pcall(failing, true, 'not a round')))
             return table.concat(seen, ',')",
        )
        .call((begin, capped, is_running, handle(&lua)))
        .unwrap();
    let refused = format!("nil|raise|{LOOP_RUNNING}=true");
    assert_eq!(
        seen,
        format!(
            "idle=false,started=true,loop {refused},method {refused},raise=false,\
             capped raise=false,restarted=true,false=false"
        )
    );
}

/// Raises the heap limit from the live heap until the overflow's step
/// succeeds. A step that fails on its own clears `loop_running`, and must
/// clear `compacting` too. A compactor return the step takes as
/// `(true, value)` is read only after an overflow was judged, so it shows
/// the failure came after the machine set `compacting`.
#[test]
fn a_step_failing_on_a_full_heap_after_an_overflow_leaves_no_flag_set() {
    let lua = Lua::new();
    let list = install_section_list(&lua);
    lua.load("messages:user('u')").exec().unwrap();
    let (begin, running) = begin_with_flag(&lua, 2);
    let mut judged = false;
    for headroom in (0..64 * 1024).step_by(8) {
        let (step, _, _): (Function, Value, Value) = begin.call(false).unwrap();
        let overflowed = ChatResult {
            outcome: ChatOutcome::Overflow(OverflowReason::Precheck),
            ..round()
        };
        let (envelope, _) = Answer::<Error>::Chat(Ok(Box::new(overflowed)))
            .into_envelope(&lua)
            .unwrap();
        lua.gc_collect().unwrap();
        lua.set_memory_limit(lua.used_memory() + headroom).unwrap();
        let stepped = step.call::<MultiValue>(envelope);
        lua.set_memory_limit(0).unwrap();
        let failed = stepped.is_err() && !running.load(Ordering::Relaxed);
        if failed {
            assert!(
                !list.compacting(),
                "a step that failed with {headroom} bytes of headroom left the list compacting"
            );
        }
        let returned = step.call::<MultiValue>((true, "returned")).is_ok();
        judged |= failed && returned;
        assert!(!running.load(Ordering::Relaxed) && !list.compacting());
        if stepped.is_ok() {
            break;
        }
    }
    assert!(
        judged,
        "no headroom failed the step after it judged the overflow"
    );
}

#[test]
fn envelope_failure_keeps_an_error_table_and_wraps_anything_else() {
    let lua = Lua::new();
    let table = error_table(&lua, &Error::Interrupted).unwrap();
    let kept = envelope_failure(&lua, Value::Table(table.clone())).unwrap();
    assert_eq!(kept.to_pointer(), table.to_pointer());
    let boom = Value::String(lua.create_string("boom").unwrap());
    let wrapped = envelope_failure(&lua, boom).unwrap();
    let raised = raised_from(&lua, &wrapped)
        .unwrap()
        .expect("an error table");
    assert_eq!(
        (raised.kind, raised.message.as_str()),
        (ErrorKind::Lua, "boom")
    );
}

#[test]
fn a_chat_answer_resumes_as_a_chat_result_userdata_the_step_takes_whole() {
    let lua = Lua::new();
    let served = ChatResult {
        outcome: ChatOutcome::ToolCalls(vec![
            echo_call("c1").with_tool(Some(ToolId::parse("web/fetch").unwrap())),
        ]),
        finish_reason: Some("tool_calls".to_owned()),
        ..round()
    };
    let (envelope, retained) = Answer::<Error>::Chat(Ok(Box::new(served.clone())))
        .into_envelope(&lua)
        .unwrap();
    assert!(retained.is_none());
    let values = envelope.into_vec();
    let Some(Value::UserData(resumed)) = values.get(1).cloned() else {
        panic!("the chat answer resumes as a userdata, got {values:?}");
    };
    assert!(resumed.is::<ChatResult>());
    let budget = InstructionBudget::default();
    let values = MultiValue::from_vec(values);
    let input = read_input(&lua, &Phase::Chatting, values, &budget).unwrap();
    let Input::Answered(Ok(taken)) = input else {
        panic!("the step reads the chat answer, got {input:?}");
    };
    assert_eq!(*taken, served);
    assert!(
        resumed.borrow::<ChatResult>().is_err(),
        "the step takes the result out of the userdata"
    );
}

/// `act`'s tag and values for `then`.
fn action(lua: &Lua, then: Then<Value>) -> (String, Vec<Value>) {
    let mut values = act(lua, then).unwrap().into_vec().into_iter();
    let tag: String = lua
        .unpack(values.next().expect("act returns its tag first"))
        .unwrap();
    (tag, values.collect())
}

/// The request `act` yields for `then`, as the protocol parses it.
fn parsed(lua: &Lua, then: Then<Value>) -> Request {
    let (tag, values) = action(lua, then);
    assert_eq!((tag.as_str(), values.len()), ("yield", 1));
    match Request::from_yield(lua, &values[0]) {
        YieldParse::Request(request) => request,
        other => panic!("the yield parses, got {other:?}"),
    }
}

#[test]
fn act_yields_each_request_as_the_protocol_parses_it() {
    let lua = Lua::new();
    let tool_call = Then::ToolCall {
        call: echo_call("c1"),
        turn: 7,
    };
    let Request::ToolCall {
        alias,
        args,
        call_id,
        turn,
    } = parsed(&lua, tool_call)
    else {
        panic!("a tool_call request");
    };
    assert_eq!(
        (alias.as_str(), call_id.as_deref(), turn),
        ("echo", Some("c1"), Some(7))
    );
    assert_eq!(args, json!({ "id": "c1" }));
    let returned = Value::String(lua.create_string("text").unwrap());
    let outcome = |then| match parsed(&lua, then) {
        Request::LocalToolDone { outcome } => outcome,
        other => panic!("a local_tool_done request, got {other:?}"),
    };
    let done = outcome(Then::Report(Some(returned)));
    assert!(matches!(done, LocalToolOutcome::Returned(text) if text == "text"));
    assert!(matches!(
        outcome(Then::Report(None)),
        LocalToolOutcome::Raised
    ));
    let list = Value::UserData(lua.create_userdata(MessageList::default()).unwrap());
    let receiver = handle(&lua);
    for (handle, expected) in [(Some(receiver.clone()), receiver), (None, Value::Nil)] {
        let messages = list.clone();
        let (tag, values) = action(&lua, Then::Chat { messages, handle });
        let Value::Table(chat) = &values[0] else {
            panic!("a request table");
        };
        assert_eq!(tag, "yield");
        assert_eq!(chat.raw_get::<String>("op").unwrap(), "chat");
        assert_eq!(chat.raw_get::<Value>("messages").unwrap(), list);
        assert_eq!(chat.raw_get::<Value>("handle").unwrap(), expected);
    }
}

#[test]
fn act_names_each_call_return_and_raise_with_its_values() {
    let lua = Lua::new();
    let value = |text: &str| Value::String(lua.create_string(text).unwrap());
    let tagged = |tag: &str, values: Vec<Value>| (tag.to_owned(), values);
    let compact = Then::Compact {
        compactor: value("f"),
        reason: OverflowReason::Precheck,
    };
    assert_eq!(
        action(&lua, compact),
        tagged("compactor", vec![value("f"), value("precheck")])
    );
    let handler = Then::Handle {
        handler: value("h"),
        args: value("a"),
    };
    assert_eq!(
        action(&lua, handler),
        tagged("handler", vec![value("h"), value("a")])
    );
    assert_eq!(action(&lua, Then::Return), tagged("return", Vec::new()));
    assert_eq!(
        action(&lua, Then::Raise(value("x"))),
        tagged("raise", vec![value("x")])
    );
    let plain = Then::RaiseNormalized(value("plain"));
    assert_eq!(action(&lua, plain), tagged("raise", vec![value("plain")]));
    let typed = mlua::Error::external(Error::ContextExhausted {
        reason: "precheck".to_owned(),
    });
    let new = Raised {
        kind: ErrorKind::EmptyModelReply,
        message: "empty".to_owned(),
        fields: [(
            "finish_reason".to_owned(),
            ErrorField::String("stop".to_owned()),
        )]
        .into(),
    };
    let raised = [
        (
            Then::RaiseNormalized(Value::Error(Box::new(typed))),
            "context_exhausted",
            "precheck",
        ),
        (Then::RaiseNew(new), "empty_model_reply", "stop"),
    ];
    for (then, kind, field) in raised {
        let (tag, values) = action(&lua, then);
        let Value::Table(table) = &values[0] else {
            panic!("an error table");
        };
        let read = |name| table.raw_get::<String>(name).unwrap();
        let field_name = if kind == "context_exhausted" {
            "reason"
        } else {
            "finish_reason"
        };
        assert_eq!((tag.as_str(), read("kind").as_str()), ("raise", kind));
        assert_eq!(read(field_name), field);
    }
}
