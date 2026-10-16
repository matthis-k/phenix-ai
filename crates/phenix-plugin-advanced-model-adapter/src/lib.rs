//! P2 Advanced Model interception point.
//!
//! This is a real selected Basic Model provider, not a second agent loop.
//! It delegates each authorized request to the configured upstream Model
//! through M1 pending imports. The Basic graph owns tool progression,
//! durable sessions, journal effects and turn boundaries. The optional
//! Meter slot is an opt-in preflight for provider-specific token accounting.
//! Opted-in checkpoint reads are source-verified and selected-model metered.
//! Without a Meter binding, the public Advanced preset remains pass-through.
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
    define(phenix_advanced_contracts::CompactionStore),
    define(phenix_advanced_contracts::CompactionSummarizer),
    provide(phenix_basic_contracts::Model, slot = "Model"),
    require(phenix_basic_contracts::Model, slot = "Upstream"),
    require(phenix_advanced_contracts::ModelTokenMeter, slot = "Meter", optional),
    // Explicit opt-in only: the public Advanced composition remains unchanged.
    // When selected, this records the complete tool-call message before any
    // Basic tool execution and never supplies its own persistence provider.
    require(phenix_basic_contracts::EventJournal, slot = "Transcript", optional),
    require(phenix_advanced_contracts::CompactionStore, slot = "Checkpoints", optional),
    require(phenix_advanced_contracts::CompactionSummarizer, slot = "Summarizer", optional)
)]
mod plugin {
    use phenix_advanced_contracts::{
        validate_model_token_measurement, CheckpointStoreProblem, CompactionStore,
        CompactionSummarizer, CompactionSummaryRequest, ModelTokenMeasurement, ModelTokenMeter,
        ModelTokenProblem,
    };
    use phenix_basic_contracts::{
        error, BasicError, ErrorCode, Event, EventJournal, EventKind, JournalAppend, JournalQuery,
        Message, Model, ModelReply, ModelRequest, Role, Scope,
    };
    use phenix_p2_compaction_replay::{
        admit_compaction, assemble_verified_tail_request, checkpoint_pressure, span_for_events,
        validate_compacted_meter_transition, CheckpointPlan, CheckpointPressure,
        CheckpointPublishProblem, CompactedAdmissionProblem, CompactedMeterProblem,
        CompactionAdmissionInput, CompactionAdmissionProblem, PressureProblem,
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

    /// Opt-in only. Checkpoint selection revalidates its full ancestor ledger
    /// through the selected checkpoint provider. The selected journal supplies
    /// exact source bytes and the suffix through the current ModelStarted.
    /// The returned request is what the selected token meter and Model see.
    fn prepare_request(
        host: &Host<'_>,
        source: &ModelRequest,
    ) -> CallResult<ModelRequest, BasicError> {
        if host.slot_count("Checkpoints").map_err(CallError::Kernel)? == 0 {
            return Ok(source.clone());
        }
        if host.slot_count("Meter").map_err(CallError::Kernel)? == 0
            || host.slot_count("Transcript").map_err(CallError::Kernel)? == 0
        {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidReply,
                "compaction requires a selected token meter and source journal",
            )));
        }
        let checkpoint = match host.invoke::<CompactionStore::latest>(
            "Checkpoints",
            &Scope {
                session: source.session.clone(),
            },
        ) {
            Ok(checkpoint) => checkpoint,
            Err(CallError::Domain(failure))
                if failure.problem == CheckpointStoreProblem::NotFound =>
            {
                return prepare_automatic_checkpoint(host, source);
            }
            Err(CallError::Domain(_)) => {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "compaction checkpoint source or lineage rejected",
                )));
            }
            Err(CallError::Kernel(failure)) => return Err(CallError::Kernel(failure)),
        };
        const PAGE_EVENTS: u64 = 128;
        const MAX_READ_EVENTS: usize = 2048;
        const MAX_PAGES: usize = 32;
        let original_count = checkpoint
            .input
            .last_sequence
            .checked_sub(checkpoint.input.first_sequence)
            .and_then(|difference| difference.checked_add(1))
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(|| {
                CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "invalid checkpoint source count",
                ))
            })?;
        let mut after = checkpoint
            .input
            .first_sequence
            .checked_sub(1)
            .ok_or_else(|| {
                CallError::Domain(error(ErrorCode::InvalidReply, "invalid checkpoint start"))
            })?;
        let mut events = Vec::<Event>::new();
        // A full journal page does not certify EOF. Read through the first
        // empty page, with a fixed upper bound on both events and import
        // invocations. Every page must advance an exact contiguous cursor.
        for _ in 0..MAX_PAGES {
            let page = host.invoke::<EventJournal::read>(
                "Transcript",
                &JournalQuery {
                    session: source.session.clone(),
                    after,
                    limit: PAGE_EVENTS,
                },
            )?;
            if page.events.is_empty() {
                if page.next != after || events.is_empty() {
                    return Err(CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "compaction journal ended at an invalid cursor",
                    )));
                }
                let (original, appended) =
                    events.split_at_checked(original_count).ok_or_else(|| {
                        CallError::Domain(error(
                            ErrorCode::InvalidReply,
                            "checkpoint source events missing",
                        ))
                    })?;
                return assemble_verified_tail_request(
                    source,
                    &checkpoint,
                    original,
                    appended,
                    8 * 1024 * 1024,
                )
                .map_err(|_| {
                    CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "compaction source history did not match the authoritative session",
                    ))
                });
            }
            if page.events.len() > PAGE_EVENTS as usize {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "compaction journal page exceeded its limit",
                )));
            }
            let mut next = after;
            for event in &page.events {
                next = next.checked_add(1).ok_or_else(|| {
                    CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "compaction journal sequence overflow",
                    ))
                })?;
                if event.sequence != next || event.session != source.session {
                    return Err(CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "compaction journal contains a gap or foreign session",
                    )));
                }
            }
            if page.next != next {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "compaction journal advanced an invalid cursor",
                )));
            }
            events.extend(page.events);
            if events.len() > MAX_READ_EVENTS {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "compaction journal exceeds bounded read capacity",
                )));
            }
            after = next;
        }
        Err(CallError::Domain(error(
            ErrorCode::InvalidReply,
            "compaction journal pagination exceeded its bounded page count",
        )))
    }

    /// An opt-in verifier may automatically publish a checkpoint only at
    /// measured pressure, through the same selected source, CAS and Model
    /// bindings. Default public Advanced has no Summarizer and is unchanged.
    fn prepare_automatic_checkpoint(
        host: &Host<'_>,
        source: &ModelRequest,
    ) -> CallResult<ModelRequest, BasicError> {
        if host.slot_count("Summarizer").map_err(CallError::Kernel)? == 0 {
            return Ok(source.clone());
        }
        let measurement = selected_measurement(host, source)?;
        let pressure = checkpoint_pressure(source, &measurement, &measurement.model, 80).map_err(
            |reason| {
                CallError::Domain(error(
                    if reason == PressureProblem::Measurement(ModelTokenProblem::OverBudget) {
                        ErrorCode::TurnLimit
                    } else {
                        ErrorCode::InvalidReply
                    },
                    "automatic compaction pressure measurement rejected",
                ))
            },
        )?;
        if pressure == CheckpointPressure::EnoughHeadroom {
            return Ok(source.clone());
        }
        const PAGE_EVENTS: u64 = 128;
        const MAX_SOURCE_EVENTS: usize = 1024;
        let mut after = 0_u64;
        let mut journal = Vec::<Event>::new();
        loop {
            let page = host.invoke::<EventJournal::read>(
                "Transcript",
                &JournalQuery {
                    session: source.session.clone(),
                    after,
                    limit: PAGE_EVENTS,
                },
            )?;
            if page.events.is_empty() {
                if page.next != after || journal.is_empty() {
                    return Err(CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "automatic compaction source ended at an invalid cursor",
                    )));
                }
                break;
            }
            if page.events.len() > PAGE_EVENTS as usize {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "automatic compaction source page exceeded its limit",
                )));
            }
            let mut next = after;
            for event in &page.events {
                next = next.checked_add(1).ok_or_else(|| {
                    CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "automatic compaction source sequence overflow",
                    ))
                })?;
                if event.sequence != next || event.session != source.session {
                    return Err(CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "automatic compaction source contains a gap or foreign session",
                    )));
                }
            }
            if page.next != next {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "automatic compaction source advanced invalid cursor",
                )));
            }
            journal.extend(page.events);
            if journal.len() > MAX_SOURCE_EVENTS {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "automatic compaction source exceeds bounded capacity",
                )));
            }
            after = next;
        }
        let span = span_for_events(&journal).map_err(|_| {
            CallError::Domain(error(
                ErrorCode::InvalidReply,
                "automatic compaction source digest rejected",
            ))
        })?;
        let proposed = match host.invoke::<CompactionSummarizer::summarize>(
            "Summarizer",
            &CompactionSummaryRequest {
                request: source.clone(),
                journal: journal.clone(),
                source: span.clone(),
            },
        ) {
            Ok(summary) => summary,
            Err(CallError::Kernel(failure)) => return Err(CallError::Kernel(failure)),
            Err(CallError::Domain(_)) => {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "selected summarizer refused the source",
                )));
            }
        };
        if proposed.source != span
            || proposed.text.trim().is_empty()
            || proposed.text.len() > 65_536
        {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidReply,
                "selected summarizer returned an invalid source citation",
            )));
        }
        let id = format!("cp-auto-{}", span.digest_sha256);
        let candidate = admit_compaction(
            CompactionAdmissionInput {
                request: source,
                original_measurement: &measurement,
                selected_model: &measurement.model,
                high_water_percent: 80,
                journal: &journal,
                previous: None,
                policy: CheckpointPlan {
                    id: &id,
                    revision: 1,
                    parent_checkpoint: None,
                    summary: &proposed.text,
                    retain_completed_turns: 1,
                },
            },
            || match host.invoke::<CompactionStore::latest>(
                "Checkpoints",
                &Scope {
                    session: source.session.clone(),
                },
            ) {
                Ok(latest) => Ok(Some(latest)),
                Err(CallError::Domain(failure))
                    if failure.problem == CheckpointStoreProblem::NotFound =>
                {
                    Ok(None)
                }
                Err(CallError::Domain(_)) => Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "checkpoint source rejected",
                ))),
                Err(CallError::Kernel(failure)) => Err(CallError::Kernel(failure)),
            },
            |candidate| {
                host.invoke::<CompactionStore::put>("Checkpoints", candidate)
                    .map_err(|failure| match failure {
                        CallError::Kernel(kernel) => CallError::Kernel(kernel),
                        CallError::Domain(_) => CallError::Domain(error(
                            ErrorCode::InvalidReply,
                            "checkpoint CAS publication rejected",
                        )),
                    })
            },
            |rebuilt| selected_measurement(host, rebuilt),
        )
        .map_err(|problem| match problem {
            CompactionAdmissionProblem::Publication(CheckpointPublishProblem::Store(failure))
            | CompactionAdmissionProblem::Remeasure(CompactedAdmissionProblem::Meter(failure)) => {
                failure
            }
            CompactionAdmissionProblem::Remeasure(
                CompactedAdmissionProblem::CandidateMeasurement(ModelTokenProblem::OverBudget),
            ) => CallError::Domain(error(
                ErrorCode::TurnLimit,
                "compacted Model request still exceeds context window",
            )),
            _ => CallError::Domain(error(
                ErrorCode::InvalidReply,
                "automatic checkpoint verification or CAS failed",
            )),
        })?;
        if candidate.checkpoint.is_none() {
            return Err(CallError::Domain(error(
                ErrorCode::TurnLimit,
                "automatic compaction had no source-verified completed turns",
            )));
        }
        Ok(candidate.request)
    }

    /// Measure the exact request through the same pinned provider selected
    /// for upstream Model execution. Selection does not confer a grant.
    fn selected_measurement(
        host: &Host<'_>,
        request: &ModelRequest,
    ) -> CallResult<ModelTokenMeasurement, BasicError> {
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
        match host.invoke::<ModelTokenMeter::measure>("Meter", request) {
            Ok(measurement) => Ok(measurement),
            Err(CallError::Kernel(failure)) => Err(CallError::Kernel(failure)),
            Err(CallError::Domain(_)) => Err(CallError::Domain(error(
                ErrorCode::ModelFailed,
                "selected Model token meter failed",
            ))),
        }
    }

    fn preflight_meter(host: &Host<'_>, request: &ModelRequest) -> CallResult<(), BasicError> {
        // An absent optional Meter preserves the public Advanced pass-through.
        if host.slot_count("Meter").map_err(CallError::Kernel)? == 0 {
            return Ok(());
        }
        let measurement = selected_measurement(host, request)?;
        // Reserve the selected Model's full completion limit.
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

    /// The source journal and checkpoint were already read through selected,
    /// scoped imports and reassembled byte-for-byte. Verify the original
    /// ModelRequest even when it exceeds the context window; its compacted
    /// successor must then fit under the identical Model configuration.
    fn preflight_compacted(
        host: &Host<'_>,
        original: &ModelRequest,
        rebuilt: &ModelRequest,
    ) -> CallResult<(), BasicError> {
        let original_measurement = selected_measurement(host, original)?;
        checkpoint_pressure(
            original,
            &original_measurement,
            &original_measurement.model,
            100,
        )
        .map_err(|problem| {
            let code = if problem == PressureProblem::Measurement(ModelTokenProblem::OverBudget) {
                ErrorCode::TurnLimit
            } else {
                ErrorCode::InvalidReply
            };
            CallError::Domain(error(code, "original Model token measurement rejected"))
        })?;
        let rebuilt_measurement = selected_measurement(host, rebuilt)?;
        validate_compacted_meter_transition(
            original,
            rebuilt,
            &original_measurement,
            &rebuilt_measurement,
            &original_measurement.model,
        )
        .map_err(|problem| {
            let code = if problem
                == CompactedMeterProblem::CandidateMeasurement(ModelTokenProblem::OverBudget)
            {
                ErrorCode::TurnLimit
            } else {
                ErrorCode::InvalidReply
            };
            CallError::Domain(error(
                code,
                "compacted Model configuration or token count rejected",
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
        // A source-verified request is metered exactly as sent. When a
        // checkpoint changes the request, independently measure the original
        // and rebuilt requests under one selected Model configuration.
        let source = request;
        let request = match prepare_request(host, &source) {
            Ok(request) => request,
            Err(failure) => return finish(Err(failure)),
        };
        let checked = if request == source {
            preflight_meter(host, &request)
        } else {
            preflight_compacted(host, &source, &request)
        };
        if let Err(failure) = checked {
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
