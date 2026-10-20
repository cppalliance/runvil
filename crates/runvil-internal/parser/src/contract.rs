//! Parses the frontmatter contract keys: `uses`, `args`, and `models`.
//!
//! The YAML is the whole contract: Packs install, models declare, args
//! type. Parsing validates the static shape - each Pack a one-segment
//! plain name declared once, the alias grammar on map keys, the closed
//! model-keyword vocabulary, arg name and type sanity - and exposes the
//! FULL declaration on the parsed [`Prompt`](crate::Prompt); satisfying
//! the declaration against the caller's environment is prepare's job,
//! never the parser's.
//!
//! `args` and `models` are defined in submodules; this root owns the
//! duplicate-Pack check and the map deserializer the two map keys share.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use runvil_types::packs::PackId;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

mod args;
mod models;

#[cfg(test)]
mod tests;

pub use args::{ArgDecl, ArgType, ArgsDecl, ArgsDeclIter};
pub use models::{ModelKeyword, ModelRole, ModelRoles, ModelRolesIter};

/// The prompt-local alias grammar: `[A-Za-z][A-Za-z0-9_-]{0,63}`.
///
/// Model labels and args field names are prompt-local and never global
/// names. (The same rule sits in `runvil-lua`'s `alias` module, the
/// run-time counterpart to this parse-time check.)
fn is_valid_alias(alias: &str) -> bool {
    let bytes = alias.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// How one contract map's keys are described in its errors.
#[derive(Clone, Copy)]
struct ContractKeys {
    /// The key kind (`model role label`), for error messages.
    what: &'static str,
}

/// Deserializes a contract map (`models`, `args`): string keys
/// validated against the alias grammar and `keys`, values deserialized as
/// `T`, duplicates rejected.
fn deserialize_contract_map<'de, D, T>(
    deserializer: D,
    keys: ContractKeys,
) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserializer.deserialize_map(MapVisitor {
        keys,
        _marker: PhantomData,
    })
}

/// The visitor behind [`deserialize_contract_map`]: a streaming map walk so
/// rejections keep their source position.
struct MapVisitor<T> {
    /// The checks the map's keys get.
    keys: ContractKeys,
    /// The value type, without ownership or variance claims.
    _marker: PhantomData<fn() -> T>,
}

impl<'de, T> Visitor<'de> for MapVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = BTreeMap<String, T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a map of {} keys to declarations",
            self.keys.what
        )
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let ContractKeys { what } = self.keys;
        let mut entries: BTreeMap<String, T> = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if !is_valid_alias(&key) {
                return Err(de::Error::custom(format!(
                    "invalid {what} `{key}`: expected [A-Za-z][A-Za-z0-9_-]{{0,63}}"
                )));
            }
            match entries.entry(key) {
                Entry::Occupied(entry) => {
                    return Err(de::Error::custom(format!(
                        "duplicate {what} `{}`: contract map keys must be unique",
                        entry.key()
                    )));
                }
                Entry::Vacant(entry) => {
                    entry.insert(map.next_value::<T>()?);
                }
            }
        }
        Ok(entries)
    }
}

/// A refused `uses:` list.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PackListError {
    /// The list names this Pack more than once.
    #[error("invalid frontmatter: Pack id {0} is declared more than once under uses")]
    Duplicate(PackId),
}

/// Refuses a `uses:` list that names one Pack twice.
///
/// The refusal names the first Pack declared again.
pub(crate) fn check_distinct_packs(packs: &[PackId]) -> Result<(), PackListError> {
    let mut seen = BTreeSet::new();
    match packs.iter().find(|pack| !seen.insert(*pack)) {
        Some(pack) => Err(PackListError::Duplicate(pack.clone())),
        None => Ok(()),
    }
}
