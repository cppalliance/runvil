//! Matches items across crates. Rustdoc ids are only meaningful inside the
//! JSON that assigned them, and the facade's JSON names a re-exported item
//! by its defining crate and its definition path (`runvil_runtime::
//! execute::run::Run`, private modules included), so every cross-crate
//! match here goes by crate name and path, never by id.

use std::collections::{BTreeSet, HashMap};

use rustdoc_types::{Crate, Id, ItemEnum, ItemKind, Use};

use super::Finding;
use super::load::Loaded;

/// Crates any surface item may mention.
const STD_CRATES: [&str; 3] = ["std", "core", "alloc"];

/// Third-party crates the surface may mention: serde's traits are how
/// dependents serialize the surface's types, and `serde_json` and
/// `serde_yaml_ng` supply the values some of those types carry. `serde`
/// defines its traits in `serde_core` and re-exports them, and rustdoc
/// names the defining crate.
const ALLOWLIST: [&str; 4] = ["serde", "serde_core", "serde_json", "serde_yaml_ng"];

/// What a closure mention must be, naming every crate it may come from:
/// the shared ones, then the crates in `allowed`, which one facade may
/// also mention.
pub(super) fn required_with(allowed: &[&str]) -> String {
    let mut crates = ALLOWLIST.to_vec();
    crates.extend(allowed);
    format!(
        "a facade re-export, {}, or an allowlisted crate ({})",
        STD_CRATES.join(", "),
        crates.join(", ")
    )
}

/// Where an id points, as the JSON that holds the id resolves it.
#[derive(Debug)]
pub(super) struct Target<'a> {
    /// The defining crate's name.
    pub(super) krate: &'a str,
    /// The definition path, starting with the crate name.
    pub(super) path: &'a [String],
    pub(super) kind: ItemKind,
}

/// The name of crate `crate_id` as `krate` numbers crates; 0 is `krate`.
fn crate_name(krate: &Crate, crate_id: u32) -> Option<&str> {
    if crate_id == 0 {
        krate.index.get(&krate.root)?.name.as_deref()
    } else {
        Some(krate.external_crates.get(&crate_id)?.name.as_str())
    }
}

/// Resolves `id` through `krate`'s path table.
pub(super) fn target(krate: &Crate, id: Id) -> Option<Target<'_>> {
    let summary = krate.paths.get(&id)?;
    Some(Target {
        krate: crate_name(krate, summary.crate_id)?,
        path: &summary.path,
        kind: summary.kind,
    })
}

/// One facade re-export, resolved to its defining item.
#[derive(Debug)]
pub(super) struct Entry {
    /// The facade path dependents name it by (`runvil::model::Message`).
    pub(super) label: String,
    /// The defining internal crate's name.
    pub(super) krate: String,
    /// The item's id in the defining crate's JSON, present in its index.
    pub(super) id: Id,
}

/// The facade's surface: its re-exports and its own modules.
#[derive(Debug, Default)]
pub(super) struct Surface {
    pub(super) entries: Vec<Entry>,
    /// Each facade module (the crate root first) and its id in the
    /// facade's JSON.
    pub(super) modules: Vec<(String, Id)>,
    /// The facade's crate name, which roots every facade path.
    pub(super) root: String,
    /// The names of the crates that may define a re-exported item: the
    /// container crates and the public crates the facade passes on.
    pub(super) internal: BTreeSet<String>,
    /// The container crates' names, which surface docs may not hold.
    pub(super) private: BTreeSet<String>,
    /// The crates the surface may mention beside the shared allowlist.
    allowed: &'static [&'static str],
    /// Defining crate, then definition path, to facade path.
    facade_paths: HashMap<String, HashMap<Vec<String>, String>>,
}

impl Surface {
    /// Walks the facade's modules from the crate root and resolves every
    /// re-export, returning a finding for each one that is not a single
    /// item defined in an internal crate.
    pub(super) fn resolve(loaded: &Loaded) -> (Surface, Vec<Finding>) {
        let mut resolver = Resolver {
            loaded,
            local: loaded
                .internal
                .iter()
                .map(|(name, krate)| (name.as_str(), local_paths(krate)))
                .collect(),
            surface: Surface {
                root: loaded.name.clone(),
                internal: loaded.internal.keys().cloned().collect(),
                private: loaded.private.clone(),
                allowed: loaded.allowed,
                ..Surface::default()
            },
            uses: Vec::new(),
            findings: Vec::new(),
        };
        resolver.module(loaded.facade.root, &loaded.name);
        let mut uses = std::mem::take(&mut resolver.uses);
        uses.sort_by(|a, b| a.0.cmp(&b.0));
        for (label, reexport) in uses {
            resolver.reexport(label, reexport);
        }
        (resolver.surface, resolver.findings)
    }

    /// The facade path of the item defined at `path` in crate `krate`.
    pub(super) fn facade_path(&self, krate: &str, path: &[String]) -> Option<&str> {
        self.facade_paths.get(krate)?.get(path).map(String::as_str)
    }

    /// What a closure mention must be, naming every crate it may come
    /// from.
    pub(super) fn required(&self) -> String {
        required_with(self.allowed)
    }

    /// Why mentioning the item at `path` in `krate` breaks closure, or
    /// `None` when the facade re-exports it or its crate is allowed.
    pub(super) fn refusal(&self, krate: &str, path: &[String]) -> Option<String> {
        if krate == self.root
            || STD_CRATES.contains(&krate)
            || ALLOWLIST.contains(&krate)
            || self.allowed.contains(&krate)
            || self.facade_path(krate, path).is_some()
        {
            None
        } else if self.private.contains(krate) {
            Some(format!(
                "an item of internal crate `{krate}` the facade does not re-export"
            ))
        } else if self.internal.contains(krate) {
            Some(format!(
                "an item of public crate `{krate}` the facade does not re-export"
            ))
        } else {
            Some(format!(
                "an item of crate `{krate}`, which is not on the allowlist"
            ))
        }
    }
}

/// Every item `krate` defines and documents, by definition path. The path
/// table and the index are separate tables, so an id the index lacks is
/// left out: a re-export of it is refused rather than walked as nothing.
fn local_paths(krate: &Crate) -> HashMap<&[String], Id> {
    krate
        .paths
        .iter()
        .filter(|(id, summary)| summary.crate_id == 0 && krate.index.contains_key(id))
        .map(|(id, summary)| (summary.path.as_slice(), *id))
        .collect()
}

struct Resolver<'a> {
    loaded: &'a Loaded,
    local: HashMap<&'a str, HashMap<&'a [String], Id>>,
    surface: Surface,
    /// Every facade `use` and its facade path, resolved in path order
    /// once the walk is done: rustdoc lists a module's items in no fixed
    /// order, and the first path an item is re-exported at is the one a
    /// second path is reported against.
    uses: Vec<(String, &'a Use)>,
    findings: Vec<Finding>,
}

impl<'a> Resolver<'a> {
    fn module(&mut self, id: Id, label: &str) {
        let loaded = self.loaded;
        let facade = &loaded.facade;
        let Some(ItemEnum::Module(module)) = facade.index.get(&id).map(|item| &item.inner) else {
            return;
        };
        self.surface.modules.push((label.to_owned(), id));
        for child in &module.items {
            let Some(item) = facade.index.get(child) else {
                continue;
            };
            let name = item.name.as_deref().unwrap_or("_");
            match &item.inner {
                ItemEnum::Module(_) => self.module(*child, &format!("{label}::{name}")),
                ItemEnum::Use(reexport) => self
                    .uses
                    .push((format!("{label}::{}", reexport.name), reexport)),
                _ => {}
            }
        }
    }

    fn reexport(&mut self, label: String, reexport: &'a Use) {
        let loaded = self.loaded;
        let Some(target) = reexport.id.and_then(|id| target(&loaded.facade, id)) else {
            return self.refuse(
                label,
                &reexport.source,
                "a re-export rustdoc resolves",
                "none",
            );
        };
        let path = target.path.join("::");
        if reexport.is_glob {
            return self.refuse(label, &path, "a single-item re-export", "a glob");
        }
        if target.kind == ItemKind::Module {
            return self.refuse(label, &path, "a single-item re-export", "a module");
        }
        let Some(id) = self
            .local
            .get(target.krate)
            .and_then(|paths| paths.get(target.path))
            .copied()
        else {
            let found = if loaded.internal.contains_key(target.krate) {
                "no item at that path in its crate's rustdoc JSON".to_owned()
            } else {
                format!("an item of crate `{}`", target.krate)
            };
            // A facade that passes on public crates may re-export their
            // items too.
            let required = if loaded.internal.len() > loaded.private.len() {
                "an item defined in an internal or public crate"
            } else {
                "an item defined in an internal crate"
            };
            return self.refuse(label, &path, required, &found);
        };
        let paths = self
            .surface
            .facade_paths
            .entry(target.krate.to_owned())
            .or_default();
        if let Some(first) = paths.get(target.path).cloned() {
            let found = format!("a second facade path beside `{first}`");
            return self.refuse(label, &path, "exactly one facade path per item", &found);
        }
        paths.insert(target.path.to_vec(), label.clone());
        self.surface.entries.push(Entry {
            label,
            krate: target.krate.to_owned(),
            id,
        });
    }

    fn refuse(&mut self, label: String, path: &str, required: &str, found: &str) {
        self.findings.push(Finding {
            item: label,
            mention: format!("`{path}` as its re-export target"),
            required: required.to_owned(),
            found: found.to_owned(),
        });
    }
}

#[cfg(test)]
#[path = "items-tests.rs"]
mod tests;
