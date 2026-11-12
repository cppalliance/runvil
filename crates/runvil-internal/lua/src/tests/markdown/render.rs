//! `markdown.render`: blocks written back as text, and the blocks it
//! refuses to write.

use super::*;

#[test]
fn render_writes_each_block_in_one_fixed_style() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.render({})", r#""""#),
            // A section's blocks render without its heading, and wrapping
            // the section in a list renders the heading too.
            ("markdown.render(usage.blocks)", r#""1. first\n2. second""#),
            (
                "markdown.render({ usage })",
                r###""## Usage\n\n1. first\n2. second""###,
            ),
            (
                "markdown.render({ md[1], md[2] })",
                r#""---\ntags:\n- a\n- b\ntitle: Notes\n---\n\nPreface paragraph.""#,
            ),
            (
                "markdown.render(markdown.section(md, '//Extras').blocks)",
                r#""***\n\n```lua\nreturn 2\n```""#,
            ),
            (
                "markdown.render({ { kind = 'fence', lang = 'md', info = '', text = '```\\nx\\n```' } })",
                r#""````md\n```\nx\n```\n````""#,
            ),
            (
                "markdown.render({ { kind = 'list', ordered = false, items = { 'a\\n- a1', '' } },\
                 { kind = 'list', ordered = false, items = { 'b' } } })",
                r#""- a\n  - a1\n-\n\n* b""#,
            ),
            (
                "markdown.render({ { kind = 'frontmatter', data = { b = json.null, a = { 1, 2 } } } })",
                r#""---\na:\n- 1\n- 2\nb: null\n---""#,
            ),
            // A block's `line` is not read.
            (
                "markdown.render({ { kind = 'rule', line = 'nowhere' }, { kind = 'rule' } })",
                r#""***\n\n---""#,
            ),
        ],
    );
}

#[test]
fn parsed_text_is_written_as_text_that_parses_to_the_same_blocks() {
    let vm = markdown_vm();
    vm.exec(
        "function without_lines(blocks)\n\
           local copy = json.decode(json.encode(blocks))\n\
           local function clear(list)\n\
             for _, block in ipairs(list) do\n\
               block.line = nil\n\
               if block.blocks then clear(block.blocks) end\n\
             end\n\
           end\n\
           clear(copy)\n\
           return json.encode(copy)\n\
         end",
    );
    for document in ["md", "log", "items", "nested", "guide.blocks"] {
        assert_values(
            &vm,
            &[(
                &format!(
                    "without_lines(markdown.parse(markdown.render({document}))) == \
                     without_lines({document})"
                ),
                "true",
            )],
        );
    }
}

#[test]
fn an_edit_made_through_a_lookup_is_written() {
    let vm = markdown_vm();
    vm.exec(
        "local list = markdown.section(md, '/Guide/Usage', 'list')\n\
         list.items[#list.items + 1] = 'third'\n\
         markdown.section(md, '/Guide/Usage').title = 'How to use it'",
    );
    assert_values(
        &vm,
        &[(
            "markdown.render({ guide.blocks[4] })",
            r###""## How to use it\n\n1. first\n2. second\n3. third""###,
        )],
    );
}

/// Asserts that `markdown.render` refuses each list of blocks with the
/// message beside it.
#[track_caller]
fn assert_refused(cases: &[(&str, &str)]) {
    let vm = markdown_vm();
    for (blocks, message) in cases {
        assert_raises(
            &vm,
            &[(
                &format!("markdown.render({blocks})"),
                &format!("markdown.render: {message}"),
            )],
        );
    }
}

#[test]
fn render_refuses_a_field_that_is_missing_or_of_the_wrong_type_and_names_it() {
    assert_refused(&[
        (
            "{ { kind = 'prose' } }",
            "blocks[1].text must be a string, got nil",
        ),
        (
            "{ md[2], { kind = 'prose', text = 5 } }",
            "blocks[2].text must be a string, got integer",
        ),
        (
            "{ { text = 'x' } }",
            "blocks[1].kind must be a string, got nil",
        ),
        (
            "{ { kind = 'code', text = 'x' } }",
            "blocks[1].kind must be one of frontmatter, section, prose, fence, list, rule, got \
             \"code\"",
        ),
        (
            "{ { kind = 'section', level = 7, title = 'A', blocks = {} } }",
            "blocks[1].level must be an integer from 1 to 6, got 7",
        ),
        (
            "{ { kind = 'section', level = 1.5, title = 'A', blocks = {} } }",
            "blocks[1].level must be an integer from 1 to 6, got 1.5",
        ),
        (
            "{ { kind = 'section', title = 'A', blocks = {} } }",
            "blocks[1].level must be an integer from 1 to 6, got nil",
        ),
    ]);
}

#[test]
fn render_refuses_a_block_whose_text_could_not_be_read_back() {
    assert_refused(&[
        (
            "{ { kind = 'section', level = 2, title = 'A\\nB', blocks = {} } }",
            "blocks[1].title must not hold a line break",
        ),
        (
            "{ { kind = 'section', level = 2, title = 'A' } }",
            "blocks[1].blocks must be a list of blocks, got nil",
        ),
        (
            "{ { kind = 'section', level = 2, title = 'A', blocks = { { kind = 'prose' } } } }",
            "blocks[1].blocks[1].text must be a string, got nil",
        ),
        (
            "{ { kind = 'fence', lang = 'lua', info = 'lua\\rx', text = '' } }",
            "blocks[1].info must not hold a line break",
        ),
        (
            "{ { kind = 'fence', info = 'lua', text = '' } }",
            "blocks[1].lang must be a string, got nil",
        ),
        (
            "{ { kind = 'list', ordered = true, items = {} } }",
            "blocks[1].items must hold at least one item",
        ),
        (
            "{ { kind = 'list', ordered = true, items = { 'a', 2 } } }",
            "blocks[1].items[2] must be a string, got integer",
        ),
        (
            "{ { kind = 'list', ordered = true, items = 'a' } }",
            "blocks[1].items must be a list of strings, got string",
        ),
        (
            "{ { kind = 'list', items = { 'a' } } }",
            "blocks[1].ordered must be a boolean, got nil",
        ),
        (
            "{ md[2], md[1] }",
            "blocks[2] is frontmatter, which must be the first block",
        ),
        (
            "{ { kind = 'section', level = 1, title = 'A', blocks = { md[1] } } }",
            "blocks[1].blocks[1] is frontmatter, which must be the first block",
        ),
        (
            "{ { kind = 'frontmatter', data = { 'a', 'b' } } }",
            "blocks[1].data must be a record, got a list",
        ),
        (
            "{ { kind = 'frontmatter' } }",
            "blocks[1].data must be a record, got nil",
        ),
        (
            "{ { kind = 'prose', text = 'ok \\255' } }",
            "blocks[1].text is not valid UTF-8",
        ),
        (
            "guide",
            "blocks must be a list of blocks, got a section; pass its blocks field",
        ),
        ("'text'", "blocks must be a list of blocks, got string"),
        ("{ 'text' }", "blocks[1] must be a table, got string"),
    ]);
}
