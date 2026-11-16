//! A run's environment: the resolution of the selected model, through the
//! run's inference broker, into the run's params.
//!
//! Everything here arrives as data or through the broker. The Harness
//! never resolves a gateway, reads a menu, or names a workspace crate: the
//! launcher puts its selection in each run's `RunSetup`, and the run binds
//! the model the broker lists under that selection.

use runvil_engine::model::{ModelId, ModelInfo};
use runvil_engine::{Error, ErrorCode, ErrorFact};

use crate::performers::InferenceBroker;

/// The run's model from `broker`'s list: the one `selected` names, or the
/// first listed when `selected` is `None`.
///
/// The model is `None` only when there is no selection and the broker
/// lists no model; the prompt's declared roles then stay unbound.
/// A failed listing, an invalid selection, or a selection
/// absent from the list is a reported failure, never a fabricated
/// fallback model info. The recorded run ends with that failure as its
/// result.
///
/// # Errors
/// Returns the broker's own failure, its message after `the model catalog
/// could not be fetched: `, when the model list cannot be fetched, and
/// `RequirementsUnmet` with the selected id as the `Name` fact and the
/// `Reason` fact `model_absent` when the selected id is not in it.
/// An empty selection or one containing control characters returns
/// `InvalidValue` before fetching the catalog.
pub(crate) async fn current_model(
    selected: Option<&str>,
    broker: &dyn InferenceBroker,
) -> Result<Option<ModelInfo>, Error> {
    let selected = selected.map(ModelId::gateway).transpose()?;
    let catalog = broker.models().await.map_err(|mut error| {
        error.message = format!("the model catalog could not be fetched: {}", error.message);
        error
    })?;
    let Some(id) = selected else {
        return Ok(catalog.models().first().cloned());
    };
    catalog.get(&id).cloned().map(Some).ok_or_else(|| {
        let selected = id.name();
        let code = ErrorCode::RequirementsUnmet;
        let mut error = Error::new(
            code,
            format!("the selected model `{selected}` is absent from the fetched catalog"),
            code.prose(),
        );
        error.facts.insert(ErrorFact::Name, selected.into());
        error.facts.insert(ErrorFact::Reason, "model_absent".into());
        error
    })
}

#[cfg(test)]
#[path = "environment-tests.rs"]
mod tests;
