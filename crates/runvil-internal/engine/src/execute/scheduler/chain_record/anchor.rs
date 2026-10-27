//! What a chain's `chat` rounds taught the context precheck about token
//! counts.

use runvil_types::metrics::Usage;

use crate::lua::UsageAnchor;
use crate::model::{Message, ModelId};

/// What the chain's `chat` rounds taught the context precheck about token
/// counts: the provider's numbers for the newest round that reported usage.
///
/// A chain has one parked `chat` round at a time. Dispatch holds that
/// round's projected messages in `in_flight`, beside the id of the model
/// they went to; the arrival of its answer settles them into `measured`
/// when the round reported usage, and drops them otherwise. A round with no
/// usage leaves the older measurement in place: it still describes a prefix
/// of the conversation, and the precheck uses it only while the request
/// extends that prefix on the same model.
///
/// The model is compared by [`ModelId`], the gateway server and model name
/// the binding resolved to, because that is what picks the tokenizer. The
/// alias is only a prompt-local label: two aliases of one model, with
/// other temperature or thinking settings, count tokens alike and share a
/// measurement.
#[derive(Debug, Default)]
pub(in crate::execute::scheduler) struct ChatAnchor {
    in_flight: Option<(ModelId, Vec<Message>)>,
    measured: Option<(ModelId, UsageAnchor)>,
}

impl ChatAnchor {
    /// The newest measurement, for the precheck of the next dispatch to
    /// `model`. A measurement from another model is not offered: its token
    /// counts come from another tokenizer.
    pub(in crate::execute::scheduler) fn measured(&self, model: &ModelId) -> Option<&UsageAnchor> {
        self.measured
            .as_ref()
            .filter(|(measured_by, _)| measured_by == model)
            .map(|(_, anchor)| anchor)
    }

    /// Records the projected messages of a round about to be sent to
    /// `model`.
    pub(in crate::execute::scheduler) fn sending(
        &mut self,
        model: ModelId,
        messages: Vec<Message>,
    ) {
        self.in_flight = Some((model, messages));
    }

    /// Settles the parked round: its model, sent messages, and `usage`
    /// replace the measurement when it reported usage; a failed round, or
    /// one with no usage, only releases the sent messages.
    pub(in crate::execute::scheduler) fn settle(&mut self, usage: Option<&Usage>) {
        let sent = self.in_flight.take();
        if let (Some((model, messages)), Some(usage)) = (sent, usage) {
            self.measured = Some((model, UsageAnchor::new(messages, usage)));
        }
    }
}
