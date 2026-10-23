//! Single source of graph scheduling: the kernel triggers this hook
//! on the committed Basic request; selection supplies the actual preparer.
use phenix_advanced_contracts::{
    model_request_sha256, validate_model_preparation_proposal, ModelPreparation,
    ModelPreparationChange, ModelPreparationError, ModelPreparationInput, ModelPreparationProblem,
    ModelPreparationProposal, PreparationReceipt,
};
use phenix_basic_contracts::{BasicPendingModelRequest, GraphStateChange, ModelRequest};
use phenix_sdk::{
    CallError, CallResult, Code, Failure, FailurePolicy, Host, JoinPolicy, StaticGraphContribution,
    StaticGraphNode,
};

pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.advanced.graph-preparer",
    events: &[],
    actions: &[],
    nodes: &[
        StaticGraphNode {
            id: "advance",
            triggers: &["state:phenix.advanced.graph-preparer/prepared_model_request"],
            after: &[
                "phenix.advanced.graph-preparer/prepare",
                "phenix.basic.agent/prepare",
            ],
            selector: "Advance",
            operation: "phenix.basic.turn-advance@1/advance",
            writes: &["phenix.basic.agent/pending_model_request"],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "prepare",
            triggers: &["state:phenix.basic.agent/pending_model_request"],
            after: &[],
            selector: "",
            operation: "phenix.advanced.model-preparation-change@1/run",
            writes: &["phenix.advanced.graph-preparer/prepared_model_request"],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
    ],
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
    require(phenix_basic_contracts::BasicTurnAdvance, slot = "Advance"),
    require(phenix_advanced_contracts::ModelPreparation, slot = "Preparer"),
    graph(super::GRAPH)
)]
mod provider {
    use super::*;
    #[derive(Default)]
    pub struct State {
        // Advanced owns its prepared output. Basic receives a read-only
        // versioned view through the selected Prepared property import.
        #[expose]
        #[watch]
        prepared_model_request: Option<ModelRequest>,
    }

    #[provide(ModelPreparationChange::run)]
    fn run(
        state: &mut State,
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
        let source_snapshot = BasicPendingModelRequest::get::locate(host, "Source")?
            .read(host)
            .map_err(host_failure)?;
        // Notification epochs are global and may include other cell commits.
        // Cite the pending request's actual last-write epoch, not the
        // coalesced graph notification epoch. Never prepare a newer request
        // on behalf of an older notification.
        let source_epoch = source_snapshot.epoch.0;
        if source_epoch == 0 || source_epoch > notification.epoch {
            return Err(invalid(
                ModelPreparationProblem::SourceChanged,
                "pending Basic request revision does not belong to this graph change",
            ));
        }
        let source = source_snapshot.value.as_ref().clone().ok_or_else(|| {
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
        let receipt = PreparationReceipt::new(source_epoch, input.original_sha256)
            .map_err(|problem| invalid(problem, "invalid source-bound preparation receipt"))?;
        // Committing the exported observation also commits the Advanced
        // plugin's internal field. Basic never grants this plugin write
        // access to Basic-owned state.
        state
            .publish_prepared_model_request(host, Some(proposal.candidate))
            .map_err(host_failure)?;
        Ok(receipt)
    }
}

#[cfg(test)]
mod ownership_tests {
    #[test]
    fn prepared_request_is_only_observable_from_outside() {
        let metadata =
            phenix_sdk::ContractMetadata::decode(super::provider::METADATA.bytes()).unwrap();
        let field = metadata
            .properties
            .iter()
            .find(|field| field.id == "prepared_model_request")
            .unwrap();
        assert!(field.get && field.watch);
        assert!(!field.set, "Advanced alone publishes this owned value");
        assert_eq!(metadata.plugin, "phenix.advanced.graph-preparer");
    }
}
