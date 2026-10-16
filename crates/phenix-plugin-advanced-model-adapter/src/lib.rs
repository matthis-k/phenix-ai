//! P2 Advanced Model interception point.
//!
//! This is a real selected Basic Model provider, not a second agent loop.
//! It delegates each authorized request to the configured upstream Model
//! through M1 pending imports. The Basic graph owns tool progression,
//! durable sessions, journal effects and turn boundaries. Until source-backed
//! compaction admission and a selected-provider token-meter contract are
//! available, it forwards the original request unchanged (never guessed
//! token counts or fabricated journal-to-Model messages).
#[phenix_sdk::plugin(
    id = "phenix.advanced.model-adapter",
    version = "1.0.0",
    provide(phenix_basic_contracts::Model, slot = "Model"),
    require(phenix_basic_contracts::Model, slot = "Upstream")
)]
mod plugin {
    use phenix_basic_contracts::{error, BasicError, ErrorCode, Model, ModelReply, ModelRequest};
    use phenix_sdk::{
        CallError, CallResult, Code, Failure, Host, ImportCall, ImportPoll, PendingImport,
        PendingResult,
    };

    #[derive(Default)]
    pub struct State {
        pending: Option<PendingImport<Model::complete>>,
    }

    fn finish(result: CallResult<ModelReply, BasicError>) -> PendingResult<ModelReply, BasicError> {
        match result {
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
            other => PendingResult::Ready(other),
        }
    }

    #[pending(Model::complete)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> PendingResult<ModelReply, BasicError> {
        if let Err(failure) = phenix_basic_contracts::check_session(host, &request.session) {
            return finish(Err(failure));
        }
        if state.pending.is_some() {
            return finish(Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "Advanced Model is already awaiting its selected upstream",
            ))));
        }
        // Neither the adapter nor its caller may select a different model.
        // The composition's authorized Upstream slot owns that decision.
        match host.begin::<Model::complete>("Upstream", &request) {
            Ok(ImportCall::Ready(result)) => finish(result),
            Ok(ImportCall::Pending(child)) => {
                state.pending = Some(child);
                PendingResult::Pending
            }
            Err(failure) => PendingResult::QuiescentFailure(failure),
        }
    }

    #[poll(Model::complete)]
    fn poll(state: &mut State, host: &Host<'_>) -> PendingResult<ModelReply, BasicError> {
        let Some(mut child) = state.pending.take() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "Advanced Model has no pending upstream call",
            ));
        };
        match child.poll(host) {
            Ok(ImportPoll::Pending | ImportPoll::Draining) => {
                state.pending = Some(child);
                PendingResult::Pending
            }
            Ok(ImportPoll::Ready(result)) => finish(result),
            Ok(ImportPoll::Cancelled) => PendingResult::QuiescentFailure(Failure::new(
                Code::ScopeRejected,
                "Advanced Model upstream cancelled",
            )),
            Err(failure) => PendingResult::QuiescentFailure(failure),
        }
    }

    #[cancel(Model::complete)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        // M1 cancels imported children and retains their resource leases.
        // This provider never detaches a model worker or replays an effect.
        state.pending = None;
        Ok(true)
    }
}
