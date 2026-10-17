//! Opt-in P2-3aj state-triggered graph preparation proof.
//! The kernel dispatches this node after the Basic request is committed.
//! This provider cannot invoke a Model, publish a checkpoint or execute tools.
use phenix_advanced_contracts::{
    validate_model_preparation_input, ModelPreparationInput, ModelPreparationProblem,
    ModelPreparationProposal,
};
use phenix_sdk::{FailurePolicy, JoinPolicy, StaticGraphContribution, StaticGraphNode};

pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.advanced.model-preparer",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "prepare",
        triggers: &["state:phenix.basic.agent/pending_model_request"],
        after: &[],
        selector: "",
        operation: "phenix.advanced.model-preparation-trigger@1/on_change",
        writes: &["phenix.advanced.model-preparer/proposal"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

/// No-op policy for the first native graph boundary test. A true compacting
/// provider must not replace this verifier with a mere checkpoint citation:
/// CAS, source replay and exact selected-model measurement remain required.
pub fn propose_unchanged(
    input: ModelPreparationInput,
) -> Result<ModelPreparationProposal, ModelPreparationProblem> {
    validate_model_preparation_input(&input)?;
    Ok(ModelPreparationProposal {
        original_sha256: input.original_sha256,
        candidate: input.original,
        checkpoint: None,
    })
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.model-preparer",
    version = "0.1.0",
    provide(phenix_advanced_contracts::ModelPreparation, slot = "Policy"),
    provide(phenix_advanced_contracts::ModelPreparationTrigger, slot = "Hook"),
    require(
        phenix_basic_contracts::BasicPendingModelRequest,
        slot = "BasicRequest"
    ),
    graph(super::GRAPH)
)]
mod plugin {
    use phenix_advanced_contracts::{
        model_request_sha256, validate_model_preparation_proposal, ModelPreparation,
        ModelPreparationChange, ModelPreparationError, ModelPreparationInput,
        ModelPreparationProblem, ModelPreparationProposal, ModelPreparationTrigger,
    };
    use phenix_basic_contracts::BasicPendingModelRequest;
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};

    #[derive(Default)]
    pub struct State {
        #[get]
        #[set]
        #[watch]
        proposal: Option<ModelPreparationProposal>,
    }

    fn denied(problem: ModelPreparationProblem, message: &str) -> CallError<ModelPreparationError> {
        CallError::Domain(ModelPreparationError {
            problem,
            message: message.into(),
        })
    }

    #[provide(ModelPreparation::prepare)]
    fn prepare(
        input: ModelPreparationInput,
    ) -> CallResult<ModelPreparationProposal, ModelPreparationError> {
        super::propose_unchanged(input).map_err(|problem| {
            denied(
                problem,
                "original Model request changed before Advanced preparation",
            )
        })
    }

    #[provide(ModelPreparationTrigger::on_change)]
    fn on_change(
        state: &mut State,
        host: &Host<'_>,
        change: ModelPreparationChange,
    ) -> CallResult<ModelPreparationProposal, ModelPreparationError> {
        if change.epoch == 0 || change.properties != ["phenix.basic.agent/pending_model_request"] {
            return Err(denied(
                ModelPreparationProblem::SourceChanged,
                "reactive preparation was not triggered by the Basic request",
            ));
        }
        let reference = BasicPendingModelRequest::get::locate(host, "BasicRequest")?;
        let snapshot = reference
            .read(host)
            .map_err(|error| Failure::new(Code::PluginFailed, error.to_string()))?;
        let original = snapshot
            .value
            .as_ref()
            .as_ref()
            .ok_or_else(|| {
                denied(
                    ModelPreparationProblem::SourceChanged,
                    "missing Basic source",
                )
            })?
            .clone();
        let expected_resource = phenix_basic_contracts::session_resource(&original.session)
            .map_err(|_| {
                denied(
                    ModelPreparationProblem::ScopeDenied,
                    "invalid Basic session",
                )
            })?;
        if host.resource()?.as_deref() != Some(expected_resource.as_str()) {
            return Err(denied(
                ModelPreparationProblem::ScopeDenied,
                "graph action resource does not match Basic session",
            ));
        }
        let source = ModelPreparationInput {
            original_sha256: model_request_sha256(&original).map_err(CallError::Kernel)?,
            original,
        };
        let proposal = super::propose_unchanged(source.clone())
            .map_err(|problem| denied(problem, "Basic source failed validation"))?;
        validate_model_preparation_proposal(&source, &proposal)
            .map_err(|problem| denied(problem, "untrusted proposal failed validation"))?;
        let mut edit = state
            .proposal
            .edit(host)
            .map_err(|error| Failure::new(Code::PluginFailed, error.to_string()))?;
        *edit = Some(proposal.clone());
        edit.commit(host)
            .map_err(|error| Failure::new(Code::PluginFailed, error.to_string()))?;
        #[cfg(feature = "witness")]
        if let Some(path) = std::env::var_os("PHENIX_P2_MODEL_PREPARE_WITNESS") {
            std::fs::write(path, proposal.original_sha256.as_bytes()).map_err(|error| {
                Failure::new(Code::PluginFailed, format!("witness write failed: {error}"))
            })?;
        }
        Ok(proposal)
    }
}
