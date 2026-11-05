//! What a run's params lack for its prompt: [`Requirements`], which the
//! run's first step reports as its refusal notice.

use std::collections::{BTreeMap, BTreeSet};

use runvil_types::error::place;
use runvil_types::models::ModelRole;
use runvil_types::packs::PackId;
use runvil_types::tools::ToolCatalog;

use crate::lua::RosterEntry;
use crate::prompt::Prompt;
use crate::{Error, error};

#[cfg(test)]
#[path = "requirements-tests.rs"]
mod tests;

/// The first line of every refusal notice.
pub(super) const NOTICE_HEADER: &str = "the environment cannot satisfy this prompt:";

/// What the run's params lack for its prompt, gathered when the run is
/// built. The run's first step fails with its notice unless it is empty.
#[derive(Debug, Default)]
pub(super) struct Requirements {
    /// The model requirements that the models filling the roles fall short
    /// of, and the `default` role a non-empty role map leaves unfilled.
    pub(super) unmet_models: Vec<UnmetRequirement>,
    /// The declared Packs the roster lacks, in declaration order.
    missing_packs: Vec<PackId>,
    /// The declared Packs the roster lists as unable to serve, each with
    /// its reason, in declaration order.
    unavailable_packs: Vec<(PackId, String)>,
    /// The reserved Pack ids, such as `adhoc`, that the run's tool catalog
    /// or Pack roster names. The Engine holds the tools under a reserved
    /// Pack id itself, so no Pack may take one.
    reserved: BTreeSet<PackId>,
}

impl Requirements {
    /// Checks each Pack `prompt` declares against `roster`, when the run
    /// has one, and records each reserved Pack id that a tool of `catalog`
    /// or a Pack of `roster`, usable or not, has.
    pub(super) fn check_packs(
        &mut self,
        prompt: &Prompt,
        catalog: &ToolCatalog,
        roster: Option<&BTreeMap<PackId, RosterEntry>>,
    ) {
        if let Some(roster) = roster {
            for entry in prompt.frontmatter().uses() {
                let id = entry.id().clone();
                match roster.get(&id) {
                    None => self.missing_packs.push(id),
                    Some(RosterEntry::Unavailable { reason }) => {
                        self.unavailable_packs.push((id, reason.clone()));
                    }
                    Some(RosterEntry::Usable { .. }) => {}
                }
            }
        }
        self.reserved = catalog
            .tools()
            .iter()
            .map(|tool| tool.id.pack())
            .chain(roster.into_iter().flat_map(BTreeMap::keys).cloned())
            .filter(PackId::is_reserved)
            .collect();
    }

    /// Returns the `RequirementsUnmet` failure the run's first step ends
    /// with, whose message and prose are the refusal notice, or `None` when
    /// nothing blocks the run.
    ///
    /// The failure is placed on the frontmatter key of the first gap in the
    /// notice that the prompt caused: `uses:` for a missing or unavailable
    /// Pack, and `models:` for a model that falls short of a role's
    /// requirement. A reserved Pack id and a role map with no `default`
    /// entry are the caller's gaps and point at no key, so a refusal made
    /// only of those has no location.
    pub(super) fn refusal(&self, prompt: &Prompt) -> Option<Error> {
        let notice = self.notice()?;
        let key = if self.missing_packs.is_empty() && self.unavailable_packs.is_empty() {
            self.unmet_models
                .iter()
                .any(|unmet| {
                    matches!(
                        unmet.check,
                        RequirementCheck::ContextMinimum | RequirementCheck::Thinking
                    )
                })
                .then_some("models")
        } else {
            Some("uses")
        };
        let line = key.and_then(|key| crate::prompt::detail::key_line(prompt, key));
        Some(place(
            error::requirements_unmet(&notice, None),
            prompt.file(),
            line,
        ))
    }

    /// Returns the refusal notice, which explains what blocks the run, or
    /// `None` when nothing does.
    ///
    /// Each line names what is missing or falls short. The model lines
    /// carry required versus actual; the Pack lines name the Pack and, for
    /// an unavailable Pack, the reason. The notice is written for a model
    /// to read: concise, factual, and self-contained.
    fn notice(&self) -> Option<String> {
        // Writing to a String is infallible, so each `write!` result is
        // discarded under the denied `unwrap_used`/`expect_used` lints.
        use std::fmt::Write as _;
        if self.unmet_models.is_empty()
            && self.missing_packs.is_empty()
            && self.unavailable_packs.is_empty()
            && self.reserved.is_empty()
        {
            return None;
        }
        let mut notice = String::from(NOTICE_HEADER);
        for id in &self.missing_packs {
            let _ = write!(notice, "\n- missing required Pack: {id}");
        }
        for (id, reason) in &self.unavailable_packs {
            let _ = write!(notice, "\n- {id} is unavailable: {reason}");
        }
        for id in &self.reserved {
            let _ = write!(
                notice,
                "\n- the run's tool catalog or Pack roster names the Pack id {id}, \
                 which the Engine reserves"
            );
        }
        for unmet in &self.unmet_models {
            let role = unmet.role.as_str();
            let _ = match unmet.check {
                RequirementCheck::ContextMinimum => write!(
                    notice,
                    "\n- role '{role}': requires a context of at least {} tokens; \
                     the model filling it provides {}",
                    unmet.required, unmet.actual
                ),
                RequirementCheck::Thinking => write!(
                    notice,
                    "\n- role '{role}': requires '{}'; \
                     the thinking capability of the model filling it is {}",
                    unmet.required, unmet.actual
                ),
                RequirementCheck::Unbound => write!(
                    notice,
                    "\n- role '{role}': requires {}; \
                     the caller's role map has no '{role}' entry",
                    unmet.required
                ),
            };
        }
        Some(notice)
    }
}

/// A model requirement that the model filling a role falls short of, or a
/// role the caller left without a model: the role, the check that failed,
/// and what the prompt required versus what the caller provided.
#[derive(Debug)]
pub(super) struct UnmetRequirement {
    /// The role whose requirement failed.
    pub(super) role: ModelRole,
    /// Which requirement check failed.
    pub(super) check: RequirementCheck,
    /// What the prompt required, such as a context minimum of `200000`, a
    /// thinking switch of `thinking: true`, or `a model`.
    pub(super) required: String,
    /// What the filling model provides, such as a context of `32000` or a
    /// thinking mode of `Never`, or `none` when the caller gave the role no
    /// model.
    pub(super) actual: String,
}

/// Which model requirement check failed.
#[derive(Debug, Clone, Copy)]
pub(super) enum RequirementCheck {
    /// The role's context minimum exceeds the context of the model that
    /// fills the role.
    ContextMinimum,
    /// The role's thinking requirement, `thinking: true` or
    /// `thinking: false`, is one the filling model's info fails to satisfy.
    Thinking,
    /// The caller's role map holds entries but none for the `default`
    /// role, which every role without its own entry falls back to.
    Unbound,
}
