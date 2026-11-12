//! `markdown.select`: every block a selector matches, at any depth.

use super::*;

#[test]
fn select_collects_every_match_at_any_depth_in_file_order() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("#markdown.select(md, 'section')", "4"),
            ("#markdown.select(md, 'fence')", "4"),
            ("#markdown.select(md, 'fence lua')", "2"),
            ("markdown.select(md, 'fence lua')[2].text", r#""return 2""#),
            ("markdown.select(md, 'list')[2].ordered", "true"),
            ("#markdown.select(md, 'frontmatter')", "1"),
            ("#markdown.select(md, 'rule')", "1"),
            (
                "markdown.select(templates.blocks, 'prose')[1].text",
                r#""Use these.""#,
            ),
            // Searched inside Guide only.
            ("#markdown.select(guide.blocks, 'fence lua')", "2"),
        ],
    );
    // A section comes before its members.
    let titles: String = vm.eval(
        "local titles = {}\n\
         for index, section in ipairs(markdown.select(md, 'section')) do\n\
           titles[index] = section.title\n\
         end\n\
         return table.concat(titles, ',')",
    );
    assert_eq!(titles, "Guide,Templates,Extras,Usage");
}

#[test]
fn a_star_selects_every_block_at_any_depth_in_file_order() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("#markdown.select(md, '*')", "16"),
            // The same tables, a section before the blocks under it.
            ("markdown.select(md, '*')[1] == md[1]", "true"),
            ("markdown.select(md, '*')[3] == guide", "true"),
            ("markdown.select(md, '*')[4] == guide.blocks[1]", "true"),
            ("markdown.select(md, '*')[6] == templates", "true"),
            ("markdown.select(md, '*')[7] == templates.blocks[1]", "true"),
            ("markdown.select(md, '*')[16] == usage.blocks[1]", "true"),
            // Searched inside Templates only: its six blocks and the two
            // under Extras.
            ("#markdown.select(templates.blocks, '*')", "8"),
            ("#markdown.select({}, '*')", "0"),
            ("#markdown.select(nested, ' * ')", "4"),
        ],
    );
    let kinds: String = vm.eval(
        "local kinds = {}\n\
         for index, block in ipairs(markdown.select(md, '*')) do\n\
           kinds[index] = block.kind\n\
         end\n\
         return table.concat(kinds, ',')",
    );
    assert_eq!(
        kinds,
        "frontmatter,prose,section,prose,list,section,prose,fence,fence,prose,fence,section,\
         rule,fence,section,list"
    );
    // Every block is selected, whatever its `kind` field holds.
    vm.exec(
        "built = {\n\
           { kind = 'note' },\n\
           { title = 'no kind' },\n\
           { kind = 'section', blocks = { { kind = 7 }, {} } },\n\
           { kind = 'prose', blocks = { { kind = 'rule' } } },\n\
         }",
    );
    assert_values(
        &vm,
        &[
            // Only a section's `blocks` are searched.
            ("#markdown.select(built, '*')", "6"),
            ("markdown.select(built, '*')[2] == built[2]", "true"),
            (
                "markdown.select(built, '*')[5] == built[3].blocks[2]",
                "true",
            ),
            ("#markdown.select(built, 'section')", "1"),
            ("#markdown.select(built, 'rule')", "0"),
        ],
    );
}

#[test]
fn select_returns_a_new_marked_list_of_the_tables_themselves() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            // Empty when nothing matches, and still a list.
            ("markdown.select(md, 'fence python')", "[]"),
            ("markdown.select({}, 'fence')", "[]"),
            (
                "markdown.select(md, 'fence')[1] == templates.blocks[2]",
                "true",
            ),
            ("markdown.select(md, 'section')[1] == md[3]", "true"),
            // Each call builds its own list.
            (
                "markdown.select(md, 'rule') == markdown.select(md, 'rule')",
                "false",
            ),
            // A fanout collection is a list of sections.
            ("markdown.select(log, 'section')[3].line", "7"),
        ],
    );
}

#[test]
fn select_refuses_a_malformed_selector_and_a_value_that_is_not_a_list_of_blocks() {
    let vm = markdown_vm();
    assert_raises(
        &vm,
        &[
            (
                "markdown.select(md, 'fences')",
                r#"markdown.select: unknown kind "fences"; kinds are frontmatter, section, prose, fence, list, rule, and * selects every block"#,
            ),
            (
                "markdown.select(md, 'rule 2')",
                r#"markdown.select: only a fence selector takes a language, got "rule 2""#,
            ),
            (
                "markdown.select(md, '* lua')",
                r#"markdown.select: only a fence selector takes a language, got "* lua""#,
            ),
            (
                "markdown.select(md, '**')",
                r#"markdown.select: unknown kind "**"; kinds are frontmatter, section, prose, fence, list, rule, and * selects every block"#,
            ),
            (
                "markdown.select(md)",
                "markdown.select: selector must be a string, got nil",
            ),
            (
                "markdown.select(md, 3)",
                "markdown.select: selector must be a string, got integer",
            ),
            (
                "markdown.select(guide, 'fence')",
                "markdown.select: blocks must be a list of blocks, got a section; pass its blocks \
                 field",
            ),
            (
                "markdown.select('text', 'fence')",
                "markdown.select: blocks must be a list of blocks, got string",
            ),
            (
                "markdown.select({ md[3], 7 }, 'fence')",
                "markdown.select: blocks[2] must be a table, got integer",
            ),
        ],
    );
}
