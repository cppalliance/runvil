//! Tests for the `uses` key: Pack ids, entry forms, and duplicates.

use super::*;

#[test]
fn a_pack_id_must_be_exactly_one_segment() {
    for id in ["runvil/web", "web/fetch", "web/", "runvil//web"] {
        let error = parse_err(&prompt(&format!("uses: [{id}]"), &[]));
        assert_eq!(error.kind(), ParseErrorKind::Frontmatter, "{id}: {error}");
    }
    assert_parse_error(
        &prompt("uses: [runvil/web]", &[]),
        ParseErrorKind::Frontmatter,
        "invalid Pack id `runvil/web`: a Pack id is one segment, such as `web`, with no '/'",
    );
}

#[test]
fn an_at_sign_in_a_pack_id_is_rejected() {
    // A Pack id carries no version, so `@` is a parse error.
    let error = parse_err(&prompt("uses: [web@1]", &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
}

#[test]
fn a_pack_id_with_uppercase_is_rejected() {
    let error = parse_err(&prompt("uses: [Web]", &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
}

#[test]
fn a_pack_entry_must_be_a_plain_name() {
    let error = parse_err(&prompt("uses: [42]", &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
}

#[test]
fn the_map_form_of_a_pack_entry_is_refused_naming_the_plain_name_form() {
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
                .contains("expected a Pack's plain name, such as `web`"),
            "the refusal names the plain-name form: {error}"
        );
        assert_eq!(
            error.line(),
            NonZeroU32::new(5),
            "the entry's own line: {error}"
        );
    }
}

#[test]
fn a_pack_declared_twice_is_refused() {
    for entries in ["[web, mcp, web]", "[web, web]"] {
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
fn a_list_of_distinct_packs_still_parses() {
    let prompt = parse_ok(&prompt("uses: [web, web2, web-search]", &[]));
    assert_eq!(prompt.frontmatter().uses().len(), 3);
}

#[test]
fn the_uses_key_takes_plain_names() {
    let prompt = parse_ok(&prompt("uses: [web, mcp]", &[]));
    let packs = prompt.frontmatter().uses();
    assert_eq!(packs.len(), 2);
    assert_eq!(packs[0].to_string(), "web");
    assert_eq!(packs[1].to_string(), "mcp");
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
