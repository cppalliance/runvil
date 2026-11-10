//! What a conversation hands its run's setup, and how it runs the agent
//! and takes the run's report: the recorder tee, the live pieces of the
//! run's own rounds, and the drive that parses the agent, runs it, and
//! ends the conversation with its run.

use std::sync::Arc;

use runvil::harness::{RunRecorder, display_chain};
use runvil::ids::RoundId;
use runvil::{Error, Harness, Host, Prompt, RunReport, RunSetup};

use super::{Conversation, lock};
use crate::protocol::{Delta, DeltaKind};
use crate::state::{FailureKind, SessionState};
use crate::tee::ConversationRecorder;

/// The report of a run a close or an uncaught stop cut short.
const CANCELED: &str = "the agent run was canceled";

impl Conversation {
    /// The recorder the run writes through: every call goes on to
    /// `inner`, and each event `inner` accepted reaches this
    /// conversation.
    #[must_use]
    pub fn recorder(&self, inner: Arc<dyn RunRecorder>) -> Arc<dyn RunRecorder> {
        Arc::new(ConversationRecorder::new(self.clone(), inner))
    }

    /// Parses the agent `name` on `host` on the blocking pool, then runs
    /// it with `setup`. A parse failure is reported as a run failure with
    /// its located message; no run is recorded.
    ///
    /// The run's Harness control is held while the run goes on, so the
    /// operator's messages, a stop, and a close reach the run. The
    /// conversation ends with its run, or with the parse failure.
    pub async fn run(&self, host: Arc<Host>, name: &str, setup: RunSetup) {
        let prompt = match parse(&host, name).await {
            Ok(prompt) => prompt,
            Err(message) => {
                tracing::warn!(
                    conversation = %self.core.id,
                    agent = %self.core.agent,
                    %message,
                    "agent failed to parse"
                );
                self.report(FailureKind::RunFailed, message);
                self.end();
                return;
            }
        };
        let harness = Harness::new(host, prompt, setup);
        {
            let mut control = lock(&self.core.control);
            let held = control.insert(harness.control());
            if self.state() != SessionState::Alive {
                held.cancel();
            }
        }
        let result = harness.run().await;
        self.take_report(result);
        self.end();
    }

    /// Notes the report's run, logs how many of the operator's messages
    /// it never read, and reports a run that failed or was cut short; a
    /// completed run reports nothing.
    fn take_report(&self, result: Result<RunReport, Error>) {
        let message = match result {
            Ok(report) => {
                self.note_run(report.run_id);
                tracing::info!(
                    conversation = %self.core.id,
                    agent = %self.core.agent,
                    unread = report.unread.len(),
                    "the agent run ended; this many of the operator's messages went unread"
                );
                match report.result {
                    Ok(_) => return,
                    Err(error) if error.is_canceled() => {
                        self.report(FailureKind::Canceled, CANCELED.to_owned());
                        return;
                    }
                    Err(error) => error.located_message(),
                }
            }
            Err(error) => display_chain(&error),
        };
        tracing::warn!(
            conversation = %self.core.id,
            agent = %self.core.agent,
            %message,
            "agent run failed"
        );
        self.report(FailureKind::RunFailed, message);
    }

    /// Broadcasts one live piece of the run's round `round` as a
    /// [`Delta`] of `kind`, stamped with the round's id, the id the
    /// round's reply event carries. No client listening is not a failure:
    /// the completed reply travels in the round's answer.
    pub fn publish_delta(&self, round: RoundId, kind: DeltaKind, content: String) {
        if let Some(channels) = self.channels() {
            // No receiver means no client is attached; deltas are
            // ephemeral and the completed-reply event is the repair.
            let _ = channels.deltas.send(Delta {
                kind,
                content,
                reply: round.get(),
            });
        }
    }
}

/// The agent `name` parsed on `host` on the blocking pool, since finding
/// it reads the agents folder, or the message of why it did not parse: the
/// parse failure's located message, or the blocking task's failure.
async fn parse(host: &Arc<Host>, name: &str) -> Result<Prompt, String> {
    let host = Arc::clone(host);
    let name = name.to_owned();
    match tokio::task::spawn_blocking(move || host.parse(&name)).await {
        Ok(parsed) => parsed.map_err(|error| error.located_message()),
        Err(join) => Err(format!("the agent could not be parsed: {join}")),
    }
}
