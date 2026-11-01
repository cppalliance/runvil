//! Unit tests for the `type` table and the extension rule.

use super::{extension, extensions, matches};

#[test]
fn the_extension_is_the_text_after_the_last_dot_unless_that_dot_is_first() {
    assert_eq!(extension("a.js"), Some("js"));
    assert_eq!(extension("a.b.js"), Some("js"));
    assert_eq!(extension(".js"), None);
    assert_eq!(extension("Makefile"), None);
    assert_eq!(extension("x."), Some(""));
}

#[test]
fn a_type_keeps_its_extensions_and_matching_is_case_sensitive() {
    let rust = extensions("rust").expect("rust is a type");
    assert!(matches(rust, "lib.rs"));
    assert!(!matches(rust, "lib.RS"));
    assert!(!matches(rust, "rs"));
    let cpp = extensions("cpp").expect("cpp is a type");
    assert!(matches(cpp, "a.h") && matches(cpp, "a.hpp"));
    assert!(extensions("Rust").is_none());
    assert!(extensions("foo").is_none());
}

#[test]
fn every_documented_type_is_known() {
    for name in [
        "js", "ts", "py", "rust", "go", "java", "c", "cpp", "cs", "rb", "php", "md", "json",
        "yaml", "toml", "html",
    ] {
        assert!(extensions(name).is_some(), "{name}");
    }
}
