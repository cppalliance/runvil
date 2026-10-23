//! Tests for the `uses` key: bare Pack ids and vendor/name entries, the
//! refused entry forms, and duplicates.

use super::*;

#[test]
fn a_vendor_name_entry_names_its_vendor_and_its_pack_id() {
    let prompt = parse_ok(&prompt("uses: [acme/poop]", &[]));
    let entries = prompt.frontmatter().uses();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].vendor(), "acme");
    assert_eq!(entries[0].id().to_string(), "poop");
}

#[test]
fn a_bare_entry_means_the_runvil_pack_of_that_id() {
    let prompt = parse_ok(&prompt("uses: [web, runvil/mcp]", &[]));
    let entries = prompt.frontmatter().uses();
    let read: Vec<(&str, String)> = entries
        .iter()
        .map(|entry| (entry.vendor(), entry.id().to_string()))
        .collect();
    assert_eq!(
        read,
        [("runvil", "web".to_owned()), ("runvil", "mcp".to_owned())]
    );
}

#[test]
fn an_entry_of_three_or_more_segments_is_refused_with_the_uses_entry_reason() {
    for entry in ["a/b/c", "acme/web/extra/more"] {
        assert_parse_error(
            &prompt(&format!("uses: [{entry}]"), &[]),
            ParseErrorKind::Frontmatter,
            &format!(
                "invalid uses entry `{entry}`: a uses entry is a Pack id, such as `web`, \
                 or vendor/name, such as `acme/web`"
            ),
        );
    }
}

#[test]
fn an_entry_with_an_empty_segment_keeps_the_pack_id_reason() {
    for entry in ["web/", "runvil//web"] {
        assert_parse_error(
            &prompt(&format!("uses: [\"{entry}\"]"), &[]),
            ParseErrorKind::Frontmatter,
            &format!("invalid uses entry `{entry}`: segments must not be empty"),
        );
    }
}

#[test]
fn an_at_sign_in_an_entry_is_rejected_with_the_charset_reason() {
    // A Pack id carries no version, so `@` is a parse error.
    assert_parse_error(
        &prompt("uses: [web@1]", &[]),
        ParseErrorKind::Frontmatter,
        "invalid uses entry `web@1`: segments may contain only lowercase ASCII letters, \
         digits, '-', '_', '.'",
    );
}

#[test]
fn an_entry_with_uppercase_is_rejected() {
    for entry in ["Web", "Acme/web"] {
        let error = parse_err(&prompt(&format!("uses: [{entry}]"), &[]));
        assert_eq!(
            error.kind(),
            ParseErrorKind::Frontmatter,
            "{entry}: {error}"
        );
    }
}

#[test]
fn an_entry_that_is_not_a_string_is_refused_naming_both_entry_forms() {
    assert_parse_error(
        &prompt("uses: [42]", &[]),
        ParseErrorKind::Frontmatter,
        "expected a Pack id, such as `web`, or vendor/name, such as `acme/web`",
    );
}

#[test]
fn the_map_form_of_an_entry_is_refused_naming_both_entry_forms() {
    for entry in [
        "  - ref: web\n",
        "  - ref: web\n    optional: true\n",
        "  - ref: web\n    config: { depth: 1 }\n",
    ] {
        let src = format!("---\nname: x\ndescription: d\nuses:\n{entry}---\n\n# T\n\n## S\n\np\n");
        let error = parse_err(&src);
        assert_eq!(
            error.kind(),
            ParseErrorKind::Frontmatter,
            "{entry}: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("expected a Pack id, such as `web`, or vendor/name, such as `acme/web`"),
            "the refusal names both entry forms: {error}"
        );
        assert_eq!(
            error.line(),
            NonZeroU32::new(5),
            "the entry's own line: {error}"
        );
    }
}

#[test]
fn a_pack_id_declared_twice_is_refused() {
    for entries in [
        "[web, mcp, web]",
        "[web, web]",
        "[web, acme/web]",
        "[acme/web, zed/web]",
    ] {
        let error = parse_err(&prompt(&format!("uses: {entries}"), &[]));
        assert_eq!(
            error.kind(),
            ParseErrorKind::Frontmatter,
            "{entries}: {error}"
        );
        assert_eq!(
            error.to_string(),
            "invalid frontmatter: Pack id web is declared more than once under uses",
            "{entries}"
        );
    }
}

#[test]
fn the_adhoc_pack_id_is_refused_from_any_vendor() {
    for entries in ["[adhoc]", "[acme/adhoc]", "[web, runvil/adhoc]"] {
        let error = parse_err(&prompt(&format!("uses: {entries}"), &[]));
        assert_eq!(
            error.kind(),
            ParseErrorKind::Frontmatter,
            "{entries}: {error}"
        );
        assert_eq!(
            error.to_string(),
            "invalid frontmatter: adhoc holds a section's own tools and is never declared \
             under uses",
            "{entries}"
        );
    }
}

#[test]
fn a_list_of_distinct_pack_ids_still_parses() {
    let prompt = parse_ok(&prompt("uses: [web, web2, web-search, acme/poop]", &[]));
    assert_eq!(prompt.frontmatter().uses().len(), 4);
}

#[test]
fn entries_keep_their_declaration_order() {
    let prompt = parse_ok(&prompt("uses: [mcp, acme/poop, web]", &[]));
    let ids: Vec<String> = prompt
        .frontmatter()
        .uses()
        .iter()
        .map(|entry| entry.id().to_string())
        .collect();
    assert_eq!(ids, ["mcp", "poop", "web"]);
}

#[test]
fn the_old_plugins_key_is_refused_as_an_unknown_field() {
    let old_key = "plugins";
    let error = parse_err(&prompt(&format!("{old_key}: [web]"), &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter, "{error}");
    assert!(
        error.to_string().contains("unknown field `plugins`"),
        "the parser names the key it does not know: {error}"
    );
}
