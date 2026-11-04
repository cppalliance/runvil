//! Tests for the scope types: a count increment seeds the id it was
//! handed.

use super::ToolCallCounts;

#[test]
fn increment_seeds_a_missing_id_and_counts_it() {
    let counts = ToolCallCounts::default();
    counts
        .increment("std/read")
        .expect("increment seeds an id it was never given");
    let snapshot = counts.snapshot().expect("the counts mutex is not poisoned");
    assert_eq!(snapshot.get("std/read"), Some(&1));
}
