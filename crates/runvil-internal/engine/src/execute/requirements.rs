//! The preflight report: [`Requirements`].

use runvil_types::packs::PackId;

#[cfg(test)]
#[path = "requirements-tests.rs"]
mod tests;

/// A report of what the caller must still satisfy before a prompt can run.
///
/// [`RunContext::prepare`](super::RunContext::prepare) returns one
/// together with the prepared context. The report lists only what needs a
/// person's attention.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Requirements {
    /// The model requirements that the bound models fall short of.
    ///
    /// Each entry names the role, the check that failed, and what was
    /// required versus what the model provides. Examples are a
    /// `min_context` of 200000 against a model with a 32k context, and
    /// `thinking` against a model whose thinking mode is `Never`. Only
    /// model binding in `RunContext::prepare` adds these.
    pub unmet_requirements: Vec<UnmetRequirement>,
    /// The required Packs that the run lacks.
    ///
    /// The caller adds a Pack the prompt declares that is not installed.
    /// The run fails until every one is satisfied.
    pub missing_required: Vec<PackId>,
    /// The required Packs that are installed but need a service
    /// the run lacks.
    ///
    /// There is one entry for each Pack and missing service. The caller
    /// adds these and leaves each such Pack's tools out of the catalog.
    /// The run fails until the application provides the service.
    pub missing_services: Vec<MissingService>,
    /// The required Packs that are installed but cannot serve, each
    /// with the reason, such as a failure to build.
    ///
    /// The caller adds these. The run fails until the Pack can serve.
    pub unavailable: Vec<UnavailablePack>,
    /// The reserved Pack ids, such as `adhoc`, that the run's tool catalog
    /// or Pack roster names, sorted, each once.
    ///
    /// The Engine holds the tools under a reserved Pack id itself, so no
    /// Pack may take one.
    /// [`RunContext::prepare`](super::RunContext::prepare) fills it. The
    /// run fails while it holds any id.
    pub reserved: Vec<PackId>,
}

impl Requirements {
    /// Returns whether the report lets the run proceed.
    ///
    /// That holds when every model requirement is met, every required
    /// Pack is present, can serve, and has the services it needs, and
    /// nothing in the run takes a reserved Pack id.
    #[must_use]
    pub fn is_satisfied(&self) -> bool {
        self.unmet_requirements.is_empty()
            && self.missing_required.is_empty()
            && self.missing_services.is_empty()
            && self.unavailable.is_empty()
            && self.reserved.is_empty()
    }

    /// Adds the entries of `other` to this report.
    ///
    /// Use it to combine the caller's Pack report with the report
    /// from `RunContext::prepare`, so that one refusal names every gap.
    /// The merge skips an entry the report already holds, and keeps
    /// `reserved` the union of both reports' ids, sorted, each once.
    ///
    /// The merge drops the `missing_required` entry of a Pack that
    /// lacks a service or is unavailable: the service or unavailable entry
    /// already names the real cause.
    pub fn merge(&mut self, other: Requirements) {
        push_new(&mut self.missing_required, other.missing_required);
        push_new(&mut self.missing_services, other.missing_services);
        push_new(&mut self.unavailable, other.unavailable);
        self.reserved.extend(other.reserved);
        self.reserved.sort();
        self.reserved.dedup();
        let (services, unavailable) = (&self.missing_services, &self.unavailable);
        self.missing_required.retain(|id| {
            !services.iter().any(|missing| missing.pack == *id)
                && !unavailable.iter().any(|entry| entry.pack == *id)
        });
        self.unmet_requirements.extend(other.unmet_requirements);
    }

    /// Returns the error to fail the run with, or `None` when the report
    /// is satisfied.
    ///
    /// The error is a [`RunError`](super::RunError) of kind
    /// [`RequirementsUnmet`](super::RunErrorKind::RequirementsUnmet) that
    /// reports the [`notice`](Requirements::notice). The caller checks it
    /// after merging every report into one, and before building the run.
    #[must_use]
    pub fn refusal(&self) -> Option<super::RunError> {
        (!self.is_satisfied()).then(|| {
            super::RunError::from(crate::Error::RequirementsUnmet {
                notice: self.notice(),
                cause: None,
            })
        })
    }

    /// Returns the refusal notice, which explains what blocks the run.
    ///
    /// Each line names what is missing or falls short, with required versus
    /// actual. The notice is written for a model to read: concise, factual,
    /// and self-contained.
    #[must_use]
    pub fn notice(&self) -> String {
        // Writing to a String is infallible, so each `write!` result is
        // discarded under the denied `unwrap_used`/`expect_used` lints.
        use std::fmt::Write as _;
        let mut notice = String::from("the environment cannot satisfy this prompt:");
        for id in &self.missing_required {
            let _ = write!(notice, "\n- missing required Pack: {id}");
        }
        for missing in &self.missing_services {
            let _ = write!(
                notice,
                "\n- {} needs {}, and the environment provides none",
                missing.pack, missing.service
            );
        }
        for entry in &self.unavailable {
            let _ = write!(
                notice,
                "\n- {} is unavailable: {}",
                entry.pack, entry.reason
            );
        }
        for id in &self.reserved {
            let _ = write!(
                notice,
                "\n- the run's tool catalog or Pack roster names the Pack id {id}, \
                 which the Engine reserves"
            );
        }
        for unmet in &self.unmet_requirements {
            let _ = match unmet.check {
                RequirementCheck::ContextMinimum => write!(
                    notice,
                    "\n- role '{}': requires a context of at least {} tokens; \
                     the current model provides {}",
                    unmet.role, unmet.required, unmet.actual
                ),
                RequirementCheck::HardKeyword => write!(
                    notice,
                    "\n- role '{}': requires '{}'; \
                     the current model's thinking capability is {}",
                    unmet.role, unmet.required, unmet.actual
                ),
            };
        }
        notice
    }
}

/// Appends each entry of `incoming` that `entries` does not already hold.
fn push_new<T: PartialEq>(entries: &mut Vec<T>, incoming: Vec<T>) {
    for entry in incoming {
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
}

/// An installed Pack that can serve the run, and its vendor.
///
/// The run's roster, set with
/// [`RunContext::packs`](super::RunContext::packs), lists every one the
/// run holds. The caller reports each Pack's vendor as it picked the
/// Pack, and every tool-call and tool-result event for one of the Pack's
/// tools records it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UsablePack {
    /// The installed Pack.
    pub pack: PackId,
    /// The vendor the caller picked the Pack from, such as `runvil` or
    /// `acme`.
    pub vendor: String,
}

impl UsablePack {
    /// Creates an entry stating that `pack`, from `vendor`, can serve.
    ///
    /// Only the caller builds these, for its roster.
    #[must_use]
    pub fn new(pack: PackId, vendor: impl Into<String>) -> UsablePack {
        UsablePack {
            pack,
            vendor: vendor.into(),
        }
    }
}

/// An installed Pack that cannot serve the run, and why.
///
/// The caller's Pack report lists each one the prompt declares, and the
/// run's roster, set with
/// [`RunContext::packs`](super::RunContext::packs), lists every one the
/// run holds.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnavailablePack {
    /// The installed Pack.
    pub pack: PackId,
    /// Why it cannot serve, written for the model that reads the refusal.
    pub reason: String,
}

impl UnavailablePack {
    /// Creates an entry stating that `pack` cannot serve, because of
    /// `reason`.
    ///
    /// Only the caller builds these, for its Pack report and its roster.
    #[must_use]
    pub fn new(pack: PackId, reason: impl Into<String>) -> UnavailablePack {
        UnavailablePack {
            pack,
            reason: reason.into(),
        }
    }
}

/// A service that a required Pack needs and the application lacks.
///
/// For example, a Pack that asks the operator a question needs a
/// service that reaches the operator, and a batch application runs on its
/// own, so it lacks that service.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MissingService {
    /// The installed, required Pack that needs the service.
    pub pack: PackId,
    /// The name of the missing service.
    ///
    /// The caller defines its own service names, so the report carries
    /// the name verbatim.
    pub service: String,
}

impl MissingService {
    /// Creates an entry stating that `pack` needs the service named
    /// `service`, which the application lacks.
    ///
    /// Only the caller's Pack report holds these.
    #[must_use]
    pub fn new(pack: PackId, service: impl Into<String>) -> MissingService {
        MissingService {
            pack,
            service: service.into(),
        }
    }
}

/// A model requirement that the model bound to a role falls short of.
///
/// It names the role, the check that failed, and what the prompt required
/// versus what the bound model provides.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnmetRequirement {
    /// The declared role label whose requirement failed.
    pub role: String,
    /// Which requirement check failed.
    pub check: RequirementCheck,
    /// What the prompt required, such as a context minimum of `200000` or
    /// the `thinking` keyword.
    pub required: String,
    /// What the bound model provides, such as a context of `32000` or a
    /// thinking mode of `Never`.
    pub actual: String,
}

/// Which model requirement check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequirementCheck {
    /// The role's context minimum exceeds the context of the model bound
    /// to the role.
    ContextMinimum,
    /// The role declares a hard keyword, `thinking` or `no-thinking`, that
    /// the bound model's info fails to satisfy.
    HardKeyword,
}
