//! Opt-in P2 graph preparation, selected through ordinary M1 imports.
//! Basic owns its pending ModelRequest; this plugin publishes only an
//! independently source-bound *proposal*. No Model/tool/checkpoint executes.
use phenix_advanced_contracts::{
    validate_model_preparation_input, GraphPreparationChange, ModelPreparationError,
    ModelPreparationInput, ModelPreparationProblem, ModelPreparationProposal,
};
use phenix_sdk::{FailurePolicy, JoinPolicy, StaticGraphContribution, StaticGraphNode};

pub const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.advanced.graph-preparation",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "prepare",
        triggers: &["state:phenix.basic.agent/pending_model_request"],
        after: &[],
        selector: "",
        operation: "phenix.advanced.graph-preparation@1/run",
        writes: &["phenix.advanced.graph-preparation/prepared"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

fn problem(kind: ModelPreparationProblem, message: &str) -> ModelPreparationError {
    ModelPreparationError {
        problem: kind,
        message: message.into(),
    }
}

/// No-compaction is an explicit verified proposal. It is not the absence
/// of an authorization check or an alternate Model execution path.
pub fn pass_through(
    input: &ModelPreparationInput,
) -> Result<ModelPreparationProposal, ModelPreparationError> {
    validate_model_preparation_input(input)
        .map_err(|kind| problem(kind, "invalid original Model request"))?;
    Ok(ModelPreparationProposal {
        original_sha256: input.original_sha256.clone(),
        candidate: input.original.clone(),
        checkpoint: None,
    })
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.graph-preparation",
    provide(phenix_advanced_contracts::GraphPreparationHook, slot = "Prepare"),
    require(phenix_basic_contracts::BasicPendingModelRequest, slot = "Pending"),
    require(
        phenix_advanced_contracts::ModelPreparation,
        slot = "Selected",
        optional
    ),
    graph(super::GRAPH)
)]
mod plugin {
    use super::*;
    use phenix_advanced_contracts::{
        model_request_sha256, validate_model_preparation_proposal, GraphPreparationHook,
        ModelPreparation, ModelPreparationProposal,
    };
    use phenix_basic_contracts::BasicPendingModelRequest;
    use phenix_sdk::{
        CallError, CallResult, Code, Failure, Host, ImportCall, ImportPoll, PendingImport,
        PendingResult,
    };

    #[derive(Default)]
    pub struct State {
        #[get]
        #[set]
        #[watch]
        prepared: Option<ModelPreparationProposal>,
        child: Option<PendingImport<ModelPreparation::prepare>>,
        expected: Option<ModelPreparationInput>,
    }

    fn finish(
        state: &mut State,
        host: &Host<'_>,
        input: &ModelPreparationInput,
        proposed: CallResult<ModelPreparationProposal, ModelPreparationError>,
    ) -> PendingResult<ModelPreparationProposal, ModelPreparationError> {
        let proposal = match proposed {
            Ok(proposal) => proposal,
            Err(CallError::Kernel(failure)) => return PendingResult::QuiescentFailure(failure),
            Err(CallError::Domain(domain)) => {
                return PendingResult::Ready(Err(CallError::Domain(domain)))
            }
        };
        if let Err(kind) = validate_model_preparation_proposal(input, &proposal) {
            return PendingResult::Ready(Err(CallError::Domain(problem(
                kind,
                "selected preparation changed the authenticated request",
            ))));
        }
        let mut edit = match state.prepared.edit(host) {
            Ok(edit) => edit,
            Err(err) => {
                return PendingResult::QuiescentFailure(Failure::new(
                    Code::PluginFailed,
                    err.to_string(),
                ))
            }
        };
        *edit = Some(proposal.clone());
        if let Err(err) = edit.commit(host) {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                err.to_string(),
            ));
        }
        // Native acceptance-only witness, emitted strictly after the M1
        // watched proposal commit. Never present in ordinary compositions.
        if let Some(path) = std::env::var_os("PHENIX_P2_GRAPH_PREPARED_WITNESS") {
            if let Err(err) = std::fs::write(path, proposal.original_sha256.as_bytes()) {
                return PendingResult::QuiescentFailure(Failure::new(
                    Code::PluginFailed,
                    format!("verification receipt could not be written: {err}"),
                ));
            }
        }
        PendingResult::Ready(Ok(proposal))
    }

    #[pending(GraphPreparationHook::run)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        change: GraphPreparationChange,
    ) -> PendingResult<ModelPreparationProposal, ModelPreparationError> {
        if change.epoch == 0
            || change.properties != ["phenix.basic.agent/pending_model_request"]
            || state.child.is_some()
            || state.expected.is_some()
        {
            return PendingResult::Ready(Err(CallError::Domain(problem(
                ModelPreparationProblem::SourceChanged,
                "invalid or concurrent graph trigger",
            ))));
        }
        let pointer = match BasicPendingModelRequest::get::locate(host, "Pending") {
            Ok(pointer) => pointer,
            Err(err) => return PendingResult::QuiescentFailure(err),
        };
        let snapshot = match pointer.read(host) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                return PendingResult::QuiescentFailure(Failure::new(
                    Code::PluginFailed,
                    err.to_string(),
                ))
            }
        };
        let Some(original) = snapshot.value.as_ref().clone() else {
            return PendingResult::Ready(Err(CallError::Domain(problem(
                ModelPreparationProblem::SourceChanged,
                "no pending Basic request",
            ))));
        };
        if let Err(failure) = phenix_basic_contracts::check_session(host, &original.session) {
            return match failure {
                CallError::Kernel(failure) => PendingResult::QuiescentFailure(failure),
                CallError::Domain(_) => PendingResult::Ready(Err(CallError::Domain(problem(
                    ModelPreparationProblem::ScopeDenied,
                    "Basic session is not authorized",
                )))),
            };
        }
        let sha = match model_request_sha256(&original) {
            Ok(sha) => sha,
            Err(err) => return PendingResult::QuiescentFailure(err),
        };
        let input = ModelPreparationInput {
            original,
            original_sha256: sha,
        };
        if host.slot_count("Selected") == 0 {
            return finish(
                state,
                host,
                &input,
                pass_through(&input).map_err(CallError::Domain),
            );
        }
        match host.begin::<ModelPreparation::prepare>("Selected", &input) {
            Ok(ImportCall::Ready(result)) => finish(state, host, &input, result),
            Ok(ImportCall::Pending(child)) => {
                state.child = Some(child);
                state.expected = Some(input);
                PendingResult::Pending
            }
            Err(failure) => PendingResult::QuiescentFailure(failure),
        }
    }

    #[poll(GraphPreparationHook::run)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
    ) -> PendingResult<ModelPreparationProposal, ModelPreparationError> {
        let Some(mut child) = state.child.take() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "no pending selected graph preparer",
            ));
        };
        match child.poll(host) {
            Ok(ImportPoll::Pending | ImportPoll::Draining) => {
                state.child = Some(child);
                PendingResult::Pending
            }
            Ok(ImportPoll::Ready(result)) => {
                let Some(expected) = state.expected.take() else {
                    return PendingResult::QuiescentFailure(Failure::new(
                        Code::PluginFailed,
                        "lost original request source",
                    ));
                };
                finish(state, host, &expected, result)
            }
            Ok(ImportPoll::Cancelled) => {
                state.expected = None;
                PendingResult::QuiescentFailure(Failure::new(
                    Code::ScopeRejected,
                    "selected graph preparation cancelled",
                ))
            }
            Err(err) => {
                state.expected = None;
                PendingResult::QuiescentFailure(err)
            }
        }
    }

    #[cancel(GraphPreparationHook::run)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        state.child = None;
        state.expected = None;
        Ok(true)
    }
}
