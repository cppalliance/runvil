//! `markdown.parse`: the tables a text becomes, their shape, and the
//! arguments it refuses.

use super::*;

#[test]
fn a_document_parses_to_plain_tables_that_ordinary_indexing_reads() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("#md", "3"),
            ("md[1].kind", r#""frontmatter""#),
            ("md[1].data.title", r#""Notes""#),
            ("md[1].data.tags[2]", r#""b""#),
            ("md[2].text", r#""Preface paragraph.""#),
            ("md[2].line", "6"),
            ("md[4]", "nil"),
            ("md.kind", "nil"),
            ("guide.kind", r#""section""#),
            ("guide.level", "1"),
            ("guide.title", r#""Guide""#),
            ("guide.line", "8"),
            ("#guide.blocks", "4"),
            ("guide.blocks[1].text", r#""Intro text\non two lines.""#),
            ("guide.blocks[2].ordered", "false"),
            ("guide.blocks[2].items[1]", r#""alpha""#),
            ("#guide.blocks[2].items", "2"),
            ("templates.title", r#""Templates""#),
            ("templates.blocks[2].lang", r#""lua""#),
            ("templates.blocks[5].lang", r#""json""#),
            ("templates.blocks[5].info", r#""json strict""#),
            ("templates.blocks[5].text", r#""{ \"ok\": true }""#),
            ("templates.blocks[6].title", r#""Extras""#),
            (
                "templates.blocks[6].blocks[1]",
                r#"{"kind":"rule","line":36}"#,
            ),
            ("templates.blocks[2].title", "nil"),
            ("usage.blocks[1].ordered", "true"),
            ("usage.blocks[1].line", "44"),
        ],
    );
    let kinds: String = vm.eval(
        "local kinds = {}\n\
         for index, block in ipairs(md) do kinds[index] = block.kind end\n\
         return table.concat(kinds, ',')",
    );
    assert_eq!(kinds, "frontmatter,prose,section");
}

#[test]
fn a_parse_result_is_plain_data_that_json_encodes_and_decodes_whole() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            (
                "usage",
                r#"{"blocks":[{"items":["first","second"],"kind":"list","line":44,"ordered":true}],"kind":"section","level":2,"line":42,"title":"Usage"}"#,
            ),
            // The whole document survives a trip through JSON text.
            (
                "json.encode(json.decode(json.encode(md))) == json.encode(md)",
                "true",
            ),
            // No table has a metatable but the list marker, so none has
            // methods.
            ("getmetatable(guide)", "nil"),
            ("getmetatable(md) == getmetatable(json.array())", "true"),
        ],
    );
}

#[test]
fn every_list_is_marked_so_an_empty_one_encodes_as_an_array() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.parse('')", "[]"),
            ("markdown.parse('\\n\\n  \\n')", "[]"),
            ("markdown.parse('## A\\n')[1].blocks", "[]"),
            (
                "markdown.parse('---\\ntags: []\\nnone: ~\\n---\\n')[1].data",
                r#"{"none":null,"tags":[]}"#,
            ),
            (
                "markdown.parse('---\\nnone: ~\\n---\\n')[1].data.none == json.null",
                "true",
            ),
            ("markdown.parse('-\\n- b\\n')[1].items", r#"["","b"]"#),
        ],
    );
}

#[test]
fn text_is_read_raw_and_every_string_uses_newlines_only() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            // A placeholder comes back as written.
            (
                "markdown.parse('Hello {{ name }}')[1].text",
                r#""Hello {{ name }}""#,
            ),
            (
                "markdown.parse('# T\\r\\n\\r\\none\\r\\ntwo\\r\\n')[1].blocks[1].text",
                r#""one\ntwo""#,
            ),
            ("markdown.parse('\\239\\187\\191# T')[1].title", r#""T""#),
            // A new parse counts lines from 1.
            ("markdown.parse(templates.blocks[3].text)[1].line", "1"),
            (
                "markdown.parse(templates.blocks[3].text)[1].kind",
                r#""section""#,
            ),
            // An item's text parses again as Markdown.
            (
                "markdown.parse(markdown.parse('- a\\n  - a1\\n  - a2\\n')[1].items[1])[2].items",
                r#"["a1","a2"]"#,
            ),
        ],
    );
}

#[test]
fn parse_refuses_a_value_that_is_not_utf8_text_within_the_limit() {
    let vm = markdown_vm();
    assert_raises(
        &vm,
        &[
            (
                "markdown.parse(42)",
                "markdown.parse expects a string, got integer",
            ),
            (
                "markdown.parse()",
                "markdown.parse expects a string, got nil",
            ),
            (
                "markdown.parse({})",
                "markdown.parse expects a string, got table",
            ),
            (
                "markdown.parse('ok \\255')",
                "markdown.parse: text is not valid UTF-8",
            ),
            (
                "markdown.parse(string.rep('x', 8388609))",
                "markdown.parse: input exceeds the 8388608-byte limit",
            ),
        ],
    );
    assert_values(&vm, &[("#markdown.parse(string.rep('x', 8388608))", "1")]);
}
