//! Basic agent orchestration as a regular, graph-triggered native plugin.
//! The kernel owns action admission, authority, scheduling and cancellation.
use phenix_sdk::{FailurePolicy, JoinPolicy, StaticGraphContribution, StaticGraphNode};

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

#[phenix_sdk::plugin(
    id = "phenix.basic.agent",
    version = "1.0.0",
    provide(phenix_basic_contracts::BasicInput, slot = "Input"),
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
        error, BasicError, BasicInput, BasicOutput, CatalogCursor, Context, ErrorCode, Event,
        EventJournal, EventKind, EventPage, JournalAppend, JournalQuery, Message, Model,
        ModelRequest, ObserverCheckpoint, ObserverRequest, PromptSubmitted, Role, RunResult,
        RunStatus, Scope, Session, SessionSnapshot, Skills, ToolCatalog, ToolExecute, ToolInfo,
        ToolQuery, ToolRequest, Usage,
    };
    use phenix_sdk::{Blob, CallError, CallResult, Host};
    use std::collections::BTreeSet;

    #[derive(Default)]
    pub struct State;

    fn event(
        host: &Host<'_>,
        session: &str,
        run: &str,
        kind: EventKind,
        text: String,
        call_id: &str,
    ) -> CallResult<(), BasicError> {
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
                    data: Blob(vec![]),
                }],
            },
        )?;
        Ok(())
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
    }

    fn execute(host: &Host<'_>, input: PromptSubmitted) -> CallResult<RunResult, BasicError> {
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
            input.text,
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

        let mut turns = 0_u64;
        let mut calls = 0_u64;
        let mut usage = Usage::default();
        loop {
            if input.limits.model_turns.is_some_and(|max| turns >= max) {
                let problem = error(ErrorCode::TurnLimit, "model turn limit reached");
                stopped(host, session, &problem)?;
                return Err(CallError::Domain(problem));
            }
            event(
                host,
                &input.session,
                &run,
                EventKind::ModelStarted,
                String::new(),
                "",
            )?;
            let reply = host.invoke::<Model::complete>(
                "Model",
                &ModelRequest {
                    session: input.session.clone(),
                    run: run.clone(),
                    history: session.history.clone(),
                    tools: tools.clone(),
                    skills: skills.clone(),
                    context: context.clone(),
                    limits: input.limits.clone(),
                },
            );
            let reply = match reply {
                Ok(value) => value,
                Err(CallError::Domain(problem)) => {
                    stopped(host, session, &problem)?;
                    return Err(CallError::Domain(problem));
                }
                Err(other) => return Err(other),
            };
            turns = turns.checked_add(1).ok_or_else(|| {
                CallError::Domain(error(ErrorCode::TurnLimit, "model turn overflow"))
            })?;
            add_usage(&mut usage, &reply.usage)?;
            add_usage(&mut session.usage, &reply.usage)?;
            if !reply.text.is_empty() {
                event(
                    host,
                    &input.session,
                    &run,
                    EventKind::TextDelta,
                    reply.text.clone(),
                    "",
                )?;
            }

            if reply.calls.is_empty() {
                session.history.push(Message {
                    role: Role::Assistant,
                    text: reply.text.clone(),
                    calls: vec![],
                    call_id: String::new(),
                    error: None,
                });
                session.status = RunStatus::Completed;
                checkpoint(host, session)?;
                event(
                    host,
                    &input.session,
                    &run,
                    EventKind::Answer,
                    reply.text.clone(),
                    "",
                )?;
                return Ok(RunResult {
                    session: input.session,
                    run,
                    answer: reply.text,
                    model_turns: turns,
                    tool_calls: calls,
                    usage,
                });
            }

            let count = u64::try_from(reply.calls.len()).map_err(|_| {
                CallError::Domain(error(ErrorCode::TurnLimit, "too many tool calls"))
            })?;
            let new_total = calls.checked_add(count).ok_or_else(|| {
                CallError::Domain(error(ErrorCode::TurnLimit, "tool call overflow"))
            })?;
            if input.limits.tool_calls.is_some_and(|max| new_total > max) {
                let problem = error(ErrorCode::TurnLimit, "tool call limit reached");
                stopped(host, session, &problem)?;
                return Err(CallError::Domain(problem));
            }

            // Checkpoint all pending IDs before the first external tool effect.
            let mut ids = BTreeSet::new();
            for call in &reply.calls {
                if call.id.is_empty() || !ids.insert(call.id.clone()) {
                    return Err(CallError::Domain(error(
                        ErrorCode::InvalidReply,
                        "duplicate or empty tool call ID",
                    )));
                }
            }
            session.history.push(Message {
                role: Role::Assistant,
                text: reply.text,
                calls: reply.calls.clone(),
                call_id: String::new(),
                error: None,
            });
            session.pending = reply.calls.clone();
            session = checkpoint(host, session)?;

            for call in reply.calls {
                event(
                    host,
                    &input.session,
                    &run,
                    EventKind::ToolStarted,
                    call.tool.clone(),
                    &call.id,
                )?;
                let outcome = host.invoke::<ToolExecute::execute>(
                    "Tools",
                    &ToolRequest {
                        session: input.session.clone(),
                        run: run.clone(),
                        call: call.clone(),
                    },
                );
                let (text, failure) = match outcome {
                    Ok(result) => (result.text, None),
                    Err(CallError::Domain(problem)) => (String::new(), Some(problem.message)),
                    // A kernel failure does not authorize blind retry.
                    Err(other) => return Err(other),
                };
                session.history.push(Message {
                    role: Role::Tool,
                    text: text.clone(),
                    calls: vec![],
                    call_id: call.id.clone(),
                    error: failure.clone(),
                });
                session.pending.retain(|pending| pending.id != call.id);
                session = checkpoint(host, session)?;
                event(
                    host,
                    &input.session,
                    &run,
                    EventKind::ToolFinished,
                    failure.unwrap_or(text),
                    &call.id,
                )?;
            }
            calls = new_total;
        }
    }

    #[provide(BasicInput::submit)]
    fn submit(
        _: &mut State,
        host: &Host<'_>,
        request: PromptSubmitted,
    ) -> CallResult<RunResult, BasicError> {
        execute(host, request)
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
