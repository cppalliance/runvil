//! The agent socket's rendering of a refused launch: the error frame's
//! text includes the refusal's cause chain, not just its outermost
//! message. A launch hands back receivers subscribed before its run
//! starts, so they hold a failure the run reported and ended on before
//! any socket attached.

use std::io;
use std::time::Duration;

use serde_json::json;
use tokio::sync::broadcast::error::TryRecvError;
use workshop_agents::{FailureKind, LaunchError, SessionState};
use workshop_gateway::ResolvedGateway;
use workshop_support::{AgentsConfig, Config};

use super::*;
use crate::app::state_with_gateway;
use crate::app::test_helpers::config_for;

#[test]
fn a_refused_launch_frame_includes_the_cause_text() {
    let cause = "agents directory is locked by another process";
    let refusal = LaunchRefusal::Refused(LaunchError::Unreadable {
        source: io::Error::new(io::ErrorKind::PermissionDenied, cause),
    });

    let rendered = refusal_text(&refusal);

    assert!(
        rendered.contains("agent session state unavailable"),
        "the refusal's own message is missing: {rendered}"
    );
    assert!(
        rendered.contains(cause),
        "the cause text is missing from the frame: {rendered}"
    );
}

#[test]
fn an_unusable_gateway_renders_its_own_message_alone() {
    assert_eq!(
        refusal_text(&LaunchRefusal::GatewayUnusable),
        "agent sessions need a usable gateway binding; check the gateway base URL and key"
    );
}

#[tokio::test]
async fn a_launch_hands_back_the_parse_failure_its_run_ended_on_before_any_attach() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let agents_dir = dir.path().join("agents");
    std::fs::create_dir(&agents_dir).expect("the agents directory creates");
    // Readable, so the launch check accepts it, but with no frontmatter,
    // so the run's parse fails before the run begins.
    std::fs::write(agents_dir.join("broken.md"), "# Broken\n").expect("the agent writes");
    let config = Config {
        agents: AgentsConfig {
            path: agents_dir,
            mcp: None,
        },
        ..config_for("http://127.0.0.1:1", dir.path())
    };
    let gateway = ResolvedGateway::from_config(&config.gateway);
    let state = state_with_gateway(&config, &gateway).expect("state builds in tests");
    // A run starts only once the catalog holds a chat-capable model.
    state
        .catalog()
        .publish(vec![json!({ "id": "test-model", "object": "model" })]);

    let (conversation, mut receivers) = state
        .agents()
        .launch("broken")
        .await
        .expect("the launch is accepted");
    tokio::time::timeout(Duration::from_secs(10), async {
        while conversation.state() != SessionState::Closed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the failed parse ends the conversation");

    assert_eq!(
        conversation.subscribe_errors().try_recv(),
        Err(TryRecvError::Closed),
        "a socket subscribing only after the run ended would see nothing"
    );
    let failure = receivers
        .errors
        .try_recv()
        .expect("the launch's receiver kept the failure");
    assert_eq!(failure.kind, FailureKind::RunFailed);
    assert!(
        failure.message.contains("frontmatter delimiter"),
        "the failure is the parse's own: {}",
        failure.message
    );
}
