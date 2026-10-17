//! Opt-in identity preparation provider. No journal edits, CAS writes,
//! Model calls or tool effects. Replaceable through one selected M1 slot.
use phenix_advanced_contracts::{
    validate_model_preparation_input, ModelPreparationError, ModelPreparationInput,
    ModelPreparationProposal,
};
use phenix_sdk::{CallError, CallResult};

pub fn unchanged_proposal(
    input: ModelPreparationInput,
) -> Result<ModelPreparationProposal, ModelPreparationError> {
    validate_model_preparation_input(&input).map_err(|problem| ModelPreparationError {
        problem,
        message: "original Basic Model request digest changed".into(),
    })?;
    Ok(ModelPreparationProposal {
        original_sha256: input.original_sha256,
        candidate: input.original,
        checkpoint: None,
    })
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.preparer-noop",
    version = "1.0.0",
    provide(phenix_advanced_contracts::ModelPreparation, slot = "Prepare")
)]
mod provider {
    use super::*;
    use phenix_advanced_contracts::ModelPreparation;

    #[derive(Default)]
    pub struct State;

    #[provide(ModelPreparation::prepare)]
    fn prepare(
        request: ModelPreparationInput,
    ) -> CallResult<ModelPreparationProposal, ModelPreparationError> {
        unchanged_proposal(request).map_err(CallError::Domain)
    }
}
