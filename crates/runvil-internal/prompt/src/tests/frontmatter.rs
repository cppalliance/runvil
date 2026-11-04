//! Frontmatter tests: required keys and delimiters, the tool-loop cap,
//! the `runvil:` key, declared files, and the line of each key.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use super::*;

#[test]
fn name_and_description_are_sufficient_frontmatter_for_parsing() {
    let prompt = parse_ok("---\nname: x\ndescription: d\n---\n\n# T\n\n## S\n\np\n");
    assert_eq!(prompt.frontmatter.name(), "x");
}

#[test]
fn missing_frontmatter_delimiter_errors() {
    assert_eq!(
        parse_err("# T\n\n## S\n\np\n").code,
        ErrorCode::FrontmatterInvalid
    );
}

#[test]
fn an_indented_delimiter_in_a_block_scalar_does_not_close_the_frontmatter() {
    let src = "---\nname: x\ndescription: |\n  first\n  ---\n  last\n---\n\n# T\n\n## S\n\np\n";
    let prompt = parse_ok(src);
    assert_eq!(prompt.frontmatter.description(), "first\n---\nlast\n");
}

#[test]
fn unknown_frontmatter_field_is_rejected() {
    let src = prompt("not_a_real_field: 1", &[]);
    assert_parse_error(&src, ErrorCode::FrontmatterInvalid, "not_a_real_field");
    // A known-field-only frontmatter still parses.
    parse_ok(&prompt("", &[]));
}

#[test]
fn max_tool_iterations_parses_positive_and_is_absent_when_unset() {
    let p = parse_ok(&prompt("max_tool_iterations: 20", &[]));
    assert_eq!(
        p.frontmatter.max_tool_iterations(),
        Some(std::num::NonZeroU32::new(20).unwrap())
    );

    let p = parse_ok(&prompt("", &[]));
    assert_eq!(p.frontmatter.max_tool_iterations(), None);
}

#[test]
fn max_tool_iterations_rejects_zero_negative_and_overflow() {
    for bad in ["0", "-1", "1001", "100000000000"] {
        let error = parse_err(&prompt(&format!("max_tool_iterations: {bad}"), &[]));
        assert_eq!(
            error.code,
            ErrorCode::FrontmatterInvalid,
            "value {bad}: {error}"
        );
    }
}

#[test]
fn max_tool_iterations_reports_the_guides_messages() {
    for raw in ["0", "-1"] {
        assert_parse_error(
            &prompt(&format!("max_tool_iterations: {raw}"), &[]),
            ErrorCode::FrontmatterInvalid,
            &format!("max_tool_iterations must be a positive integer (>= 1), got {raw}"),
        );
    }
    for raw in ["1001", "100000000000"] {
        assert_parse_error(
            &prompt(&format!("max_tool_iterations: {raw}"), &[]),
            ErrorCode::FrontmatterInvalid,
            &format!("max_tool_iterations must be <= 1000, got {raw}"),
        );
    }
}

#[test]
fn max_tool_iterations_accepts_the_upper_boundary() {
    let p = parse_ok(&prompt(
        &format!("max_tool_iterations: {MAX_TOOL_ITERATIONS}"),
        &[],
    ));
    assert_eq!(
        p.frontmatter.max_tool_iterations(),
        Some(std::num::NonZeroU32::new(MAX_TOOL_ITERATIONS).unwrap())
    );
}

#[test]
fn frontmatter_exposes_runvil_field() {
    let with = "---\nname: x\ndescription: d\nrunvil: 0\n---\n\n# T\n\n## S\n\np\n";
    assert_eq!(parse_ok(with).frontmatter.runvil(), Some(0));

    let without = "---\nname: x\ndescription: d\n---\n\n# T\n\n## S\n\np\n";
    assert_eq!(parse_ok(without).frontmatter.runvil(), None);
}

#[test]
fn frontmatter_parses_input_and_output() {
    let prompt = parse_ok(&prompt(
        "
        input: { path: paper.md, description: The input paper }
        output: { path: report.md, description: The output report }
        ",
        &[],
    ));
    let fm = prompt.frontmatter();
    let input = fm.input().expect("input declared");
    assert_eq!(input.path(), "paper.md");
    assert_eq!(input.description(), "The input paper");
    let output = fm.output().expect("output declared");
    assert_eq!(output.path(), "report.md");
    assert_eq!(output.description(), "The output report");
}

#[test]
fn frontmatter_without_input_output_still_parses() {
    let prompt = parse_ok(&prompt("", &[]));
    assert!(prompt.frontmatter().input().is_none());
    assert!(prompt.frontmatter().output().is_none());
}

#[test]
fn runvil_zero_is_accepted() {
    // `runvil: 0` is the active Engine major: it parses and is exposed on
    // the frontmatter.
    let src = "---\nname: x\ndescription: d\nrunvil: 0\n---\n\n# T\n\n## S\n\np\n";
    assert_eq!(parse_ok(src).frontmatter().runvil(), Some(0));
}

#[test]
fn a_key_line_is_the_one_column_0_line_that_declares_the_key() {
    // The scan reads lines, not YAML, so this frontmatter need not decode.
    let src = concat!(
        "---\n",
        "name: x\n",                 // 2: plain
        "\"description\": |\n",      // 3: double-quoted
        "  uses: [block scalar]\n",  // 4: indented block scalar text
        "  runvil: 1\n",             // 5: indented block scalar text
        "'runvil' \t: 0\n",          // 6: single-quoted, spaces and a tab
        "# output: x\n",             // 7: a comment
        "args:\n",                   // 8: the colon ends the line
        "input:x\n",                 // 9: no space after the colon
        "x-team.owner: a: b\n",      // 10: any key, up to the first such colon
        "models: {}\n",              // 11: declared twice
        "models: {}\n",              // 12: declared twice
        "max_tool_iterations : 3\n", // 13: a space before the colon
        "- uses: [web]\n",           // 14: a sequence entry
        "---\n\n# T\n",
    );
    let split = crate::frontmatter::split_frontmatter(src).expect("the frontmatter splits");
    let lines: BTreeMap<&str, u32> = split
        .key_lines
        .iter()
        .map(|(key, line)| (key.as_str(), line.get()))
        .collect();
    assert_eq!(
        lines,
        BTreeMap::from([
            ("args", 8),
            ("description", 3),
            ("max_tool_iterations", 13),
            ("name", 2),
            ("runvil", 6),
            ("x-team.owner", 10),
        ])
    );
}

#[test]
fn a_parsed_prompt_gives_the_engine_the_line_of_each_key_it_declares() {
    let prompt = parse_ok("---\nname: x\ndescription: d\nrunvil: 0\nuses: [web]\n---\n\n# T\n");
    let line = |key| crate::detail::key_line(&prompt, key).map(NonZeroU32::get);
    assert_eq!((line("runvil"), line("uses")), (Some(4), Some(5)));
    assert_eq!(line("models"), None, "an absent key has no line");
}
