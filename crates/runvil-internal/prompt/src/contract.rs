//! Parses the frontmatter contract keys: `uses`, `args`, and `models`.
//!
//! The YAML is the whole contract: Packs install, models declare, args
//! type. Parsing validates the static shape - each `uses:` entry a bare
//! Pack id or vendor/name, no Pack id declared twice, no reserved Pack id,
//! the alias grammar on arg names, the five model role names, and each arg
//! default against its declared type - and exposes the FULL declaration on
//! the parsed [`Prompt`](crate::Prompt); checking the declaration against
//! the caller's environment is the job of the run's first step, never the
//! parser's.
//!
//! `args` and `models` are defined in submodules; this root owns the `uses:`
//! checks, the arg-name check, and the map deserializer the two map keys
//! share.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use runvil_types::models::ModelRole;
use runvil_types::names::is_alias;
use runvil_types::packs::PackRef;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::error;

mod args;
mod models;

#[cfg(test)]
mod tests;

pub use args::{ArgDecl, ArgType, ArgsDecl, ArgsDeclIter};
pub(crate) use models::RoleMap;
pub use models::RoleRequirements;

/// A key of a contract map: how it is checked and how an error names it.
trait ContractKey: Ord {
    /// Refuses a key the map's grammar does not allow.
    fn check(&self, what: &str) -> Result<(), String>;

    /// The key as an error names it.
    fn shown(&self) -> String;
}

impl ContractKey for String {
    fn check(&self, what: &str) -> Result<(), String> {
        if is_alias(self) {
            Ok(())
        } else {
            Err(format!(
                "invalid {what} `{self}`: expected [A-Za-z][A-Za-z0-9_-]{{0,63}}"
            ))
        }
    }

    fn shown(&self) -> String {
        self.clone()
    }
}

impl ContractKey for ModelRole {
    /// A role name is checked while it deserializes, against the five names.
    fn check(&self, _what: &str) -> Result<(), String> {
        Ok(())
    }

    fn shown(&self) -> String {
        self.as_str().to_owned()
    }
}

/// Deserializes a contract map (`models`, `args`): keys of type `K` checked
/// by [`ContractKey`], values deserialized as `T`, duplicates rejected. `what`
/// names the keys in every error.
fn deserialize_contract_map<'de, D, K, T>(
    deserializer: D,
    what: &'static str,
) -> Result<BTreeMap<K, T>, D::Error>
where
    D: Deserializer<'de>,
    K: ContractKey + Deserialize<'de>,
    T: Deserialize<'de>,
{
    deserializer.deserialize_map(MapVisitor {
        what,
        _marker: PhantomData,
    })
}

/// The visitor behind [`deserialize_contract_map`]: a streaming map walk so
/// rejections keep their source position.
struct MapVisitor<K, T> {
    /// The key kind (`model role` or `arg name`), for error messages.
    what: &'static str,
    /// The key and value types, without ownership or variance claims.
    _marker: PhantomData<fn() -> (K, T)>,
}

impl<'de, K, T> Visitor<'de> for MapVisitor<K, T>
where
    K: ContractKey + Deserialize<'de>,
    T: Deserialize<'de>,
{
    type Value = BTreeMap<K, T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a map of {} keys to declarations", self.what)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let what = self.what;
        let mut entries: BTreeMap<K, T> = BTreeMap::new();
        while let Some(key) = map.next_key::<K>()? {
            key.check(what).map_err(de::Error::custom)?;
            match entries.entry(key) {
                Entry::Occupied(entry) => {
                    return Err(de::Error::custom(format!(
                        "duplicate {what} `{}`: contract map keys must be unique",
                        entry.key().shown()
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

/// Refuses a `uses:` list that names one Pack id twice, whatever the
/// vendors, because a run holds one Pack per Pack id.
///
/// The refusal names the first Pack id declared again, with the `Reason`
/// fact `pack_duplicate`.
pub(crate) fn check_distinct_packs(packs: &[PackRef]) -> crate::Result<()> {
    let mut seen = BTreeSet::new();
    match packs.iter().find(|pack| !seen.insert(pack.id())) {
        Some(pack) => Err(error::pack_refusal(
            "pack_duplicate",
            pack.id(),
            format!(
                "invalid frontmatter: Pack id {} is declared more than once under uses",
                pack.id()
            ),
        )),
        None => Ok(()),
    }
}

/// Refuses a `uses:` list that names a reserved Pack id, such as `adhoc`,
/// whatever the vendor, because the Engine holds the tools under it.
///
/// The refusal names the first reserved Pack id declared, with the
/// `Reason` fact `pack_reserved`.
pub(crate) fn check_no_reserved_pack(packs: &[PackRef]) -> crate::Result<()> {
    match packs.iter().find(|pack| pack.id().is_reserved()) {
        Some(pack) => Err(error::pack_refusal(
            "pack_reserved",
            pack.id(),
            format!(
                "invalid frontmatter: Pack id {} is reserved by the Engine and is never declared \
                 under uses",
                pack.id()
            ),
        )),
        None => Ok(()),
    }
}
