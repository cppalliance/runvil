//! The selector forms of `prompt.section` and `prompt.section_or` in a
//! running prompt: one block of a section's own content, read from a
//! section, from one level down, from any depth, from the H1 body, and
//! from a library.
//!
//! Each prompt probes expressions where they are written. A probe appends
//! one line to `probe.txt`: the value's JSON text, `nil`, or `raises: `
//! and the error's text. The last section returns the file.

use super::*;

/// The first of a probe site's three lines: the function that records
/// one result.
const PROBE: &str = "local function probe(f) local ok, value = pcall(f) local line = \"nil\" \
    if not ok then line = \"raises: \" .. tostring(value) elseif value ~= nil then line = \
    json.encode(value) end store.append(\"probe.txt\", line .. \"\\n\") end";

/// The section that hands the probes' lines back as the run's text.
const REPORT: &str = "\n## Report\n\n```lua\nreturn store.read(\"probe.txt\")\n```\n";

/// Three lines of Lua that probe each of `expressions` where the lines
/// are written, and then run `then`.
fn probes(expressions: &[&str], then: &str) -> String {
    let calls: Vec<String> = expressions
        .iter()
        .map(|expression| format!("probe(function() return {expression} end)"))
        .collect();
    format!("{PROBE}\n{}\n{then}", calls.join(" "))
}

/// Runs `source` and asserts that its probes gave `expected`, in order. An
/// expectation that starts with `raises: ` is a failure whose text holds
/// the rest, and any other is the whole line.
#[track_caller]
fn assert_probes(source: &str, expected: &[&str]) {
    let trace = Scenario::new(source.to_owned()).run();
    let text = trace.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), expected.len(), "one line per probe: {text}");
    for (number, (line, want)) in lines.iter().zip(expected).enumerate() {
        match want.strip_prefix("raises: ") {
            Some(message) => assert!(
                line.starts_with("raises: ") && line.contains(message),
                "probe {} must raise {message}, got {line}",
                number + 1
            ),
            None => assert_eq!(line, want, "probe {}", number + 1),
        }
    }
}

/// The report prompt: `Write` holds `house_style`, `Template`, and
/// `Topics`, and its sibling is `Review`. `write` is the three lines of
/// Lua in `Write`, and `template` is added to the end of `Template`.
fn report(write: &str, template: &str) -> String {
    format!(
        "---\nname: report\ndescription: Write one report per topic\nrunvil: 0\n---\n\n\
         # Report\n\n\
         ## Write\n\n\
         ```lua\n{write}\n```\n\n\
         ### house_style\n\n\
         Short sentences. No hype.\n\n\
         ### Template\n\n\
         Fill in this outline.\n\n\
         ```markdown\n# Title\n\n## Summary\n```\n{template}\n\
         ### Topics\n\n\
         - allocators\n- coroutines\n\n\
         ## Review\n\n\
         Check every claim.\n{REPORT}"
    )
}

#[test]
fn a_section_reads_one_block_of_a_child_a_sibling_or_any_section() {
    let write = probes(
        &[
            r#"prompt.section("house_style")"#,
            r#"prompt.section("Template")"#,
            r#"prompt.section("Template", "fence markdown").text"#,
            r#"prompt.section("Template", "fence markdown").line"#,
            r#"prompt.section("Template", "fence").lang"#,
            r#"prompt.section("Template", "prose").text"#,
            r#"prompt.section("Template", "fence", 1).lang"#,
            r#"prompt.section("Topics", "list").items"#,
            r#"prompt.section("../Review")"#,
            r#"prompt.section("/Review")"#,
            r#"prompt.section("/Write/Template", "fence").lang"#,
            r#"prompt.section("Missing")"#,
            r#"prompt.section("Review")"#,
            r#"prompt.section("../Write")"#,
            r#"prompt.section("Template", "list")"#,
            r#"prompt.section("Template", "fence", 2)"#,
            r#"prompt.section("Template/Summary")"#,
            r#"prompt.section_or("Missing")"#,
            r#"prompt.section_or("Missing", "fence")"#,
            r#"prompt.section_or("Template", "list")"#,
            r#"prompt.section_or("Template", 'fence ""')"#,
            r#"prompt.section_or("Template/Summary")"#,
        ],
        "local probed = true",
    );
    assert_probes(
        &report(&write, ""),
        &[
            r#""Short sentences. No hype.""#,
            r#""Fill in this outline.\n\n```markdown\n# Title\n\n## Summary\n```""#,
            r##""# Title\n\n## Summary""##,
            // A line of the prompt file.
            "25",
            r#""markdown""#,
            r#""Fill in this outline.""#,
            r#""markdown""#,
            r#"["allocators","coroutines"]"#,
            r#""Check every claim.""#,
            r#""Check every claim.""#,
            r#""markdown""#,
            "raises: section `Missing` not found: `/Write` has no child `Missing`",
            "raises: section `Review` not found: `/Write` has no child `Review`",
            "raises: section `../Write` not found: `/Write` has no sibling `Write`",
            "raises: prompt.section: no block in section `Template` matches selector \"list\"",
            "raises: prompt.section: selector \"fence\" matches 1 block in section `Template`; \
             there is no block 2",
            "raises: section path \"Template/Summary\" has more than one name; a relative path \
             is a child's name, or \"../\" and a sibling's name",
            "nil",
            "nil",
            "nil",
            "nil",
            "raises: section path \"Template/Summary\" has more than one name",
        ],
    );
}

#[test]
fn a_section_one_level_down_reads_its_siblings_and_any_section() {
    let template = format!(
        "\n```lua\n{}\n```\n",
        probes(
            &[
                r#"prompt.section("../Topics", "list").items"#,
                r#"prompt.section("/Review")"#,
                r#"prompt.section("/Write", "fence lua").lang"#,
                r#"prompt.section_or("../Review")"#,
                // Its own content holds two fences now, and a path from
                // the top reads them.
                r#"prompt.section("/Write/Template", "fence", 1).lang"#,
                r#"prompt.section("/Write/Template", "fence lua").kind"#,
            ],
            "local probed = true",
        )
    );
    assert_probes(
        &report("call(\"Template\")\n\n", &template),
        &[
            r#"["allocators","coroutines"]"#,
            r#""Check every claim.""#,
            r#""lua""#,
            "nil",
            r#""markdown""#,
            r#""fence""#,
        ],
    );
}

/// Two workers under different parents and one top-level `Templates`.
/// `research` is the Lua of `Research`, and `worker` the Lua of its
/// `Worker`.
fn shared_template(research: &str, worker: &str) -> String {
    format!(
        "---\nname: shared-template\ndescription: Two workers read one template\nrunvil: 0\n\
         ---\n\n\
         # Shared template\n\n\
         ## Research\n\n\
         ```lua\n{research}\n```\n\n\
         ### Worker\n\n\
         ```lua\n{worker}\n```\n\n\
         ## Review\n\n\
         ### Worker\n\n\
         ```lua\nlocal template = prompt.section(\"/Templates\", \"fence markdown\").text\n```\n\n\
         ## Templates\n\n\
         ```markdown\n# Title\n```\n{REPORT}"
    )
}

#[test]
fn a_shared_section_has_one_path_at_every_depth() {
    let worker = probes(
        &[
            r#"prompt.section("/Templates", "fence markdown").text"#,
            r#"prompt.section("/Review/Worker", "fence lua").lang"#,
            r#"prompt.section_or("../Templates")"#,
            r#"call("/Templates")"#,
        ],
        "local probed = true",
    );
    let research = probes(
        &[
            r#"prompt.section("../Templates", "fence markdown").text"#,
            r#"prompt.section("/Templates", "fence markdown").text"#,
        ],
        "call(\"Worker\")",
    );
    assert_probes(
        &shared_template(&research, &worker),
        &[
            r##""# Title""##,
            r##""# Title""##,
            r##""# Title""##,
            r#""lua""#,
            "nil",
            "raises: section path \"/Templates\" starts with \"/\"; only require, \
             prompt.section, and prompt.section_or take a path from the top",
        ],
    );
}

#[test]
fn one_title_under_two_parents_is_one_section_from_each_and_one_from_the_top() {
    let source = format!(
        "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
         # H1\n\n\
         ```lua\n{}\n```\n\n\
         ## H2\n\n\
         ```lua\n{}\n```\n\n\
         ### Items\n\n\
         - a\n- b\n\n\
         ```lua\n{}\n```\n\n\
         ## H3\n\n\
         ### Items\n\n\
         - c\n{REPORT}",
        probes(
            &[
                r#"prompt.section("/H2/Items", "list").items"#,
                r#"prompt.section("Items")"#,
                r#"prompt.section_or("Items")"#,
                r#"prompt.section("H3")"#,
            ],
            "local probed = true",
        ),
        probes(
            &[
                r#"prompt.section("Items", "list").items"#,
                r#"prompt.section("/H3/Items", "list").items"#,
                r#"prompt.section("../H3/Items")"#,
            ],
            "call(\"Items\")",
        ),
        probes(
            &[
                r#"prompt.section("../Items")"#,
                r#"prompt.section("/H3")"#,
                r#"prompt.section("/H3/Items", "list").items[1]"#,
            ],
            "local probed = true",
        ),
    );
    assert_probes(
        &source,
        &[
            // From the H1 body.
            r#"["a","b"]"#,
            "raises: section `Items` not found: the top level has no section `Items`",
            "nil",
            r#""""#,
            // From H2.
            r#"["a","b"]"#,
            r#"["c"]"#,
            "raises: section path \"../H3/Items\" has more than one name",
            // From Items under H2.
            "raises: section `../Items` not found: `/H2/Items` has no sibling `Items`",
            r#""""#,
            r#""c""#,
        ],
    );
}

#[test]
fn a_library_reads_blocks_of_its_own_sections_and_of_its_users_through_g() {
    let library = "
        return {
          read = function()
            local template = prompt.section('Template', 'fence markdown').text
            local topics = _G.prompt.section_or('Topics', 'list')
            return template .. '|' .. (topics and table.concat(topics.items, ',') or 'none')
          end,
          strict = function() return _G.prompt.section('Topics', 'list') end,
        }";
    let source = PromptSource::new()
        .section(
            "Research",
            "
            var.research = require('/Library').read()",
        )
        .prose("### Topics", "- allocators\n- coroutines")
        .section(
            "Review",
            "
            local library = require('/Library')
            local ok, err = pcall(library.strict)
            return var.research .. '\\n' .. library.read() .. '\\n' .. tostring(err)",
        )
        .section("Library", library)
        .prose("### Template", "```markdown\n# Title\n```");
    let trace = Scenario::new(source.build()).run();
    let text = trace.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[..2],
        ["# Title|allocators,coroutines", "# Title|none"],
        "the library's template is its own, and the topics are its user's: {text}"
    );
    assert!(
        lines[2].contains("section `Topics` not found: `/Review` has no child `Topics`"),
        "a strict read through _G names the user: {text}"
    );
}

#[test]
fn an_uncaught_strict_read_in_the_h1_body_fails_the_gate_before_any_section_runs() {
    let source = PromptSource::new()
        .h1("local template = prompt.section('/Templates', 'fence markdown').text")
        .section("Main", "store.write('ran.txt', 'yes')\nreturn 'ran'")
        .prose("Templates", "No fence here.");
    let trace = Scenario::new(source.build()).run();
    let rendered = trace.error().to_string();
    assert!(
        rendered.contains(
            "prompt.section: no block in section `/Templates` matches selector \"fence markdown\""
        ),
        "the read is a check that the prompt has what it needs: {rendered}"
    );
    assert!(
        trace.read("ran.txt").is_err(),
        "no section runs after a failed gate"
    );
}
