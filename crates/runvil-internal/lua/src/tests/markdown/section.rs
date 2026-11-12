//! `markdown.section` and `markdown.section_or`: the section a path
//! names, the block a selector picks, what is a miss and what is a
//! mistake, and the scope and identity of what comes back.

use super::*;

#[test]
fn a_path_names_one_section_from_the_top_of_the_list() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.section(md, '/Guide').title", r#""Guide""#),
            ("markdown.section(md, '/Guide/Templates').line", "16"),
            ("markdown.section(md, '/Guide/Templates/Extras').level", "3"),
            ("markdown.section(md, '//Extras').line", "34"),
            ("markdown.section(md, '//Templates/Extras').line", "34"),
            ("markdown.section(md, '/Guide//Extras').line", "34"),
            ("markdown.section(md, '//Guide').line", "8"),
            // Whitespace around the path and around each step is ignored.
            ("markdown.section(md, ' / Guide / Templates ').line", "16"),
            // A path starts at the list it is given.
            ("markdown.section(guide.blocks, '/Usage').line", "42"),
            ("markdown.section_or(md, '/Guide').title", r#""Guide""#),
        ],
    );
}

#[test]
fn a_path_that_names_no_section_raises_from_section_and_is_nil_from_section_or() {
    let vm = markdown_vm();
    let misses = [
        (
            "/Templates",
            r#"path "/Templates" matches no section; step 1, "Templates", matches nothing"#,
        ),
        // Titles are case-sensitive.
        (
            "/guide",
            r#"path "/guide" matches no section; step 1, "guide", matches nothing"#,
        ),
        (
            "/Guide/Nope",
            r#"path "/Guide/Nope" matches no section; step 2, "Nope", matches nothing"#,
        ),
        (
            "//Nope",
            r#"path "//Nope" matches no section; step 1, "//Nope", matches nothing"#,
        ),
        // A step in the middle that keeps nothing is where the path stops.
        (
            "/Nope/Templates",
            r#"path "/Nope/Templates" matches no section; step 1, "Nope", matches nothing"#,
        ),
    ];
    for (path, message) in misses {
        assert_raises(
            &vm,
            &[(
                &format!("markdown.section(md, '{path}')"),
                &format!("markdown.section: {message}"),
            )],
        );
        assert_values(
            &vm,
            &[(&format!("markdown.section_or(md, '{path}')"), "nil")],
        );
    }
}

#[test]
fn a_step_index_picks_among_sections_that_share_a_title() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.section(log, '/Log/Entry[1]').line", "3"),
            ("markdown.section(log, '/Log/Entry[2]').line", "7"),
            ("markdown.section(log, '/Log/Entry[-1]').line", "7"),
            ("markdown.section(log, '/Log[1]/Entry[2]').line", "7"),
            ("markdown.section_or(log, '/Log/Entry[3]')", "nil"),
            (
                "markdown.section(log, '/Log/Entry[2]', 'prose').text",
                r#""second""#,
            ),
        ],
    );
    let ambiguous = r#"path "/Log/Entry" is ambiguous; it matches 2 sections, at lines 3 and 7; add an index such as "/Log/Entry[1]""#;
    assert_raises(
        &vm,
        &[
            (
                "markdown.section(log, '/Log/Entry')",
                &format!("markdown.section: {ambiguous}"),
            ),
            // Several matches is a choice, not an absence, so both raise.
            (
                "markdown.section_or(log, '/Log/Entry')",
                &format!("markdown.section_or: {ambiguous}"),
            ),
            (
                "markdown.section(log, '/Log/Entry[3]')",
                r#"markdown.section: path "/Log/Entry[3]" matches no section; step 2, "Entry[3]", matches nothing"#,
            ),
        ],
    );
}

#[test]
fn each_step_is_applied_to_the_whole_tree_in_file_order() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.section(nested, '/A/A/B').line", "3"),
            ("markdown.section(nested, '/A/B').line", "4"),
            // File order, not the order the A sections were found in.
            ("markdown.section(nested, '//A/B[1]').line", "3"),
            ("markdown.section(nested, '//A/B[2]').line", "4"),
            // The inner B counts once, though both A sections reach it.
            ("markdown.section(nested, '//A//B[1]').line", "3"),
            ("markdown.section(nested, '//A//B[2]').line", "4"),
            ("markdown.section_or(nested, '//A//B[3]')", "nil"),
            ("markdown.section(nested, '//B[1]').line", "3"),
            ("markdown.section(nested, '//A[2]/B').line", "3"),
            ("markdown.section(nested, '//A[1]/B').line", "4"),
            ("markdown.section(items, '/H1/H2/Items').line", "5"),
            ("markdown.section(items, '/H1/H3/Items').line", "12"),
            ("markdown.section(items, '//H3/Items').line", "12"),
            ("markdown.section(items, '//Items[1]').line", "5"),
            ("markdown.section(items, '//Items[2]').line", "12"),
            ("markdown.section(items, '//H1//Items[2]').line", "12"),
            ("markdown.section_or(items, '/Items')", "nil"),
        ],
    );
    assert_raises(
        &vm,
        &[
            (
                "markdown.section(nested, '//A/B')",
                r#"markdown.section: path "//A/B" is ambiguous; it matches 2 sections, at lines 3 and 4; add an index such as "//A/B[1]""#,
            ),
            (
                "markdown.section_or(items, '//Items')",
                r#"markdown.section_or: path "//Items" is ambiguous; it matches 2 sections, at lines 5 and 12; add an index such as "//Items[1]""#,
            ),
            (
                "markdown.section(nested, '/A/C/B')",
                r#"markdown.section: path "/A/C/B" matches no section; step 2, "C", matches nothing"#,
            ),
            (
                "markdown.section(nested, '//A[3]/B')",
                r#"markdown.section: path "//A[3]/B" matches no section; step 1, "//A[3]", matches nothing"#,
            ),
        ],
    );
}

#[test]
fn a_title_is_matched_as_the_path_grammar_reads_it() {
    let vm = markdown_vm();
    vm.exec(
        "titles = markdown.parse('# The *big* `foo` API\\n\\n## Input/Output\\n\\n\
         ## Appendix [2]\\n\\n## [Draft] Notes\\n\\n## C:\\\\\\\\Users\\n\\n## Notes [old]\\n')",
    );
    assert_values(
        &vm,
        &[
            ("markdown.section(titles, '/The big foo API').line", "1"),
            ("markdown.section(titles, [[//Input\\/Output]]).line", "3"),
            ("markdown.section(titles, [=[//Appendix \\[2]]=]).line", "5"),
            // Unescaped, the bracketed number is an index.
            ("markdown.section_or(titles, '//Appendix [2]')", "nil"),
            ("markdown.section(titles, '//[Draft] Notes').line", "7"),
            ("markdown.section(titles, [[//C:\\\\Users]]).line", "9"),
            ("markdown.section(titles, '//Notes [old]').line", "11"),
            ("markdown.section(titles, '//Notes [old] [1]').line", "11"),
        ],
    );
}

#[test]
fn a_selector_picks_the_one_block_of_its_kind_in_the_sections_own_blocks() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            (
                "markdown.section(md, '/Guide/Templates', 'fence markdown').text",
                r##""# Hello""##,
            ),
            // The other lua fence is in Extras, and a subsection's blocks
            // are not counted.
            (
                "markdown.section(md, '/Guide/Templates', 'fence lua').text",
                r#""return 1""#,
            ),
            (
                "markdown.section(md, '/Guide/Templates', 'section').title",
                r#""Extras""#,
            ),
            (
                "markdown.section(md, '/Guide', 'list').items[2]",
                r#""beta""#,
            ),
            (
                "markdown.section(md, '//Extras', 'fence').text",
                r#""return 2""#,
            ),
            (
                "markdown.section(items, '/H1/H2/Items', 'list').items",
                r#"["a","b"]"#,
            ),
            // Extra spaces in the selector are ignored.
            (
                "markdown.section(md, '/Guide/Templates', '  fence   markdown ').lang",
                r#""markdown""#,
            ),
            (
                "markdown.section_or(md, '/Guide/Templates', 'fence markdown').text",
                r##""# Hello""##,
            ),
        ],
    );
}

#[test]
fn an_index_picks_among_several_blocks() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            (
                "markdown.section(md, '/Guide/Templates', 'fence', 3).text",
                r#""{ \"ok\": true }""#,
            ),
            (
                "markdown.section(md, '/Guide/Templates', 'fence', -1).lang",
                r#""json""#,
            ),
            (
                "markdown.section(md, '/Guide/Templates', 'fence', -3).line",
                "20",
            ),
            (
                "markdown.section(md, '/Guide/Templates', 'prose', 2).text",
                r#""Then the fallback:""#,
            ),
            // An index is allowed when there is one match.
            (
                "markdown.section(md, '/Guide/Templates', 'fence markdown', 1).text",
                r##""# Hello""##,
            ),
            // A number with an integer value is an index.
            (
                "markdown.section(md, '/Guide/Templates', 'fence', 2.0).line",
                "24",
            ),
            // A lookup scoped to a section still reports lines of the
            // whole text.
            (
                "markdown.section(guide.blocks, '/Templates', 'fence', 3).line",
                "30",
            ),
        ],
    );
    // A loop counts upward with section_or until it returns nil.
    let langs: String = vm.eval(
        "local langs, n = {}, 1\n\
         while true do\n\
           local fence = markdown.section_or(md, '/Guide/Templates', 'fence', n)\n\
           if not fence then break end\n\
           langs[n] = fence.lang\n\
           n = n + 1\n\
         end\n\
         return table.concat(langs, ',')",
    );
    assert_eq!(langs, "lua,markdown,json");
}

#[test]
fn a_block_that_is_not_there_raises_from_section_and_is_nil_from_section_or() {
    let vm = markdown_vm();
    let misses = [
        (
            "'/Guide/Templates', 'list'",
            r#"no block in "/Guide/Templates" matches selector "list""#,
        ),
        // Every fence there has a language.
        (
            r#"'/Guide/Templates', 'fence ""'"#,
            r#"no block in "/Guide/Templates" matches selector "fence """"#,
        ),
        (
            "'/Guide/Templates', 'fence', 4",
            r#"selector "fence" matches 3 blocks in "/Guide/Templates"; there is no block 4"#,
        ),
        (
            "'/Guide/Templates', 'fence', -4",
            r#"selector "fence" matches 3 blocks in "/Guide/Templates"; there is no block -4"#,
        ),
        // The second lua fence is in Extras.
        (
            "'/Guide/Templates', 'fence lua', 2",
            r#"selector "fence lua" matches 1 block in "/Guide/Templates"; there is no block 2"#,
        ),
        // A section's own blocks hold no frontmatter.
        (
            "'/Guide', 'frontmatter'",
            r#"no block in "/Guide" matches selector "frontmatter""#,
        ),
        // No such section.
        (
            "'/Guide/Nope', 'fence'",
            r#"path "/Guide/Nope" matches no section; step 2, "Nope", matches nothing"#,
        ),
    ];
    for (arguments, message) in misses {
        assert_raises(
            &vm,
            &[(
                &format!("markdown.section(md, {arguments})"),
                &format!("markdown.section: {message}"),
            )],
        );
        assert_values(
            &vm,
            &[(&format!("markdown.section_or(md, {arguments})"), "nil")],
        );
    }
}

#[test]
fn several_matches_without_an_index_raise_from_both() {
    let vm = markdown_vm();
    let three = r#"selector "fence" matches 3 blocks, at lines 20, 24, and 30; add an index, such as 1 for the first"#;
    let two = r#"selector "prose" matches 2 blocks, at lines 18 and 28; add an index, such as 1 for the first"#;
    assert_raises(
        &vm,
        &[
            (
                "markdown.section(md, '/Guide/Templates', 'fence')",
                &format!("markdown.section: {three}"),
            ),
            (
                "markdown.section_or(md, '/Guide/Templates', 'fence')",
                &format!("markdown.section_or: {three}"),
            ),
            (
                "markdown.section(md, '/Guide/Templates', 'prose')",
                &format!("markdown.section: {two}"),
            ),
            (
                "markdown.section_or(md, '/Guide/Templates', 'prose')",
                &format!("markdown.section_or: {two}"),
            ),
        ],
    );
}

#[test]
fn a_malformed_path_selector_or_index_raises_from_both_whether_or_not_the_section_is_there() {
    let vm = markdown_vm();
    for function in ["markdown.section", "markdown.section_or"] {
        let cases = [
            (
                "md, 'Guide/Setup'",
                r#"a path starts with "/", got "Guide/Setup""#.to_owned(),
            ),
            (
                "md, '/Guide/../Usage'",
                r#"path "/Guide/../Usage" uses ".."; parsed text has no parent to step to"#
                    .to_owned(),
            ),
            (
                "md, '/Guide/'",
                r#"path "/Guide/" has an empty step"#.to_owned(),
            ),
            ("md, '///a'", r#"path "///a" has an empty step"#.to_owned()),
            ("md, ''", r#"path "" has an empty step"#.to_owned()),
            (
                r"md, [[/a\b]]",
                r#"path "/a\b" has an unknown escape "\b"; the escapes are "\/", "\\", and "\[""#
                    .to_owned(),
            ),
            (
                "log, '/Log/Entry[0]'",
                r#"path "/Log/Entry[0]" has index 0; an index is a nonzero integer"#.to_owned(),
            ),
            ("md, 42", "path must be a string, got integer".to_owned()),
            ("md", "path must be a string, got nil".to_owned()),
            (
                "md, '/Guide', 'fences', 1",
                r#"unknown kind "fences"; kinds are frontmatter, section, prose, fence, list, rule"#
                    .to_owned(),
            ),
            (
                "md, '/Guide/Nope', 'fences', 1",
                r#"unknown kind "fences"; kinds are frontmatter, section, prose, fence, list, rule"#
                    .to_owned(),
            ),
            (
                "md, '/Guide', 'list lua', 1",
                r#"only a fence selector takes a language, got "list lua""#.to_owned(),
            ),
            (
                "md, '/Guide', 'fence a b'",
                r#"a fence selector takes one language, got "fence a b""#.to_owned(),
            ),
            (
                "md, '/Guide', 7",
                "selector must be a string, got integer".to_owned(),
            ),
            (
                "md, '/Guide', nil, 1",
                "an index needs a selector before it".to_owned(),
            ),
            (
                "md, '/Guide', 'fence', 0",
                "index must be a nonzero integer, got 0".to_owned(),
            ),
            (
                "md, '/Guide/Nope', 'fence', 0",
                "index must be a nonzero integer, got 0".to_owned(),
            ),
            (
                "md, '/Guide', 'fence', 1.5",
                "index must be a nonzero integer, got 1.5".to_owned(),
            ),
            (
                "md, '/Guide', 'fence', '1'",
                "index must be a nonzero integer, got string".to_owned(),
            ),
        ];
        for (arguments, message) in cases {
            assert_raises(
                &vm,
                &[(
                    &format!("{function}({arguments})"),
                    &format!("{function}: {message}"),
                )],
            );
        }
    }
}

#[test]
fn a_lookup_sees_only_the_list_it_is_given_and_a_section_is_not_a_list() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.section(guide.blocks, '/Usage').line", "42"),
            // A section is not inside its own blocks.
            ("markdown.section_or(guide.blocks, '/Guide')", "nil"),
            ("markdown.section_or(guide.blocks, '/Guide/Usage')", "nil"),
            // An empty list holds no section.
            ("markdown.section_or({}, '/Guide')", "nil"),
            ("markdown.section_or(json.array(), '//Guide')", "nil"),
        ],
    );
    for function in ["markdown.section", "markdown.section_or"] {
        assert_raises(
            &vm,
            &[
                (
                    &format!("{function}(guide, '/Usage')"),
                    &format!(
                        "{function}: blocks must be a list of blocks, got a section; pass its \
                         blocks field"
                    ),
                ),
                (
                    &format!("{function}(templates.blocks[2], '/Usage')"),
                    &format!("{function}: blocks must be a list of blocks, got a fence block"),
                ),
                (
                    &format!("{function}('text', '/Usage')"),
                    &format!("{function}: blocks must be a list of blocks, got string"),
                ),
                (
                    &format!("{function}(nil, '/Usage')"),
                    &format!("{function}: blocks must be a list of blocks, got nil"),
                ),
                (
                    &format!("{function}({{ 'text' }}, '/Usage')"),
                    &format!("{function}: blocks[1] must be a table, got string"),
                ),
                (
                    &format!(
                        "{function}({{ {{ kind = 'section', title = 'A', blocks = 5 }} }}, '/A/B')"
                    ),
                    &format!("{function}: blocks[1].blocks must be a list of blocks, got integer"),
                ),
            ],
        );
    }
}

#[test]
fn a_lookup_returns_the_tables_inside_the_list_it_was_given() {
    let vm = markdown_vm();
    assert_values(
        &vm,
        &[
            ("markdown.section(md, '/Guide') == md[3]", "true"),
            (
                "markdown.section(md, '/Guide/Templates', 'fence', 1) == templates.blocks[2]",
                "true",
            ),
            (
                "markdown.section_or(md, '//Usage', 'list') == usage.blocks[1]",
                "true",
            ),
        ],
    );
    // An edit made through a lookup's result is an edit of the document.
    vm.exec("markdown.section(md, '/Guide/Usage', 'list').items[1] = 'edited'");
    assert_values(
        &vm,
        &[("md[3].blocks[4].blocks[1].items[1]", r#""edited""#)],
    );
}

#[test]
fn a_lookup_reads_only_the_fields_it_needs_from_blocks_built_by_hand() {
    let vm = markdown_vm();
    vm.exec(
        "built = {\n\
           { kind = 'note', title = 'A' },\n\
           { kind = 'section', title = 'A', blocks = {\n\
             { kind = 'fence', lang = 'lua' },\n\
             { kind = 'fence' },\n\
             { kind = 'mystery', lang = 'lua' },\n\
           } },\n\
           { kind = 'section', title = 'A' },\n\
           { title = 'A' },\n\
         }",
    );
    assert_values(
        &vm,
        &[
            // A block whose kind the lookup does not know matches nothing.
            ("markdown.section(built, '/A[1]') == built[2]", "true"),
            ("markdown.section(built, '/A[2]') == built[3]", "true"),
            ("markdown.section_or(built, '/A[3]')", "nil"),
            (
                "markdown.section(built, '/A[1]', 'fence lua') == built[2].blocks[1]",
                "true",
            ),
            (
                "markdown.section(built, '/A[1]', 'fence', 2) == built[2].blocks[2]",
                "true",
            ),
            // A fence with no `lang` field has no language to match.
            ("markdown.section_or(built, '/A[1]', 'fence \"\"')", "nil"),
            // A section with no `blocks` field holds nothing.
            ("markdown.section_or(built, '/A[2]', 'fence')", "nil"),
            ("markdown.section_or(built, '/A[2]/B')", "nil"),
        ],
    );
    // Without lines, the messages that list them leave them out.
    assert_raises(
        &vm,
        &[
            (
                "markdown.section(built, '/A')",
                r#"markdown.section: path "/A" is ambiguous; it matches 2 sections; add an index such as "/A[1]""#,
            ),
            (
                "markdown.section(built, '/A[1]', 'fence')",
                r#"markdown.section: selector "fence" matches 2 blocks; add an index, such as 1 for the first"#,
            ),
        ],
    );
}
