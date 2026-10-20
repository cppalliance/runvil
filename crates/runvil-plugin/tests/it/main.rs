//! Plugin contract suite: the contract as a Plugin crate sees it, through
//! this crate's public items alone - a fixture `PackInfo` and `Pack`
//! built and called the way the Harness does, and the `TestCall` that
//! lends a test's call its context.
#![cfg(feature = "test-support")]
#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]

mod pack_info;
mod test_call;
