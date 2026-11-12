//! The `markdown` functions over blocks read back from `var`, and the
//! lists they refuse: one that cannot be read, one on its own path, one
//! nested too deep, and one too large to walk.

use super::*;

/// A VM whose `var.doc` holds the parse of [`GUIDE`], stored and read
/// back as guarded views.
#[track_caller]
fn stored_vm() -> BuiltVm {
    let seed = json!({});
    let vm = TestVm::new().var(&seed).shims().build();
    bind_samples(&vm);
    vm.exec("var.doc = md");
    vm
}

#[test]
fn every_function_reads_through_blocks_kept_in_var() {
    let vm = stored_vm();
    assert_values(
        &vm,
        &[
            // The stored copy is a view, not the parse result.
            ("var.doc == md", "false"),
            ("markdown.section(var.doc, '/Guide').title", r#""Guide""#),
            ("markdown.section(var.doc, '/Guide/Templates').line", "16"),
            (
                "markdown.section(var.doc, '//Extras', 'fence').text",
                r#""return 2""#,
            ),
            (
                "markdown.section(var.doc, '/Guide/Templates', 'fence', -1).lang",
                r#""json""#,
            ),
            ("markdown.section_or(var.doc, '/Guide/Nope')", "nil"),
            (
                "markdown.section_or(var.doc, '/Guide/Templates', 'list')",
                "nil",
            ),
            ("#markdown.select(var.doc, 'fence lua')", "2"),
            ("markdown.select(var.doc, 'list')[2].ordered", "true"),
            (
                "markdown.render(markdown.section(var.doc, '/Guide/Usage').blocks)",
                r#""1. first\n2. second\n""#,
            ),
            // The stored copy keeps every source, so it is still written
            // back as the text.
            ("markdown.render(var.doc) == guide_text", "true"),
            ("#markdown.select(var.doc, '*')", "16"),
            (
                "markdown.select(var.doc, '*')[4].source",
                r#""Intro text\non two lines.\n\n""#,
            ),
            // So does a copy that went through JSON text.
            (
                "markdown.render(json.decode(json.encode(md))) == guide_text",
                "true",
            ),
            // A section's `blocks` read through the view narrows a lookup.
            (
                "markdown.section(var.doc[3].blocks, '/Usage', 'list').items[2]",
                r#""second""#,
            ),
        ],
    );
}

#[test]
fn a_lookup_over_a_view_returns_the_views_own_members() {
    let vm = stored_vm();
    assert_values(
        &vm,
        &[
            ("markdown.section(var.doc, '/Guide') == var.doc[3]", "true"),
            (
                "markdown.select(var.doc, 'section')[1] == var.doc[3]",
                "true",
            ),
        ],
    );
    // What comes back is still guarded, so a write through it is checked.
    assert_raises(
        &vm,
        &[(
            "(function() markdown.section(var.doc, '/Guide').title = function() end end)()",
            "var.doc[3].title must be JSON data, got function",
        )],
    );
}

#[test]
fn a_section_given_through_a_view_is_still_not_a_list() {
    let vm = stored_vm();
    assert_raises(
        &vm,
        &[(
            "markdown.section(var.doc[3], '/Usage')",
            "markdown.section: blocks must be a list of blocks, got a section; pass its blocks \
             field",
        )],
    );
}

#[test]
fn a_list_whose_data_only_a_function_can_read_is_refused() {
    let vm = markdown_vm();
    assert_raises(
        &vm,
        &[(
            "markdown.select(sys, 'fence')",
            "markdown.select: blocks cannot be read (sys is sealed); copy the fields you need \
             into a plain table",
        )],
    );
}

#[test]
fn a_list_that_contains_itself_is_refused() {
    let vm = markdown_vm();
    vm.exec(
        "loop = { { kind = 'section', title = 'A', source = '# A\\n' } }\n\
         loop[1].blocks = loop",
    );
    assert_raises(
        &vm,
        &[
            (
                "markdown.select(loop, 'fence')",
                "markdown.select: blocks[1].blocks contains itself",
            ),
            (
                "markdown.select(loop, '*')",
                "markdown.select: blocks[1].blocks contains itself",
            ),
            (
                "markdown.section(loop, '//A[2]')",
                "markdown.section: blocks[1].blocks contains itself",
            ),
            (
                "markdown.render(loop)",
                "markdown.render: blocks[1].blocks contains itself",
            ),
        ],
    );
}

#[test]
fn a_list_nested_past_the_depth_limit_is_refused() {
    let vm = markdown_vm();
    // `deep(n)` is a list nested `n` lists deep, the list given being
    // level 1.
    vm.exec(
        "function deep(levels)\n\
           local list = {}\n\
           for _ = 2, levels do\n\
             list = { { kind = 'section', level = 1, title = 'A', source = '# A\\n', blocks = list } }\n\
           end\n\
           return list\n\
         end",
    );
    assert_values(
        &vm,
        &[
            ("#markdown.select(deep(127), 'section')", "126"),
            ("#markdown.select(deep(127), '*')", "126"),
            ("#markdown.render(deep(127))", "504"),
        ],
    );
    let name = format!("blocks{}", "[1].blocks".repeat(127));
    assert_raises(
        &vm,
        &[
            (
                "markdown.select(deep(128), 'section')",
                &format!("markdown.select: {name} is nested more than 127 levels deep"),
            ),
            (
                "markdown.section(deep(128), '//A[1]')",
                &format!("markdown.section: {name} is nested more than 127 levels deep"),
            ),
            (
                "markdown.render(deep(128))",
                &format!("markdown.render: {name} is nested more than 127 levels deep"),
            ),
        ],
    );
}

#[test]
fn a_shared_list_walked_past_the_budget_is_refused() {
    // 17 levels of a section that holds the level below twice: 35 tables
    // in memory and about 260,000 blocks in the walk, more than the
    // 131,072 a 1 MiB heap allows.
    let vm = TestVm::new()
        .limits(1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .shims()
        .build();
    vm.exec(
        "shared = {}\n\
         for _ = 1, 17 do\n\
           local section = { kind = 'section', level = 1, title = 'A', source = '# A\\n', blocks = shared }\n\
           shared = { section, section }\n\
         end",
    );
    assert_raises(
        &vm,
        &[
            (
                "markdown.select(shared, 'fence')",
                "markdown.select: blocks are too large to walk",
            ),
            (
                "markdown.section(shared, '//B')",
                "markdown.section: blocks are too large to walk",
            ),
            (
                "markdown.render(shared)",
                "markdown.render: blocks are too large to walk",
            ),
        ],
    );
}
