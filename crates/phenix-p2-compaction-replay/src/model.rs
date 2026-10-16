//! Model-budget policy over source-verified context projections.
//!
//! This module deliberately does not translate journal events to chat messages:
//! Basic's EventJournal does not retain all ModelRequest fields (such as tool
//! arguments). The *selected* Advanced prompt assembler must use authoritative
//! session history, attach the projection's source citations and return a
//! complete ModelRequest together with a model-specific token count.
use crate::{compacted_view, ContextProjection, ViewProblem};
use phenix_advanced_contracts::CompactionCheckpoint;
use phenix_basic_contracts::Event;

/// The request is the exact assembled object that was measured; callers must
/// submit this object, not independently assemble a second unmeasured prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedModelView<T> {
    pub request: T,
    pub view: ContextProjection,
    pub model_input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelectionError<E> {
    InvalidBudget,
    Projection(ViewProblem),
    Model(E),
    NoFittingView,
}

/// Try verified checkpoints newest-first, accepting only a request measured
/// within the *selected Model adapter's* actual full-request token budget.
///
/// `checkpoints` must come from an authorized revision-validated checkpoint
/// store, and `journal` must come from an authorized live EventJournal read.
/// A source integrity failure aborts selection: retrying an older view could
/// conceal source erasure/tampering. A genuine capacity miss may try an older
/// checkpoint. The callback builds an entire request from authoritative
/// history plus this view, then passes *that same* request by reference to the Model provider's\n/// token meter. Model admission must use the returned object unchanged.
pub fn select_model_view<T, E>(
    checkpoints: &[CompactionCheckpoint],
    journal: &[Event],
    max_visible_payload_bytes: u64,
    max_model_input_tokens: u64,
    mut assemble_request: impl FnMut(&ContextProjection) -> Result<T, E>,
    mut measure_request_tokens: impl FnMut(&T) -> Result<u64, E>,
) -> Result<SelectedModelView<T>, ModelSelectionError<E>> {
    if max_visible_payload_bytes == 0 || max_model_input_tokens == 0 {
        return Err(ModelSelectionError::InvalidBudget);
    }
    for checkpoint in checkpoints.iter().rev() {
        let view = match compacted_view(checkpoint, journal, max_visible_payload_bytes) {
            Ok(view) => view,
            Err(ViewProblem::OverBudget) => continue,
            Err(problem) => return Err(ModelSelectionError::Projection(problem)),
        };
        let request = assemble_request(&view).map_err(ModelSelectionError::Model)?;
        let model_input_tokens =
            measure_request_tokens(&request).map_err(ModelSelectionError::Model)?;
        if model_input_tokens <= max_model_input_tokens {
            return Ok(SelectedModelView {
                request,
                view,
                model_input_tokens,
            });
        }
    }
    Err(ModelSelectionError::NoFittingView)
}
