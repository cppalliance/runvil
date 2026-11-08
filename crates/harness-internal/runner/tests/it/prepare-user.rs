//! The built-in `runvil/user` at preparation: a run whose setup has a user
//! holds it, so a prompt that declares it runs and `tools.packs.get`
//! hands back its Pack object, and a run whose setup has none holds it
//! unavailable, so that prompt is refused at its first step naming why
//! and `tools.packs.get('user')` is nil.

use super::*;

/// A prompt that declares the user Pack and returns a constant.
const NEEDS_USER: &str = "---\nname: needs-user\ndescription: d\nrunvil: 0\n\
    uses:\n  - user\n---\n\n# Title\n\n## Only\n\n```lua\nreturn 'ran'\n```\n";

/// A prompt that declares no Pack and returns the type of what
/// `tools.packs.get('user')` hands back.
const CHECKS_USER: &str = "---\nname: checks-user\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\nreturn type(tools.packs.get('user'))\n```\n";

/// The setup of a run with a user, recorded through `recorder`.
fn with_user(recorder: &Arc<MemoryRecorder>) -> RunSetup {
    RunSetup {
        has_user: true,
        ..setup_over(recorder)
    }
}

#[tokio::test]
async fn a_prompt_declaring_user_runs_with_a_user_and_is_refused_at_its_first_step_without_one() {
    let report = report_of(NEEDS_USER, bare(), with_user(&recorder())).await;
    assert_eq!(
        completed(report.result),
        "ran",
        "a run with a user holds it"
    );

    let recorder = recorder();
    let (run_id, result) = prepare_and_drive(NEEDS_USER, bare(), setup_over(&recorder)).await;
    assert_eq!(
        refusal(result),
        "the environment cannot satisfy this prompt:\n\
         - user is unavailable: this run has no user (at prompt:5)",
        "the run names why, on the `uses:` line"
    );
    assert_no_records(&recorder, run_id);
}

#[tokio::test]
async fn tools_packs_get_user_is_a_pack_object_in_a_run_with_a_user_and_nil_in_one_without() {
    let report = report_of(CHECKS_USER, bare(), with_user(&recorder())).await;
    assert_eq!(
        completed(report.result),
        "userdata",
        "a run with a user holds runvil/user, declared or not"
    );
    assert_eq!(
        completed(drive_over(CHECKS_USER, bare()).await),
        "nil",
        "a run without one holds it unavailable"
    );
}
