//! Grep's `type` filter: a short name for a set of file extensions.

/// Each type name beside the extensions it keeps, without the dot.
const TYPES: [(&str, &[&str]); 16] = [
    ("js", &["js", "mjs", "cjs", "jsx"]),
    ("ts", &["ts", "tsx", "mts", "cts"]),
    ("py", &["py", "pyi"]),
    ("rust", &["rs"]),
    ("go", &["go"]),
    ("java", &["java"]),
    ("c", &["c", "h"]),
    ("cpp", &["cc", "cpp", "cxx", "hh", "hpp", "hxx", "h"]),
    ("cs", &["cs"]),
    ("rb", &["rb"]),
    ("php", &["php"]),
    ("md", &["md", "markdown"]),
    ("json", &["json"]),
    ("yaml", &["yaml", "yml"]),
    ("toml", &["toml"]),
    ("html", &["html", "htm"]),
];

/// The extensions of the type `name`, or `None` when no type has that
/// name. Names are case-sensitive.
pub(super) fn extensions(name: &str) -> Option<&'static [&'static str]> {
    TYPES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, extensions)| *extensions)
}

/// The extension of `file_name`: the text after its last `.`, unless that
/// dot is the first character, so `a.js` has `js` and `.js` has none.
fn extension(file_name: &str) -> Option<&str> {
    let dot = file_name.rfind('.')?;
    if dot == 0 {
        return None;
    }
    file_name.get(dot + 1..)
}

/// Whether the file called `file_name` has one of `extensions`, compared
/// case-sensitively.
pub(super) fn matches(extensions: &[&str], file_name: &str) -> bool {
    extension(file_name).is_some_and(|found| extensions.contains(&found))
}

#[cfg(test)]
mod tests;
