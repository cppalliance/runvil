//! Integration tests for `plugin-std`: the Pack `install` inserts, the
//! tools `construct` names under the Pack id it receives, and what a call
//! answers with arguments that match or miss the schema.
#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]

mod tools;
