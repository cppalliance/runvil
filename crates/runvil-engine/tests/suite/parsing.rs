//! Public-parser contracts: invalid fixtures report their exact
//! [`ErrorCode`] and message.

use std::fs;
use std::path::Path;

use runvil_engine::ErrorCode;
use runvil_engine::Prompt;

use super::support::TestResult;

const INVALID_FIXTURES: [(&str, ErrorCode, &str); 6] = [
    ("missing-h1", ErrorCode::StructureInvalid, "requires an H1"),
    (
        "unrecognized-lua-label",
        ErrorCode::FenceInvalid,
        "unrecognized fence label",
    ),
    (
        "lua-fence-opener",
        ErrorCode::FenceInvalid,
        "fence opening line must be exactly",
    ),
    (
        "lua-fence-closer",
        ErrorCode::FenceInvalid,
        "fence closing line must be exactly",
    ),
    (
        "malformed-epilog",
        ErrorCode::LuaCompile,
        "section `Transform` epilog",
    ),
    (
        "list-h3-non-list-content",
        ErrorCode::ListEmptyItem,
        "empty bullet item",
    ),
];

#[test]
fn invalid_prompt_files_report_public_error_contracts() -> TestResult {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/prompts/invalid");
    for (stem, code, message_fragment) in INVALID_FIXTURES {
        let name = format!("invalid/{stem}.md");
        let source = fs::read_to_string(dir.join(format!("{stem}.md")))?;
        let Err(error) = Prompt::parse(&|_: &str| Ok(source.clone()), &name).0 else {
            return Err(format!("fixture {name} unexpectedly parsed").into());
        };
        assert_eq!(
            error.code, code,
            "fixture {name} returned the wrong error code: {error:?}"
        );
        assert!(
            error.message.contains(message_fragment),
            "fixture {name} error did not contain {message_fragment:?}: {error}"
        );
    }
    Ok(())
}
