//! Single source of graph scheduling: the kernel triggers this hook
//! on the committed Basic request; selection supplies the actual preparer.
use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_proposal, ModelPreparation,
    ModelPreparationChange, ModelPreparationError, ModelPreparationInput, ModelPreparationProblem,
    ModelPreparationProposal, PreparationReceipt,
};
use phenix_basic_contracts::{
    BasicPendingModelRequest, BasicPreparedModelRequest, GraphStateChange,
};
use phenix_sdk::{
    CallError, CallResult, Code, Failure, FailurePolicy, Host, JoinPolicy, StaticGraphContribution,
    StaticGraphNode,
};

pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.advanced.graph-preparer",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "prepare",
        triggers: &["state:phenix.basic.agent/pending_model_request"],
        after: &[],
        selector: "",
        operation: "phenix.advanced.model-preparation-change@1/run",
        writes: &["phenix.basic.agent/prepared_model_request"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

fn invalid(problem: ModelPreparationProblem, message: &str) -> CallError<ModelPreparationError> {
    CallError::Domain(ModelPreparationError {
        problem,
        message: message.into(),
    })
}
fn host_failure(error: impl std::fmt::Display) -> CallError<ModelPreparationError> {
    CallError::Kernel(Failure::new(Code::PluginFailed, error.to_string()))
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.graph-preparer",
    version = "1.0.0",
    provide(phenix_advanced_contracts::ModelPreparationChange, slot = "Change"),
    require(phenix_basic_contracts::BasicPendingModelRequest, slot = "Source"),
    require(phenix_basic_contracts::BasicPreparedModelRequest, slot = "Prepared"),
    require(phenix_advanced_contracts::ModelPreparation, slot = "Preparer"),
    graph(super::GRAPH)
)]
mod provider {
    use super::*;
    #[derive(Default)]
    pub struct State;

    #[provide(ModelPreparationChange::run)]
    fn run(
        host: &Host<'_>,
        notification: GraphStateChange,
    ) -> CallResult<PreparationReceipt, ModelPreparationError> {
        if notification.epoch == 0
            || notification.properties != ["phenix.basic.agent/pending_model_request"]
        {
            return Err(invalid(
                ModelPreparationProblem::SourceChanged,
                "wrong graph change event",
            ));
        }
        let source = BasicPendingModelRequest::get::locate(host, "Source")?
            .read(host)
            .map_err(host_failure)?
            .value
            .as_ref()
            .clone()
            .ok_or_else(|| {
                invalid(
                    ModelPreparationProblem::SourceChanged,
                    "no pending Basic request",
                )
            })?;
        let expected_resource =
            phenix_basic_contracts::session_resource(&source.session).map_err(|_| {
                invalid(
                    ModelPreparationProblem::ScopeDenied,
                    "invalid Basic session",
                )
            })?;
        if host.resource()?.as_deref() != Some(expected_resource.as_str()) {
            return Err(invalid(
                ModelPreparationProblem::ScopeDenied,
                "graph prepare session mismatch",
            ));
        }
        let input = ModelPreparationInput {
            original_sha256: model_request_sha256(&source).map_err(host_failure)?,
            original: source,
        };
        let proposal: ModelPreparationProposal =
            host.invoke::<ModelPreparation::prepare>("Preparer", &input)?;
        validate_model_preparation_proposal(&input, &proposal).map_err(|problem| {
            invalid(problem, "selected preparation proposal is not source bound")
        })?;
        // P2-3ak is an identity-only cutover, not yet a compaction permission.
        // Even a structurally sound checkpoint proposal cannot bypass the
        // mandatory independent source/CAS/meter verification later.
        if proposal.candidate != input.original || proposal.checkpoint.is_some() {
            return Err(invalid(
                ModelPreparationProblem::CapabilitiesChanged,
                "changed prepared requests need verified checkpoint admission",
            ));
        }
        let selected = BasicPreparedModelRequest::set::locate(host, "Prepared")?;
        let mut draft = selected.edit(host).map_err(host_failure)?;
        *draft = Some(proposal.candidate);
        draft.commit(host).map_err(host_failure)?;
        Ok(PreparationReceipt {
            source_epoch: notification.epoch,
            request_sha256: input.original_sha256,
        })
    }
}
