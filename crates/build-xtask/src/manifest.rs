//! Shared walk over a parsed `Cargo.toml`: the dependency tables of the
//! requested kinds, both at the top level and under `[target.<cfg>]`.
//!
//! Every structural check that reads declared dependencies goes through
//! this one walk and names the kinds it asks for, as the product matrix
//! does.

/// Every dependency table of the given kinds, labeled the way its section
/// header reads: the plain `<kind>` tables and their `[target.<cfg>.<kind>]`
/// forms, in manifest order. Kinds not present in the manifest are skipped.
pub(crate) fn dependency_tables<'a>(
    manifest: &'a toml::Value,
    kinds: &[&str],
) -> Vec<(String, &'a toml::map::Map<String, toml::Value>)> {
    let mut tables = Vec::new();
    for kind in kinds {
        if let Some(table) = manifest.get(kind).and_then(toml::Value::as_table) {
            tables.push(((*kind).to_owned(), table));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for (target, value) in targets {
            for kind in kinds {
                if let Some(table) = value.get(kind).and_then(toml::Value::as_table) {
                    tables.push((format!("target.'{target}'.{kind}"), table));
                }
            }
        }
    }
    tables
}
