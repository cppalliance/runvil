//! What a conversation hands its run's setup, and how it runs the agent
//! and takes the run's report: the recorder tee, the run's own services
//! holding the conversation's input broker, the live pieces of the run's
//! own rounds, and the drive that parses the agent, runs it, and ends the
//! conversation with its run.

use std::sync::Arc;

use harness::pack::HostServices;
use harness::record::RunRecorder;
use harness::{Harness, Host, RunReport, RunSetup, display_chain};
use plugin_user_input::{INPUT_BROKER, InputBroker};
use runvil::ids::RoundId;
use runvil::{Error, Prompt};
use tokio::sync::broadcast;

use super::{Conversation, lock};
use crate::input::SessionInputBroker;
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

    /// The run's own services: this conversation's input broker, supplied
    /// under [`INPUT_BROKER`], and nothing else.
    #[must_use]
    pub fn run_services(&self) -> HostServices {
        let frames = self
            .channels()
            .map_or_else(|| broadcast::channel(1).0, |channels| channels.waits);
        let broker: Arc<dyn InputBroker> = Arc::new(SessionInputBroker::new(
            Arc::clone(&self.core.waits),
            frames,
        ));
        let mut services = HostServices::new();
        if let Err(error) = services.provide(&INPUT_BROKER, broker) {
            // An empty map takes the broker's valid literal id, so this
            // refusal is defensive.
            tracing::warn!(
                conversation = %self.core.id,
                %error,
                "the conversation's input broker was refused; the run has no operator"
            );
        }
        services
    }

    /// Parses the agent `name` on `host` on the blocking pool, then runs
    /// it with `setup`. A parse failure is reported as a run failure with
    /// its located message; no run is recorded.
    ///
    /// The run's Harness control is held while the run goes on, so a stop
    /// or a close reaches the run. The conversation ends with its run, or
    /// with the parse failure.
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

    /// Notes the report's run and reports a run that failed or was cut
    /// short; a completed run reports nothing.
    fn take_report(&self, result: Result<RunReport, Error>) {
        let message = match result {
            Ok(report) => {
                self.note_run(report.run_id);
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
