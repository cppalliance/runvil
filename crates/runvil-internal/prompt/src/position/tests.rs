//! Tests for the source-position helpers.

use runvil_types::error::ErrorCode;

use super::newlines_before;

#[test]
fn newlines_before_reports_broken_byte_offset_invariants() {
    for offset in [1, 4] {
        let error = newlines_before("é\n", offset)
            .expect_err("a non-boundary or out-of-range offset must return an error");
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(
            error.message,
            "internal invariant violated: parser: source byte offset invariant broken"
        );
    }
}
