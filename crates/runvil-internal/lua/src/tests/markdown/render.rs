//! `markdown.render`: a parse written back as the text it was read from,
//! the one field it writes, the two line endings it adds, and the lists
//! it refuses.

use super::*;

#[test]
fn a_parsed_document_is_written_back_as_the_text_it_was_read_from() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.render(md) == guide_text", "true"),
            ("markdown.render(log) == log_text", "true"),
            ("markdown.render(items) == items_text", "true"),
            ("markdown.render(nested) == nested_text", "true"),
            ("markdown.render({})", r#""""#),
            // Whatever a rewriting writer would change comes back as it
            // was: numbers, markers, fences, rules, headings, blank lines.
            (
                "(function()\n\
                   local text = '3) c\\n4) d\\n\\n\\n* x\\n\\n~~~ ~`\\ncode\\n~~~\\n\
                 ___\\nTitle\\n=====\\n\\n\\n\\nlast'\n\
                   return markdown.render(markdown.parse(text)) == text\n\
                 end)()",
                "true",
            ),
            // Line endings are written as they were read, as `\n`, and a
            // byte order mark is not written.
            (
                "markdown.render(markdown.parse('\\239\\187\\191# T\\r\\n\\r\\nx\\r\\n'))",
                r##""# T\n\nx\n""##,
            ),
            // Text of blank lines alone has no block, so nothing is written.
            ("markdown.render(markdown.parse('\\n\\n'))", r#""""#),
        ],
    );
}

#[test]
fn the_sources_of_every_block_in_file_order_join_to_the_text() {
    let vm = markdown_vm();
    vm.exec(
        "function joined(blocks)\n\
           local parts = {}\n\
           for index, block in ipairs(markdown.select(blocks, '*')) do\n\
             parts[index] = block.source\n\
           end\n\
           return table.concat(parts)\n\
         end",
    );
    assert_values(
        &vm,
        &[
            ("joined(md) == guide_text", "true"),
            ("joined(log) == log_text", "true"),
            ("joined(items) == items_text", "true"),
            ("joined(nested) == nested_text", "true"),
            // The join of a section's blocks is what render writes for them.
            (
                "joined(templates.blocks) == markdown.render(templates.blocks)",
                "true",
            ),
        ],
    );
}

#[test]
fn a_sections_blocks_are_written_without_its_heading() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            (
                "markdown.render(usage.blocks)",
                r#""1. first\n2. second\n""#,
            ),
            // Wrapping the section in a list writes the heading too.
            (
                "markdown.render({ usage })",
                r###""## Usage\n\n1. first\n2. second\n""###,
            ),
            (
                "markdown.render({ md[1], md[2] })",
                r#""---\ntitle: Notes\ntags: [a, b]\n---\n\nPreface paragraph.\n\n""#,
            ),
            (
                "markdown.render(markdown.section(md, '//Extras').blocks)",
                r#""---\n\n```lua\nreturn 2\n```\n\n""#,
            ),
            // The blocks of a section and of its subsections, in order.
            (
                "markdown.render({ templates }) == \
                 guide_text:sub(guide_text:find('## Templates', 1, true), \
                 guide_text:find('## Usage', 1, true) - 1)",
                "true",
            ),
        ],
    );
}

#[test]
fn only_the_source_is_written_so_an_edit_of_another_field_changes_nothing() {
    let vm = markdown_vm();
    vm.exec(
        "local list = markdown.section(md, '/Guide/Usage', 'list')\n\
         list.items[#list.items + 1] = 'third'\n\
         list.ordered = false\n\
         usage.title = 'How to use it'\n\
         usage.level = 5\n\
         templates.blocks[2].text = 'return 99'\n\
         md[1].data.title = 'Other'",
    );
    assert_values(&vm, &[("markdown.render(md) == guide_text", "true")]);
    // An edit of the source is what is written.
    vm.exec(
        "usage.source = '## How to use it\\n\\n'\n\
         usage.blocks[1].source = '1. first\\n2. second\\n3. third\\n'",
    );
    assert_values(
        &vm,
        &[(
            "markdown.render({ usage })",
            r###""## How to use it\n\n1. first\n2. second\n3. third\n""###,
        )],
    );
}

#[test]
fn a_document_with_one_sections_blocks_replaced_is_written_back_otherwise_unchanged() {
    let vm = markdown_vm();
    vm.exec(
        "local entry = markdown.section(log, '/Log/Entry[1]')\n\
         entry.blocks = { { kind = 'prose', source = 'changed\\n\\n' } }",
    );
    assert_values(
        &vm,
        &[(
            "markdown.render(log)",
            r##""# Log\n\n## Entry\n\nchanged\n\n## Entry\n\nsecond\n""##,
        )],
    );
    // A heading rewritten with the blank line after it, and a block added
    // to the end of the section's list.
    vm.exec(
        "usage.source = '## How to use it\\n\\n'\n\
         table.insert(usage.blocks, { kind = 'prose', source = '\\nDone.\\n' })",
    );
    assert_values(
        &vm,
        &[
            (
                "markdown.render({ usage })",
                r###""## How to use it\n\n1. first\n2. second\n\nDone.\n""###,
            ),
            ("#usage.blocks", "2"),
        ],
    );
}

#[test]
fn a_block_built_by_hand_is_written_by_its_source_alone() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            // No field but the source is needed, a kind included.
            (
                "markdown.render({ { source = '# T\\n\\n' }, { kind = 'note', source = 'x\\n' } })",
                r##""# T\n\nx\n""##,
            ),
            // A block's other fields are not checked.
            (
                "markdown.render({ { kind = 'list', line = 'nowhere', items = 7, source = '- a\\n' } })",
                r#""- a\n""#,
            ),
            // A section with no `blocks` field holds nothing, and one
            // with no source of its own still has its blocks written.
            (
                "markdown.render({ { kind = 'section', source = '# A\\n' }, \
                 { kind = 'section', source = '', blocks = { { kind = 'rule', source = '---\\n' } } } })",
                r##""# A\n---\n""##,
            ),
            // Only a section's `blocks` are written.
            (
                "markdown.render({ { kind = 'prose', source = 'x\\n', blocks = { md[2] } } })",
                r#""x\n""#,
            ),
            // Blocks of two parses, and blocks built by hand, in one list.
            (
                "markdown.render({ log[1].blocks[2], { source = '***\\n\\n' }, usage })",
                r###""## Entry\n\nsecond\n***\n\n## Usage\n\n1. first\n2. second\n""###,
            ),
        ],
    );
}

#[test]
fn every_block_starts_on_a_new_line() {
    let vm = markdown_vm();
    // The last block of a text has no line ending when the text has none.
    vm.exec(
        "one = markdown.parse('# A\\nlast')\n\
         two = markdown.parse('# B\\n\\n- x')",
    );
    assert_values(
        &vm,
        &[
            ("one[1].blocks[1].source", r#""last""#),
            (
                "markdown.render({ one[1], two[1], one[1] })",
                r##""# A\nlast\n# B\n\n- x\n# A\nlast""##,
            ),
            // A source edited to end without one.
            (
                "markdown.render({ { kind = 'section', source = '## New' }, \
                 { kind = 'fence', source = '```\\nx\\n```' }, { kind = 'rule', source = '***' } })",
                r###""## New\n```\nx\n```\n***""###,
            ),
            // Two prose blocks built by hand get the blank line too.
            (
                "markdown.render({ { kind = 'prose', source = 'one\\n' }, \
                 { kind = 'prose', source = 'two\\n' } })",
                r#""one\n\ntwo\n""#,
            ),
        ],
    );
}

#[test]
fn a_blank_line_sits_between_two_prose_blocks_and_nowhere_else() {
    let vm = markdown_vm();
    vm.exec("mixed = markdown.parse('one\\n***\\ntwo\\n```\\nx\\n```\\nthree')");
    assert_values(
        &vm,
        &[
            // No prose block of this text has a blank line after it.
            ("#markdown.select(mixed, 'prose')", "3"),
            (
                "markdown.render(markdown.select(mixed, 'prose'))",
                r#""one\n\ntwo\n\nthree""#,
            ),
            // Nothing is added where the blank line is already there.
            (
                "markdown.render(markdown.select(md, 'prose'))",
                r#""Preface paragraph.\n\nIntro text\non two lines.\n\nUse these.\n\nThen the fallback:\n\n""#,
            ),
            // The last prose block of a section, then prose after it.
            (
                "markdown.render({ log[1].blocks[2], mixed[1] })",
                r###""## Entry\n\nsecond\n\none\n""###,
            ),
            // No other two kinds are parted: the blocks of the text are
            // written as they were, and so are two lists.
            (
                "markdown.render(mixed)",
                r#""one\n***\ntwo\n```\nx\n```\nthree""#,
            ),
            (
                "markdown.render({ usage.blocks[1], usage.blocks[1] })",
                r#""1. first\n2. second\n1. first\n2. second\n""#,
            ),
            (
                "markdown.render({ mixed[1], usage.blocks[1], mixed[3] })",
                r#""one\n1. first\n2. second\ntwo\n""#,
            ),
        ],
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
fn render_refuses_a_block_with_no_source_to_write_and_names_it() {
    assert_refused(&[
        (
            "{ { kind = 'prose', text = 'x' } }",
            "blocks[1].source must be a string, got nil",
        ),
        (
            "{ md[2], { kind = 'rule' } }",
            "blocks[2].source must be a string, got nil",
        ),
        (
            "{ { kind = 'section', source = '# A\\n', blocks = { md[2], { kind = 'prose' } } } }",
            "blocks[1].blocks[2].source must be a string, got nil",
        ),
        (
            "{ { kind = 'prose', source = 5 } }",
            "blocks[1].source must be a string, got integer",
        ),
        (
            "{ { kind = 'prose', source = { 'x' } } }",
            "blocks[1].source must be a string, got table",
        ),
        (
            "{ { kind = 'prose', source = 'ok \\255' } }",
            "blocks[1].source is not valid UTF-8",
        ),
    ]);
}

#[test]
fn render_refuses_a_value_that_is_not_a_list_of_blocks() {
    assert_refused(&[
        (
            "guide",
            "blocks must be a list of blocks, got a section; pass its blocks field",
        ),
        (
            "md[2]",
            "blocks must be a list of blocks, got a prose block",
        ),
        ("'text'", "blocks must be a list of blocks, got string"),
        ("nil", "blocks must be a list of blocks, got nil"),
        ("{ 'text' }", "blocks[1] must be a table, got string"),
        (
            "{ { kind = 'section', source = '# A\\n', blocks = 5 } }",
            "blocks[1].blocks must be a list of blocks, got integer",
        ),
        (
            "{ { kind = 'section', source = '# A\\n', blocks = usage } }",
            "blocks[1].blocks must be a list of blocks, got a section; pass its blocks field",
        ),
    ]);
}
