//! Integration tests for `plugin-user-input`: the `PackInfo`, the ask
//! tool `construct` names under the installed name, the answers a call
//! returns from the run's broker, and the prelude.
#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]

mod ask;
