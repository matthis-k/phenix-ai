//! P2 Advanced Model interception point.
//!
//! This is a real selected Basic Model provider, not a second agent loop.
//! It delegates each authorized request to the configured upstream Model
//! through M1 pending imports. The Basic graph owns tool progression,
//! durable sessions, journal effects and turn boundaries. The optional
//! Meter slot is an opt-in preflight for provider-specific token accounting.
//! Until source-backed compaction admission exists, the original request is
//! submitted unchanged. Without a Meter binding, forwarding is unchanged.
#[phenix_sdk::plugin(
    id = "phenix.advanced.model-adapter",
    version = "1.0.0",
    // M1 intermediate providers need an explicit *ceiling* for descendant
    // Model streaming: Journal::append delegates to Storage::transact.
    // These definitions grant no binding or independent effect.
    define(phenix_basic_contracts::EventJournal),
    define(phenix_basic_contracts::Storage),
    // Explicitly declare the maximum downstream token-meter interface.
    // A selected Model consumer need not import this Advanced-only schema.
    define(phenix_advanced_contracts::ModelTokenMeter),
    provide(phenix_basic_contracts::Model, slot = "Model"),
    require(phenix_basic_contracts::Model, slot = "Upstream"),
    require(phenix_advanced_contracts::ModelTokenMeter, slot = "Meter", optional)
)]
mod plugin {
    use phenix_advanced_contracts::{
        validate_model_token_measurement, ModelTokenMeter, ModelTokenProblem,
    };
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

    fn preflight_meter(host: &Host<'_>, request: &ModelRequest) -> CallResult<(), BasicError> {
        let measurement = match host.invoke::<ModelTokenMeter::measure>("Meter", request) {
            Ok(measurement) => measurement,
            // Optional slots are absent from the standard Advanced preset.
            // All other import failures must propagate rather than disable
            // a configured preflight.
            Err(CallError::Kernel(failure)) if failure.code == Code::MissingBinding => {
                return Ok(());
            }
            Err(CallError::Kernel(failure)) => return Err(CallError::Kernel(failure)),
            Err(CallError::Domain(_)) => {
                return Err(CallError::Domain(error(
                    ErrorCode::ModelFailed,
                    "selected Model token meter failed",
                )));
            }
        };
        // model_tokens is the selected Model's completion-token ceiling.
        // Reserve the full amount; never fabricate a default output reserve
        // for an explicitly metered composition.
        let output_reserve = request
            .limits
            .model_tokens
            .filter(|tokens| *tokens > 0)
            .ok_or_else(|| {
                CallError::Domain(error(
                    ErrorCode::TurnLimit,
                    "metered Model requires an explicit positive token budget",
                ))
            })?;
        validate_model_token_measurement(request, &measurement, &measurement.model, output_reserve)
            .map_err(|reason| {
                CallError::Domain(error(
                    if reason == ModelTokenProblem::OverBudget {
                        ErrorCode::TurnLimit
                    } else {
                        ErrorCode::InvalidReply
                    },
                    "selected Model token measurement rejected",
                ))
            })
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
        // Composition must co-select Upstream and Meter from one Model
        // provider/configuration. The measurement's model name alone is not
        // proof of that provider identity.
        if let Err(failure) = preflight_meter(host, &request) {
            return finish(Err(failure));
        }
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
