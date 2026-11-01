//! The H1 pass on the scheduler. Each submodule holds one topic and
//! reaches the scenario below through `use super::*`.

use super::*;

/// A run of `source` for a scheduler H1 test: under the test model, which
/// fills all five roles.
fn h1_scenario(source: &str) -> Scenario {
    Scenario::new(source).bound()
}

mod control_flow;
mod pass;
mod returns_and_prose;
