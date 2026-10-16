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
    require(phenix_advanced_contracts::ModelTokenMeter, slot = "Meter", optional),
    // Explicit opt-in only: the public Advanced composition remains unchanged.
    // When selected, this records the complete tool-call message before any
    // Basic tool execution and never supplies its own persistence provider.
    require(phenix_basic_contracts::EventJournal, slot = "Transcript", optional)
)]
mod plugin {
    use phenix_advanced_contracts::{
        validate_model_token_measurement, ModelTokenMeter, ModelTokenProblem,
    };
    use phenix_basic_contracts::{
        error, BasicError, ErrorCode, Event, EventJournal, EventKind, JournalAppend, Message,
        Model, ModelReply, ModelRequest, Role,
    };
    use phenix_sdk::{
        CallError, CallResult, Code, Failure, Host, ImportCall, ImportPoll, PendingImport,
        PendingResult,
    };

    #[derive(Default)]
    pub struct State {
        pending: Option<PendingImport<Model::complete>>,
        pending_run: Option<(String, String)>,
    }

    fn finish(result: CallResult<ModelReply, BasicError>) -> PendingResult<ModelReply, BasicError> {
        match result {
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
            other => PendingResult::Ready(other),
        }
    }

    fn finish_with_receipt(
        host: &Host<'_>,
        session: &str,
        run: &str,
        result: CallResult<ModelReply, BasicError>,
    ) -> PendingResult<ModelReply, BasicError> {
        let Ok(reply) = &result else {
            return finish(result);
        };
        if reply.calls.is_empty() {
            return finish(result);
        }
        let count = match host.slot_count("Transcript") {
            Ok(count) => count,
            Err(failure) => return PendingResult::QuiescentFailure(failure),
        };
        if count == 0 {
            return finish(result);
        }
        let message = Message {
            role: Role::Assistant,
            text: reply.text.clone(),
            calls: reply.calls.clone(),
            call_id: String::new(),
            error: None,
        };
        let payload = match phenix_basic_contracts::encode(&message) {
            Ok(payload) => payload,
            Err(failure) => return PendingResult::QuiescentFailure(failure),
        };
        // The authorized Basic journal is the source of truth for recovery.
        // Persist the complete Model tool-call message *before* returning
        // that reply to the Basic agent, which alone executes the tools.
        let event = Event {
            sequence: 0,
            session: session.into(),
            run: run.into(),
            kind: EventKind::AssistantReply,
            call_id: String::new(),
            text: message.text,
            data: payload,
        };
        finish(
            host.invoke::<EventJournal::append>(
                "Transcript",
                &JournalAppend {
                    session: session.into(),
                    events: vec![event],
                },
            )
            .map(|_| ())
            .map(|_| reply.clone()),
        )
    }

    fn preflight_meter(host: &Host<'_>, request: &ModelRequest) -> CallResult<(), BasicError> {
        // An absent optional Meter preserves the public Advanced pass-through.
        // When present, it must be exported by the *same pinned plugin
        // instance* as the selected upstream Model, not simply claim the
        // same model-name string in a potentially untrusted measurement.
        if host.slot_count("Meter").map_err(CallError::Kernel)? == 0 {
            return Ok(());
        }
        let upstream = host
            .selected_provider::<Model::complete>("Upstream")
            .map_err(CallError::Kernel)?;
        let meter = host
            .selected_provider::<ModelTokenMeter::measure>("Meter")
            .map_err(CallError::Kernel)?;
        if upstream != meter {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidReply,
                "token meter does not share the selected Model provider",
            )));
        }
        let measurement = match host.invoke::<ModelTokenMeter::measure>("Meter", request) {
            Ok(measurement) => measurement,
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
            Ok(ImportCall::Ready(result)) => {
                finish_with_receipt(host, &request.session, &request.run, result)
            }
            Ok(ImportCall::Pending(child)) => {
                state.pending_run = Some((request.session, request.run));
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
            Ok(ImportPoll::Ready(result)) => {
                let Some((session, run)) = state.pending_run.take() else {
                    return PendingResult::QuiescentFailure(Failure::new(
                        Code::PluginFailed,
                        "Advanced Model lost pending tool-reply identity",
                    ));
                };
                finish_with_receipt(host, &session, &run, result)
            }
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
        state.pending_run = None;
        Ok(true)
    }
}
