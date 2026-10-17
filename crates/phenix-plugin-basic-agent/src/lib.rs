//! Basic agent orchestration as a regular, graph-triggered native plugin.
//! The kernel owns action admission, authority, scheduling and cancellation.
use phenix_basic_contracts::{ContextItem, Limits, Message, ModelRequest, Skill, ToolInfo};
use phenix_sdk::{FailurePolicy, JoinPolicy, StaticGraphContribution, StaticGraphNode};

/// Pure snapshot projection for the authoritative Basic-to-Model boundary.
///
/// A future graph-managed prepare node must consume this exact typed value;
/// it must not reconstruct history, tools or scopes independently. This
/// function neither initiates a Model call nor changes Basic session state.
pub fn project_model_request(
    session: &str,
    run: &str,
    history: &[Message],
    tools: &[ToolInfo],
    skills: &[Skill],
    context: &[ContextItem],
    limits: &Limits,
) -> ModelRequest {
    ModelRequest {
        session: session.into(),
        run: run.into(),
        history: history.to_vec(),
        tools: tools.to_vec(),
        skills: skills.to_vec(),
        context: context.to_vec(),
        limits: limits.clone(),
    }
}

/// Fail closed during the first staged graph integration: a preparer may
/// observe the Basic request but cannot rewrite *any* execution byte yet.
/// Source-backed compaction requires journal/CAS/meter admission in the
/// later selected Advanced graph contribution.
#[cfg(feature = "staged-graph")]
pub fn verify_staged_model_candidate(
    original: &ModelRequest,
    candidate: &ModelRequest,
) -> Result<(), phenix_basic_contracts::BasicError> {
    if candidate != original {
        return Err(phenix_basic_contracts::error(
            phenix_basic_contracts::ErrorCode::Denied,
            "prepared Model request differs from the authoritative Basic projection",
        ));
    }
    Ok(())
}

#[cfg(not(feature = "staged-graph"))]
const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.basic.agent",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "submit",
        triggers: &["phenix.basic.prompt"],
        after: &[],
        selector: "",
        operation: "phenix.basic.input@1/submit",
        writes: &[],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[cfg(feature = "staged-graph")]
const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.basic.agent",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "prepare",
        triggers: &["phenix.basic.prompt"],
        after: &[],
        selector: "",
        operation: "phenix.basic.turn-prepare@1/prepare",
        writes: &["phenix.basic.agent/pending_model_request"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }, StaticGraphNode {
        id: "model-advance",
        triggers: &["state:phenix.basic.agent/prepared_model_request"],
        after: &[],
        selector: "",
        operation: "phenix.basic.turn-advance@1/advance",
        writes: &[],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[cfg(all(test, feature = "staged-graph"))]
#[path = "staged_graph_tests.rs"]
mod staged_graph_tests;

#[phenix_sdk::plugin(
    id = "phenix.basic.agent",
    version = "1.0.0",
    provide(phenix_basic_contracts::BasicInput, slot = "Input"),
    provide(phenix_basic_contracts::BasicTurnPrepare, slot = "Prepare"),
    provide(phenix_basic_contracts::BasicTurnAdvance, slot = "Advance"),
    provide(phenix_basic_contracts::BasicOutput, slot = "Output"),
    require(phenix_basic_contracts::Model, slot = "Model"),
    require(phenix_basic_contracts::ToolCatalog, slot = "Catalog"),
    require(phenix_basic_contracts::ToolExecute, slot = "Tools"),
    require(phenix_basic_contracts::Skills, slot = "Skills"),
    require(phenix_basic_contracts::Context, slot = "Context"),
    require(phenix_basic_contracts::Session, slot = "Session"),
    require(phenix_basic_contracts::EventJournal, slot = "Journal"),
    // Preserve Storage authority for nested Session, Journal and memory calls.
    // The agent does not select or invoke a Storage provider itself.
    require(phenix_basic_contracts::Storage, slot = "Storage", optional),
    graph(super::GRAPH)
)]
mod plugin {
    use phenix_basic_contracts::{
        error, BasicError, BasicInput, BasicOutput, BasicTurnAdvance, BasicTurnPrepare, CatalogCursor, Context,
        ErrorCode, Event, EventJournal, EventKind, EventPage, JournalAppend, JournalQuery, Message,
        GraphStateChange, Model, ModelRequest, ObserverCheckpoint, ObserverRequest, PromptSubmitted, Role, RunResult,
        RunStatus, Scope, Session, SessionSnapshot, Skills, ToolCatalog, ToolCompletion,
        ToolExecute, ToolInfo, ToolQuery, ToolRequest, Usage,
    };
    use phenix_sdk::{
        Blob, CallError, CallResult, Code, Failure, Host, ImportCall, ImportPoll, PendingImport,
        PendingResult,
    };
    use std::collections::BTreeSet;

    struct Running {
        input: PromptSubmitted,
        session: SessionSnapshot,
        run: String,
        tools: Vec<ToolInfo>,
        skills: Vec<phenix_basic_contracts::Skill>,
        context: Vec<phenix_basic_contracts::ContextItem>,
        turns: u64,
        calls: u64,
        usage: Usage,
        model_after: u64,
        child: Option<PendingImport<Model::complete>>,
    }

    #[derive(Default)]
    pub struct State {
        active: Option<Running>,
        #[get]
        #[set]
        #[watch]
        pending_model_request: Option<ModelRequest>,
        #[get]
        #[set]
        #[watch]
        prepared_model_request: Option<ModelRequest>,
    }

    fn event(
        host: &Host<'_>,
        session: &str,
        run: &str,
        kind: EventKind,
        text: String,
        call_id: &str,
    ) -> CallResult<u64, BasicError> {
        event_with_data(host, session, run, kind, text, call_id, Blob(vec![]))
    }

    fn event_with_data(
        host: &Host<'_>,
        session: &str,
        run: &str,
        kind: EventKind,
        text: String,
        call_id: &str,
        data: Blob,
    ) -> CallResult<u64, BasicError> {
        host.invoke::<EventJournal::append>(
            "Journal",
            &JournalAppend {
                session: session.into(),
                events: vec![Event {
                    sequence: 0,
                    session: session.into(),
                    run: run.into(),
                    kind,
                    call_id: call_id.into(),
                    text,
                    data,
                }],
            },
        )
    }

    fn checkpoint(
        host: &Host<'_>,
        current: SessionSnapshot,
    ) -> CallResult<SessionSnapshot, BasicError> {
        host.invoke::<Session::checkpoint>("Session", &current)
    }

    fn catalog(host: &Host<'_>, session: &str) -> CallResult<Vec<ToolInfo>, BasicError> {
        let mut tools = Vec::new();
        let mut cursor: Option<CatalogCursor> = None;
        let mut visited = BTreeSet::new();
        loop {
            let page = host.invoke::<ToolCatalog::list>(
                "Catalog",
                &ToolQuery {
                    session: session.into(),
                    query: String::new(),
                    cursor,
                    limit: 64,
                },
            )?;
            tools.extend(page.tools);
            cursor = page.next;
            let Some(next) = cursor.as_ref() else {
                break;
            };
            // A broken provider may not trap the agent in an endless
            // pagination loop.
            let key = (next.revision, next.offset, next.query.clone());
            if !visited.insert(key) {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidCursor,
                    "catalog cursor repeated",
                )));
            }
        }
        let mut names = BTreeSet::new();
        if tools
            .iter()
            .any(|tool| tool.id.is_empty() || !names.insert(tool.id.clone()))
        {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidReply,
                "invalid or duplicate catalog tool",
            )));
        }
        Ok(tools)
    }

    fn add_usage(total: &mut Usage, extra: &Usage) -> CallResult<(), BasicError> {
        total.input_tokens = total
            .input_tokens
            .checked_add(extra.input_tokens)
            .ok_or_else(|| {
                CallError::Domain(error(ErrorCode::TurnLimit, "input usage overflow"))
            })?;
        total.output_tokens = total
            .output_tokens
            .checked_add(extra.output_tokens)
            .ok_or_else(|| {
                CallError::Domain(error(ErrorCode::TurnLimit, "output usage overflow"))
            })?;
        Ok(())
    }

    fn stopped(
        host: &Host<'_>,
        mut state: SessionSnapshot,
        error: &BasicError,
    ) -> CallResult<(), BasicError> {
        state.status = RunStatus::Failed;
        let current = checkpoint(host, state)?;
        event(
            host,
            &current.session,
            &current.run,
            EventKind::Failure,
            error.message.clone(),
            "",
        )
        .map(|_| ())
    }

    fn prepare(host: &Host<'_>, input: PromptSubmitted) -> CallResult<Running, BasicError> {
        phenix_basic_contracts::check_session(host, &input.session)?;
        if input.text.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "empty prompt",
            )));
        }

        let mut session = host.invoke::<Session::open>(
            "Session",
            &Scope {
                session: input.session.clone(),
            },
        )?;
        if session.status == RunStatus::Indeterminate || !session.pending.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::Indeterminate,
                "unreconciled tool calls",
            )));
        }
        if session.status == RunStatus::Running {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "a run is already active",
            )));
        }
        let next = session
            .checkpoint
            .checked_add(1)
            .ok_or_else(|| CallError::Domain(error(ErrorCode::Conflict, "run counter overflow")))?;
        let run = format!("run-{next}");
        session.run = run.clone();
        session.status = RunStatus::Running;
        session.checkpoint = next;
        session.history.push(Message {
            role: Role::User,
            text: input.text.clone(),
            calls: vec![],
            call_id: String::new(),
            error: None,
        });
        session = checkpoint(host, session)?;
        event(
            host,
            &input.session,
            &run,
            EventKind::PromptAccepted,
            input.text.clone(),
            "",
        )?;
        let tools = catalog(host, &input.session)?;
        let skills = host.invoke::<Skills::list>(
            "Skills",
            &Scope {
                session: input.session.clone(),
            },
        )?;
        let context = host.invoke::<Context::list>(
            "Context",
            &Scope {
                session: input.session.clone(),
            },
        )?;
        Ok(Running {
            input,
            session,
            run,
            tools,
            skills,
            context,
            turns: 0,
            calls: 0,
            usage: Usage::default(),
            model_after: 0,
            child: None,
        })
    }

    // A pending Model may have already committed live deltas. A synchronous
    // Model has not. Account for both without depending on provider identity.
    fn journal_tail(
        host: &Host<'_>,
        session: &str,
        mut after: u64,
    ) -> CallResult<(u64, Vec<Event>), BasicError> {
        let mut collected = Vec::new();
        loop {
            let page = host.invoke::<EventJournal::read>(
                "Journal",
                &JournalQuery {
                    session: session.into(),
                    after,
                    limit: 128,
                },
            )?;
            if page.events.is_empty() {
                return Ok((after, collected));
            }
            if page.next <= after {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "journal cursor did not advance",
                )));
            }
            after = page.next;
            collected.extend(page.events);
        }
    }

    fn reconcile_text(
        host: &Host<'_>,
        running: &Running,
        text: &str,
    ) -> CallResult<(), BasicError> {
        let (_, events) = journal_tail(host, &running.input.session, running.model_after)?;
        let streamed: String = events
            .iter()
            .filter(|entry| entry.run == running.run && entry.kind == EventKind::TextDelta)
            .map(|entry| entry.text.as_str())
            .collect();
        let Some(remaining) = text.strip_prefix(&streamed) else {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidReply,
                "model progress disagrees with final reply",
            )));
        };
        if !remaining.is_empty() {
            event(
                host,
                &running.input.session,
                &running.run,
                EventKind::TextDelta,
                remaining.into(),
                "",
            )?;
        }
        Ok(())
    }

    /// Canonical boundary shared by ordinary P1 and staged P2 requests.
    /// It accounts for turn limits and records the exact source journal
    /// boundary before publishing any derived Model view.
    fn prepare_next_model_request(
        host: &Host<'_>,
        running: &mut Running,
    ) -> CallResult<ModelRequest, BasicError> {
        if running
            .input
            .limits
            .model_turns
            .is_some_and(|max| running.turns >= max)
        {
            let problem = error(ErrorCode::TurnLimit, "model turn limit reached");
            stopped(host, running.session.clone(), &problem)?;
            return Err(CallError::Domain(problem));
        }
        running.model_after = event(
            host,
            &running.input.session,
            &running.run,
            EventKind::ModelStarted,
            String::new(),
            "",
        )?;
        Ok(super::project_model_request(
            &running.input.session,
            &running.run,
            &running.session.history,
            &running.tools,
            &running.skills,
            &running.context,
            &running.input.limits,
        ))
    }

    #[provide(BasicTurnPrepare::prepare)]
    fn prepare_graph(
        state: &mut State,
        host: &Host<'_>,
        request: PromptSubmitted,
    ) -> CallResult<ModelRequest, BasicError> {
        if state.active.is_some() {
            return Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "a Basic run is already active",
            )));
        }
        let mut running = prepare(host, request)?;
        let projected = prepare_next_model_request(host, &mut running)?;
        // Publishing the watched snapshot is the only graph handoff here.
        // Model::complete is NOT invoked by this operation. A later graph
        // contribution will validate a selected replacement and resume the
        // same run, without re-creating a new Basic agent loop.
        let mut edit = state
            .pending_model_request
            .edit(host)
            .map_err(|err| Failure::new(Code::PluginFailed, err.to_string()))?;
        *edit = Some(projected.clone());
        edit.commit(host)
            .map_err(|err| Failure::new(Code::PluginFailed, err.to_string()))?;
        state.active = Some(running);
        Ok(projected)
    }

    enum Advance {
        Pending,
        Done(RunResult),
    }

    // Each model reply, immediate or polled, resumes this same continuation.
    // Tool effects remain synchronous until the separate pending-tools slice.
    fn advance(
        host: &Host<'_>,
        running: &mut Running,
        mut reply: Option<CallResult<phenix_basic_contracts::ModelReply, BasicError>>,
    ) -> CallResult<Advance, BasicError> {
        loop {
            if let Some(result) = reply.take() {
                let response = match result {
                    Ok(response) => response,
                    Err(CallError::Domain(problem)) => {
                        stopped(host, running.session.clone(), &problem)?;
                        return Err(CallError::Domain(problem));
                    }
                    Err(other) => return Err(other),
                };
                running.turns = running.turns.checked_add(1).ok_or_else(|| {
                    CallError::Domain(error(ErrorCode::TurnLimit, "model turn overflow"))
                })?;
                add_usage(&mut running.usage, &response.usage)?;
                add_usage(&mut running.session.usage, &response.usage)?;
                match reconcile_text(host, running, &response.text) {
                    Ok(()) => {}
                    Err(CallError::Domain(problem)) => {
                        stopped(host, running.session.clone(), &problem)?;
                        return Err(CallError::Domain(problem));
                    }
                    Err(other) => return Err(other),
                }

                if response.calls.is_empty() {
                    running.session.history.push(Message {
                        role: Role::Assistant,
                        text: response.text.clone(),
                        calls: vec![],
                        call_id: String::new(),
                        error: None,
                    });
                    running.session.status = RunStatus::Completed;
                    checkpoint(host, running.session.clone())?;
                    event(
                        host,
                        &running.input.session,
                        &running.run,
                        EventKind::Answer,
                        response.text.clone(),
                        "",
                    )?;
                    return Ok(Advance::Done(RunResult {
                        session: running.input.session.clone(),
                        run: running.run.clone(),
                        answer: response.text,
                        model_turns: running.turns,
                        tool_calls: running.calls,
                        usage: running.usage.clone(),
                    }));
                }

                let count = u64::try_from(response.calls.len()).map_err(|_| {
                    CallError::Domain(error(ErrorCode::TurnLimit, "too many tool calls"))
                })?;
                let new_total = running.calls.checked_add(count).ok_or_else(|| {
                    CallError::Domain(error(ErrorCode::TurnLimit, "tool call overflow"))
                })?;
                if running
                    .input
                    .limits
                    .tool_calls
                    .is_some_and(|max| new_total > max)
                {
                    let problem = error(ErrorCode::TurnLimit, "tool call limit reached");
                    stopped(host, running.session.clone(), &problem)?;
                    return Err(CallError::Domain(problem));
                }
                let mut ids = BTreeSet::new();
                for call in &response.calls {
                    if call.id.is_empty() || !ids.insert(call.id.clone()) {
                        let problem =
                            error(ErrorCode::InvalidReply, "duplicate or empty tool call ID");
                        stopped(host, running.session.clone(), &problem)?;
                        return Err(CallError::Domain(problem));
                    }
                }
                running.session.history.push(Message {
                    role: Role::Assistant,
                    text: response.text,
                    calls: response.calls.clone(),
                    call_id: String::new(),
                    error: None,
                });
                running.session.pending = response.calls.clone();
                running.session = checkpoint(host, running.session.clone())?;

                for call in response.calls {
                    event(
                        host,
                        &running.input.session,
                        &running.run,
                        EventKind::ToolStarted,
                        call.tool.clone(),
                        &call.id,
                    )?;
                    let outcome = host.invoke::<ToolExecute::execute>(
                        "Tools",
                        &ToolRequest {
                            session: running.input.session.clone(),
                            run: running.run.clone(),
                            call: call.clone(),
                        },
                    );
                    let (text, failure) = match outcome {
                        Ok(result) => (result.text, None),
                        Err(CallError::Domain(problem)) => (String::new(), Some(problem.message)),
                        Err(other) => return Err(other),
                    };
                    running.session.history.push(Message {
                        role: Role::Tool,
                        text: text.clone(),
                        calls: vec![],
                        call_id: call.id.clone(),
                        error: failure.clone(),
                    });
                    running
                        .session
                        .pending
                        .retain(|pending| pending.id != call.id);
                    running.session = checkpoint(host, running.session.clone())?;
                    let data = phenix_basic_contracts::encode(&ToolCompletion {
                        failed: failure.is_some(),
                    })
                    .map_err(CallError::Kernel)?;
                    event_with_data(
                        host,
                        &running.input.session,
                        &running.run,
                        EventKind::ToolFinished,
                        failure.unwrap_or(text),
                        &call.id,
                        data,
                    )?;
                }
                running.calls = new_total;
            }

            let request = prepare_next_model_request(host, running)?;
            match host
                .begin::<Model::complete>("Model", &request)
                .map_err(CallError::Kernel)?
            {
                ImportCall::Ready(result) => reply = Some(result),
                ImportCall::Pending(child) => {
                    running.child = Some(child);
                    return Ok(Advance::Pending);
                }
            }
        }
    }

    fn finish(
        state: &mut State,
        running: Running,
        value: CallResult<Advance, BasicError>,
    ) -> PendingResult<RunResult, BasicError> {
        match value {
            Ok(Advance::Pending) => {
                state.active = Some(running);
                PendingResult::Pending
            }
            Ok(Advance::Done(reply)) => PendingResult::Ready(Ok(reply)),
            Err(CallError::Domain(error)) => PendingResult::Ready(Err(CallError::Domain(error))),
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
        }
    }

    // Opt-in two-wave admission: Basic has already published the canonical
    // request. The only currently valid replacement is byte-for-byte equal
    // to that projection. Checkpoints and altered history remain unadmitted
    // until the selected Advanced graph verifier is composed.
    #[pending(BasicTurnAdvance::advance)]
    fn begin_model_advance(
        state: &mut State,
        host: &Host<'_>,
        notice: GraphStateChange,
    ) -> PendingResult<RunResult, BasicError> {
        if notice.epoch == 0
            || notice.properties != ["phenix.basic.agent/prepared_model_request"]
        {
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Denied,
                "Model advance requires a committed prepared-request state change",
            ))));
        }
        let source = match state.pending_model_request.read(host) {
            Ok(snapshot) => snapshot.value.as_ref().clone(),
            Err(failure) => {
                return PendingResult::QuiescentFailure(Failure::new(
                    Code::PluginFailed,
                    failure.to_string(),
                ));
            }
        };
        let prepared = match state.prepared_model_request.read(host) {
            Ok(snapshot) => snapshot.value.as_ref().clone(),
            Err(failure) => {
                return PendingResult::QuiescentFailure(Failure::new(
                    Code::PluginFailed,
                    failure.to_string(),
                ));
            }
        };
        let (Some(source), Some(prepared)) = (source, prepared) else {
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Denied,
                "Model advance has no published Basic and prepared requests",
            ))));
        };
        if let Err(error) = super::verify_staged_model_candidate(&source, &prepared) {
            return PendingResult::Ready(Err(CallError::Domain(error)));
        }
        let Some(mut running) = state.active.take() else {
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "Model advance has no authoritative Basic run",
            ))));
        };
        if running.run != source.run || running.input.session != source.session || running.child.is_some() {
            state.active = Some(running);
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Denied,
                "prepared request is not the active Basic turn",
            ))));
        }
        match host.begin::<Model::complete>("Model", &prepared) {
            Ok(ImportCall::Ready(reply)) => {
                let result = staged_reply(host, &mut running, reply);
                finish(state, running, result)
            }
            Ok(ImportCall::Pending(child)) => {
                running.child = Some(child);
                state.active = Some(running);
                PendingResult::Pending
            }
            Err(failure) => PendingResult::QuiescentFailure(failure),
        }
    }

    // Intentionally fail closed on model-request iterations with tools.
    // The next step must reenter the watched preparation graph after any
    // tool result; executing them here would bypass the Advanced gate.
    fn staged_reply(
        host: &Host<'_>,
        running: &mut Running,
        reply: CallResult<phenix_basic_contracts::ModelReply, BasicError>,
    ) -> CallResult<Advance, BasicError> {
        if reply
            .as_ref()
            .is_ok_and(|response| !response.calls.is_empty())
        {
            let problem = error(
                ErrorCode::Denied,
                "staged Model tool continuation requires graph re-preparation",
            );
            stopped(host, running.session.clone(), &problem)?;
            return Err(CallError::Domain(problem));
        }
        advance(host, running, Some(reply))
    }

    #[poll(BasicTurnAdvance::advance)]
    fn poll_model_advance(state: &mut State, host: &Host<'_>) -> PendingResult<RunResult, BasicError> {
        let Some(mut running) = state.active.take() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "staged pending Basic run is missing",
            ));
        };
        let Some(mut child) = running.child.take() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "staged pending Model child is missing",
            ));
        };
        match child.poll(host) {
            Ok(ImportPoll::Pending | ImportPoll::Draining) => {
                running.child = Some(child);
                state.active = Some(running);
                PendingResult::Pending
            }
            Ok(ImportPoll::Ready(reply)) => {
                let result = staged_reply(host, &mut running, reply);
                finish(state, running, result)
            }
            Ok(ImportPoll::Cancelled) => PendingResult::QuiescentFailure(Failure::new(
                Code::ScopeRejected,
                "staged Model child was cancelled",
            )),
            Err(failure) => PendingResult::QuiescentFailure(failure),
        }
    }

    #[cancel(BasicTurnAdvance::advance)]
    fn cancel_model_advance(state: &mut State) -> Result<bool, Failure> {
        state.active = None;
        Ok(true)
    }

    #[pending(BasicInput::submit)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: PromptSubmitted,
    ) -> PendingResult<RunResult, BasicError> {
        if cfg!(feature = "staged-graph") {
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Denied,
                "staged Basic requires the graph preparation admission path",
            ))));
        }
        if state.active.is_some() {
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Conflict,
                "a Basic run is already active",
            ))));
        }
        match prepare(host, request) {
            Ok(mut running) => {
                let value = advance(host, &mut running, None);
                finish(state, running, value)
            }
            Err(CallError::Domain(error)) => PendingResult::Ready(Err(CallError::Domain(error))),
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
        }
    }

    #[poll(BasicInput::submit)]
    fn poll(state: &mut State, host: &Host<'_>) -> PendingResult<RunResult, BasicError> {
        let Some(mut running) = state.active.take() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "Basic pending run is missing",
            ));
        };
        let Some(mut child) = running.child.take() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "Basic pending Model is missing",
            ));
        };
        match child.poll(host) {
            Ok(ImportPoll::Pending | ImportPoll::Draining) => {
                running.child = Some(child);
                state.active = Some(running);
                PendingResult::Pending
            }
            Ok(ImportPoll::Ready(reply)) => {
                let result = advance(host, &mut running, Some(reply));
                finish(state, running, result)
            }
            Ok(ImportPoll::Cancelled) => PendingResult::QuiescentFailure(Failure::new(
                Code::ScopeRejected,
                "Basic Model import was cancelled",
            )),
            Err(failure) => PendingResult::QuiescentFailure(failure),
        }
    }

    #[cancel(BasicInput::submit)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        // The kernel cancels imported children before this callback and
        // retains their physical leases. The agent owns no worker thread.
        state.active = None;
        Ok(true)
    }

    #[provide(BasicOutput::read)]
    fn read(
        _: &mut State,
        host: &Host<'_>,
        request: JournalQuery,
    ) -> CallResult<EventPage, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        host.invoke::<EventJournal::read>("Journal", &request)
    }

    #[provide(BasicOutput::observer)]
    fn observer(
        _: &mut State,
        host: &Host<'_>,
        request: ObserverRequest,
    ) -> CallResult<ObserverCheckpoint, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        host.invoke::<EventJournal::observer>("Journal", &request)
    }

    #[provide(BasicOutput::checkpoint_observer)]
    fn checkpoint_observer(
        _: &mut State,
        host: &Host<'_>,
        request: ObserverCheckpoint,
    ) -> CallResult<ObserverCheckpoint, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        host.invoke::<EventJournal::checkpoint_observer>("Journal", &request)
    }
}
