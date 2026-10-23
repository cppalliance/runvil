//! Tests for the section's `messages` list and its parse through the
//! protocol.

use std::collections::BTreeMap;

use mlua::{Lua, LuaSerdeExt, Value};
use serde_json::json;

use super::MessageList;
use crate::protocol::{Request, YieldParse};
use crate::test_vm::TestVm;
use crate::tests::{system, user};
use crate::{Error, RenderedTools};

#[path = "messages-tests-list.rs"]
mod list;

#[path = "messages-tests-records.rs"]
mod records;

#[path = "messages-tests-view.rs"]
mod view;

/// Parses `messages` through the chat protocol boundary, as a
/// `models.loop` round's yield would.
fn chat_parse(lua: &Lua, messages: Value) -> YieldParse {
    let request: mlua::Table = lua
        .load("return { op = 'chat', messages = ... }")
        .call(messages)
        .expect("the chat request table builds around any messages value");
    Request::from_yield(lua, &Value::Table(request))
}

#[test]
fn messages_is_the_vms_one_empty_list_and_lua_reaches_no_other() {
    let vm = TestVm::new().build();
    let (kind, len, first_nil, same, new_nil): (String, i64, bool, bool, bool) = vm.eval(
        "local first = messages\n\
         return type(messages), #messages, messages[1] == nil, first == messages,\n\
           messages['new'] == nil",
    );
    assert_eq!((kind.as_str(), len), ("userdata", 0));
    assert!(first_nil && same && new_nil);
    vm.exec("messages:user('u')");
    assert_eq!(vm.messages().to_records(), vec![user("u")]);
}

/// A rendered tool list advertising each of `names` as an adhoc tool.
fn rendered(names: &[&str]) -> RenderedTools {
    RenderedTools {
        schemas: Vec::new(),
        dispatch: names
            .iter()
            .map(|name| ((*name).to_owned(), None))
            .collect(),
    }
}

#[test]
fn a_list_is_rendered_only_once_a_tool_list_is_attached_and_keeps_the_first() {
    let list = MessageList::default();
    assert!(!list.rendered(), "a fresh list holds no rendered tool list");
    assert!(list.rendered_tools().is_none());
    list.clone().attach_rendered_tools(rendered(&["first"]));
    assert!(list.rendered(), "a clone shares the attached list");
    list.attach_rendered_tools(rendered(&["second"]));
    let kept = list
        .rendered_tools()
        .expect("the list holds a rendered tool list");
    assert_eq!(
        kept.dispatch,
        BTreeMap::from([("first".to_owned(), None)]),
        "the first attached list stays"
    );
}

#[test]
fn a_chat_request_whose_messages_is_not_a_list_is_a_malformed_yield() {
    let vm = TestVm::new().build();
    vm.exec("messages:user('u')");
    for source in [
        "return { { role = 'system', content = 'be terse' }, { role = 'user', content = 'hi' } }",
        "return {}",
        "return 'messages'",
        "return nil",
        "return messages[1]",
    ] {
        let messages = vm.eval(source);
        assert!(
            matches!(chat_parse(vm.lua(), messages), YieldParse::Malformed(_)),
            "{source}"
        );
    }
}

#[test]
fn a_non_empty_list_parses_to_a_chat_request_holding_that_list() {
    let vm = TestVm::new().build();
    let lua = vm.lua();
    let built = vm.eval("messages:system('be terse'):user('hi') return messages");
    let list = match chat_parse(lua, built) {
        YieldParse::Request(Request::Chat { list, binding }) => {
            assert!(binding.is_none(), "a round without a handle has no binding");
            list
        }
        other => panic!("expected a chat request, got {other:?}"),
    };
    assert_eq!(list.to_records(), vm.messages().to_records());
    assert_eq!(list.to_records(), vec![system("be terse"), user("hi")]);
    vm.exec("messages:user('more')");
    assert_eq!(
        list.to_records().len(),
        3,
        "the request holds the section's list, not a copy"
    );
}

#[test]
fn a_list_or_a_record_view_is_refused_by_the_var_and_prose_guards() {
    let vm = TestVm::new().build();
    vm.install_lazy_prose(|state| {
        let refusals = ["messages", "record"].map(|name| match (state.globals)(name) {
            Err(Error::Lua(message)) => message,
            other => format!("{name} was not refused: {other:?}"),
        });
        Ok(refusals.join("|"))
    })
    .expect("the prose guard installs");
    let (writes, prose): (String, String) = vm.eval(
        "messages:user('u')\n\
         record = messages[1]\n\
         local out = {}\n\
         for _, write in ipairs({\n\
           function() var.list = messages end,\n\
           function() var.record = record end,\n\
         }) do\n\
           local ok, err = pcall(write)\n\
           out[#out + 1] = ok and 'stored' or tostring(err):match('var%.%a+ must be JSON data, got %a+')\n\
         end\n\
         return table.concat(out, '|'), prose",
    );
    assert_eq!(
        writes,
        "var.list must be JSON data, got userdata|var.record must be JSON data, got userdata"
    );
    assert_eq!(
        prose,
        "global `messages` is a userdata; bare globals in prose must be JSON data|\
         global `record` is a userdata; bare globals in prose must be JSON data"
    );
}

#[test]
fn the_list_runs_under_the_hardened_section_sandbox() {
    let vm = TestVm::new().build();
    let views: Value = vm.eval(
        "messages:system('s'):user('u')\n\
         return { messages[1], messages[2] }",
    );
    let json: serde_json::Value = vm
        .lua()
        .from_value(views)
        .expect("record views convert under the hardened sandbox");
    assert_eq!(
        json,
        json!([{ "role": "system", "content": "s" }, { "role": "user", "content": "u" }])
    );
}
