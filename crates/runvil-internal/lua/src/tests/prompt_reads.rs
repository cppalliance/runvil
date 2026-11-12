//! The `prompt` reads over a reader: `prompt.section` returns a section's
//! Markdown or one block of it and raises for what is not there,
//! `prompt.section_or` returns nil for that, and both raise for a path the
//! reader refuses, a target that is not a string, and a malformed selector
//! or index.

use super::*;

/// The error the reader gives for a path that names no section.
const MISSING: &str = "section `Missing` not found: `/Main` has no child `Missing`";

/// The error the reader gives for the path it refuses.
const REFUSED: &str = "section path \"bad/\" has an empty step";

/// A section's Markdown with prose and one fence, which starts on line 23
/// of its prompt file.
const TEMPLATE: &str = "Fill in this outline.\n\n```markdown\n# Title\n\n## Summary\n```";

/// A section's Markdown with a paragraph, two lists, and a `lua` fence,
/// which starts on line 40 of its prompt file.
const TOPICS: &str = "Pick from these:\n\n- allocators\n  - arenas\n- coroutines\n-\n\n\
```lua\nreturn 'not run'\n```\n\n1. one\n2. two";

/// A section's Markdown that opens as frontmatter would, which starts on
/// line 60 of its prompt file.
const LOOKALIKE: &str = "---\n\na: b\n\n---";

/// The text of a section whose Markdown is `markdown`, starting on `line`
/// of the prompt file.
fn section_text(markdown: &str, line: u32) -> SectionText {
    SectionText::new(
        Arc::from(markdown),
        0..markdown.len(),
        NonZeroU32::new(line).expect("a prompt line is at least 1"),
        Vec::new(),
    )
}

/// A VM whose reader knows the sections `Rules`, `Template`, `Topics`,
/// `Lookalike`, and the empty `Empty`, refuses the path `bad/`, and finds
/// no section for any other path.
fn reading_vm() -> BuiltVm {
    TestVm::new()
        .reading(|path, _| {
            SectionRead::Found(match path.trim() {
                "Rules" => section_text("Be brief.", 1),
                "Template" => section_text(TEMPLATE, 23),
                "Topics" => section_text(TOPICS, 40),
                "Lookalike" => section_text(LOOKALIKE, 60),
                "Empty" => section_text("", 70),
                _ => return SectionRead::Missing(crate::make_lua_runtime_error(MISSING)),
            })
        })
        .build()
}

/// Asserts that each Lua expression gives the value its JSON text names,
/// where `nil` stands for a nil result.
#[track_caller]
fn assert_values(vm: &BuiltVm, cases: &[(&str, &str)]) {
    for (expression, want) in cases {
        let got: String = vm.eval(&format!(
            "local value = {expression}\n\
             if value == nil then return 'nil' end\n\
             return json.encode(value)"
        ));
        assert_eq!(got, *want, "{expression}");
    }
}

/// Asserts that each Lua expression raises a `lua` error whose text is
/// the message beside it.
#[track_caller]
fn assert_raises(vm: &BuiltVm, cases: &[(&str, &str)]) {
    for (expression, message) in cases {
        assert_eq!(caught(vm, expression), lua_error(message), "{expression}");
    }
}

#[test]
fn section_returns_the_markdown_and_raises_for_a_path_that_names_no_section() {
    let vm = reading_vm();
    assert_values(
        &vm,
        &[
            ("prompt.section('Rules')", r#""Be brief.""#),
            ("prompt.section('Empty')", r#""""#),
        ],
    );
    assert_raises(&vm, &[("prompt.section('Missing')", MISSING)]);
}

#[test]
fn section_or_returns_the_markdown_and_nil_for_a_path_that_names_no_section() {
    let vm = reading_vm();
    assert_values(
        &vm,
        &[
            ("prompt.section_or('Rules')", r#""Be brief.""#),
            ("prompt.section_or('Missing')", "nil"),
            ("prompt.section_or('Missing', 'fence')", "nil"),
            ("prompt.section_or('Missing', 'fence', 2)", "nil"),
        ],
    );
}

#[test]
fn both_reads_raise_for_a_refused_path_and_for_a_target_that_is_not_a_string() {
    // The reader of this VM refuses one path with an error of its own.
    let vm = TestVm::new().build();
    vm.install_control_globals(|_: String, _| -> Result<SectionRead> {
        Err(crate::make_lua_runtime_error(REFUSED))
    })
    .expect("the control globals install");
    for read in ["section", "section_or"] {
        let refused: String = vm.eval(&format!(
            "local ok, err = pcall(prompt.{read}, 'bad/')\n\
             assert(not ok, 'the read must raise')\n\
             return tostring(err)"
        ));
        assert!(refused.contains(REFUSED), "prompt.{read} got {refused}");
    }
    let vm = reading_vm();
    for read in ["prompt.section", "prompt.section_or"] {
        for (argument, kind) in [("42", "integer"), ("nil", "nil"), ("{}", "table")] {
            assert_raises(
                &vm,
                &[(
                    &format!("{read}({argument})"),
                    &format!("section target must be a string, got {kind}"),
                )],
            );
        }
    }
}

#[test]
fn a_selector_picks_one_block_of_the_sections_own_content() {
    let vm = reading_vm();
    assert_values(
        &vm,
        &[
            (
                "prompt.section('Template', 'fence markdown').text",
                r##""# Title\n\n## Summary""##,
            ),
            ("prompt.section('Template', 'fence').lang", r#""markdown""#),
            (
                "prompt.section('Template', 'prose').text",
                r#""Fill in this outline.""#,
            ),
            // An index is allowed when there is one match.
            (
                "prompt.section('Template', 'fence', 1).lang",
                r#""markdown""#,
            ),
            ("prompt.section('Template', 'fence', -1).kind", r#""fence""#),
            (
                "prompt.section_or('Template', 'fence markdown').info",
                r#""markdown""#,
            ),
            // A list beside prose is found, and an index picks among two.
            (
                "prompt.section('Topics', 'list', 1).items",
                r#"["allocators\n- arenas","coroutines",""]"#,
            ),
            ("prompt.section('Topics', 'list', 2).ordered", "true"),
            ("prompt.section('Topics', 'list', -1).items[2]", r#""two""#),
            // A Lua fence is a fence like any other, and it is not run.
            (
                "prompt.section('Topics', 'fence lua').text",
                r#""return 'not run'""#,
            ),
            (
                "prompt.section('  Template ', ' fence ').kind",
                r#""fence""#,
            ),
        ],
    );
}

#[test]
fn a_blocks_line_counts_lines_of_the_prompt_file() {
    let vm = reading_vm();
    assert_values(
        &vm,
        &[
            ("prompt.section('Template', 'prose').line", "23"),
            ("prompt.section('Template', 'fence markdown').line", "25"),
            ("prompt.section('Topics', 'list', 1).line", "42"),
            ("prompt.section('Topics', 'fence').line", "47"),
            ("prompt.section('Topics', 'list', 2).line", "51"),
            // Parsing the section's text counts from the section instead.
            ("markdown.parse(prompt.section('Template'))[2].line", "3"),
        ],
    );
}

#[test]
fn the_block_is_a_plain_table_built_for_the_call() {
    let vm = reading_vm();
    assert_values(
        &vm,
        &[
            (
                "prompt.section('Template', 'prose')",
                r#"{"kind":"prose","line":23,"text":"Fill in this outline."}"#,
            ),
            (
                "prompt.section('Topics', 'list', 2)",
                r#"{"items":["one","two"],"kind":"list","line":51,"ordered":true}"#,
            ),
            // Each call builds a new table, so editing one changes nothing
            // else.
            (
                "prompt.section('Template', 'fence') == prompt.section('Template', 'fence')",
                "false",
            ),
            (
                "(function()\n\
                   prompt.section('Template', 'fence').text = 'edited'\n\
                   return prompt.section('Template', 'fence').text\n\
                 end)()",
                r##""# Title\n\n## Summary""##,
            ),
        ],
    );
}

#[test]
fn a_block_that_is_not_there_raises_from_section_and_is_nil_from_section_or() {
    let vm = reading_vm();
    let misses = [
        (
            "'Template', 'list'",
            "no block in section `Template` matches selector \"list\"",
        ),
        (
            "'Template', 'fence', 2",
            "selector \"fence\" matches 1 block in section `Template`; there is no block 2",
        ),
        // The only fence has a language.
        (
            r#"'Template', 'fence ""'"#,
            "no block in section `Template` matches selector \"fence \"\"\"",
        ),
        (
            "' Topics ', 'list', 3",
            "selector \"list\" matches 2 blocks in section `Topics`; there is no block 3",
        ),
        (
            "'Empty', 'prose'",
            "no block in section `Empty` matches selector \"prose\"",
        ),
        (
            "'Rules', 'rule', -1",
            "no block in section `Rules` matches selector \"rule\"",
        ),
    ];
    for (arguments, message) in misses {
        assert_raises(
            &vm,
            &[(
                &format!("prompt.section({arguments})"),
                &format!("prompt.section: {message}"),
            )],
        );
        assert_values(&vm, &[(&format!("prompt.section_or({arguments})"), "nil")]);
    }
}

#[test]
fn several_matches_without_an_index_raise_from_both_and_name_lines_of_the_prompt_file() {
    let vm = reading_vm();
    let two = "selector \"list\" matches 2 blocks, at lines 42 and 51; add an index, such as 1 \
               for the first";
    assert_raises(
        &vm,
        &[
            (
                "prompt.section('Topics', 'list')",
                &format!("prompt.section: {two}"),
            ),
            (
                "prompt.section_or('Topics', 'list')",
                &format!("prompt.section_or: {two}"),
            ),
        ],
    );
}

#[test]
fn the_kinds_a_sections_own_content_never_holds_are_refused() {
    let vm = reading_vm();
    for read in ["prompt.section", "prompt.section_or"] {
        // Whether or not the section is there.
        for path in ["Template", "Missing"] {
            assert_raises(
                &vm,
                &[
                    (
                        &format!("{read}('{path}', 'section')"),
                        &format!(
                            "{read}: a section's own content has no subsections; name a \
                             subsection by its own path"
                        ),
                    ),
                    (
                        &format!("{read}('{path}', 'frontmatter', 1)"),
                        &format!("{read}: a section's own content has no frontmatter"),
                    ),
                ],
            );
        }
    }
}

#[test]
fn frontmatter_is_never_recognized_in_a_sections_content() {
    let vm = reading_vm();
    assert_values(
        &vm,
        &[
            ("prompt.section('Lookalike', 'rule', 1).line", "60"),
            ("prompt.section('Lookalike', 'prose').text", r#""a: b""#),
            ("prompt.section('Lookalike', 'rule', 2).line", "64"),
            // The same text given to `markdown.parse` opens with it.
            (
                "markdown.parse(prompt.section('Lookalike'))[1].kind",
                r#""frontmatter""#,
            ),
        ],
    );
}

#[test]
fn a_malformed_selector_or_index_raises_from_both_before_the_path_is_looked_up() {
    let vm = reading_vm();
    for read in ["prompt.section", "prompt.section_or"] {
        for path in ["Template", "Missing"] {
            let cases = [
                (
                    "'fences', 1",
                    "unknown kind \"fences\"; kinds are frontmatter, section, prose, fence, list, \
                     rule",
                ),
                (
                    "'list lua'",
                    "only a fence selector takes a language, got \"list lua\"",
                ),
                ("7", "selector must be a string, got integer"),
                ("nil, 1", "an index needs a selector before it"),
                ("'fence', 0", "index must be a nonzero integer, got 0"),
                ("'fence', 1.5", "index must be a nonzero integer, got 1.5"),
                (
                    "'fence', 'one'",
                    "index must be a nonzero integer, got string",
                ),
            ];
            for (arguments, message) in cases {
                assert_raises(
                    &vm,
                    &[(
                        &format!("{read}('{path}', {arguments})"),
                        &format!("{read}: {message}"),
                    )],
                );
            }
        }
    }
}
