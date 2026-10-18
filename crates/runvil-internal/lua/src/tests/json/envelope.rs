//! Decoding the body of an `untrusted` envelope: the example chapter 13 of
//! the language guide quotes, its patterns included.

use super::*;

#[test]
fn a_body_after_a_blank_line_decodes_once_cut_from_the_envelope() {
    let vm = json_vm();
    let (key, ok): (String, bool) = vm.eval(
        r#"local r = untrusted('url: https://example.com/data.json\ntruncated: false\nextraction: plain\n\n{"key":"value","nested":{"ok":true}}')
local data = json.decode(r:match('\n\n(.*)\n</untrusted_input_%x+>$'))
return data.key, data.nested.ok"#,
    );
    assert_eq!((key.as_str(), ok), ("value", true));
}

#[test]
fn a_whole_body_decodes_once_cut_between_the_tags() {
    let vm = json_vm();
    let (query, count, marked): (String, i64, bool) = vm.eval(
        r#"local r = untrusted('{"query":"rust","results":[]}')
local data = json.decode(r:match('<untrusted_input_%x+>\n(.*)\n</untrusted_input_%x+>$'))
return data.query, #data.results, getmetatable(data.results) == false"#,
    );
    assert_eq!(
        (query.as_str(), count, marked),
        ("rust", 0, true),
        "`results` decodes to an empty marked list"
    );
}

#[test]
fn markup_in_a_body_decodes_with_every_less_than_sign_escaped() {
    let vm = json_vm();
    let html: String = vm.eval(
        r#"local r = untrusted('{"html":"<b>hi</b>"}')
return json.decode(r:match('<untrusted_input_%x+>\n(.*)\n</untrusted_input_%x+>$')).html"#,
    );
    assert_eq!(html, "&lt;b>hi&lt;/b>");
}
