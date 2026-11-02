//! Parses the `models` frontmatter key into the requirements a prompt puts on
//! its model roles.
//!
//! A prompt has five fixed roles, so the key is optional and only attaches
//! requirements to them: a minimum context window and a thinking switch.
//! The run's first step checks each requirement against the model that
//! fills the role.
//! Parse only validates shape: a role name outside the five, a field outside
//! the two, a repeated role, and a zero context minimum are errors.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use runvil_types::models::ModelRole;
use serde::Deserialize;

use crate::contract;

/// The requirements a prompt puts on one model role.
///
/// `min_context` is the smallest context window, in tokens, the model that
/// fills the role may have. `thinking: true` requires a model that can think
/// and turns thinking on for the role's requests, and `thinking: false`
/// requires a model that can run without thinking and turns it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRequirements {
    /// The minimum context window in tokens.
    #[serde(default)]
    min_context: Option<NonZeroU32>,
    /// The thinking requirement.
    #[serde(default)]
    thinking: Option<bool>,
}

impl RoleRequirements {
    /// Returns the minimum context window in tokens, when declared.
    #[must_use]
    pub fn min_context(&self) -> Option<NonZeroU32> {
        self.min_context
    }

    /// Returns the thinking requirement, when declared: `Some(true)` for a
    /// model that can think, `Some(false)` for one that can run without
    /// thinking.
    #[must_use]
    pub fn thinking(&self) -> Option<bool> {
        self.thinking
    }
}

/// The requirements a prompt declares under the `models` frontmatter key,
/// keyed by the role they apply to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RoleMap {
    roles: BTreeMap<ModelRole, RoleRequirements>,
}

impl RoleMap {
    /// Returns the requirements declared for `role`, when present.
    pub(crate) fn get(&self, role: ModelRole) -> Option<&RoleRequirements> {
        self.roles.get(&role)
    }
}

impl<'de> Deserialize<'de> for RoleMap {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let roles = contract::deserialize_contract_map(deserializer, "model role")?;
        Ok(RoleMap { roles })
    }
}
