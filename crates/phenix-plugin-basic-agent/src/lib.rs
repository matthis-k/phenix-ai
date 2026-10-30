//! Basic agent orchestration as a regular, graph-triggered native plugin.
//! The kernel owns action admission, authority, scheduling and cancellation.
use phenix_basic_contracts::{ContextItem, Limits, Message, ModelRequest, Skill, ToolInfo};
#[cfg(not(feature = "graph-v2"))]
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

// Provider-contributed catalog pages are untrusted. Distinct cursor offsets
// alone do not prevent a malicious catalog from keeping the graph busy forever.
// This is the Basic agent's bounded discovery budget, not a kernel limit.
const MAX_CATALOG_PAGES: usize = 64;
const MAX_CATALOG_TOOLS: usize = 1024;
// Basic must not trust an arbitrary EventJournal implementation to bound
// replay work or honor the query's sequence/session identity.
const JOURNAL_PAGE_LIMIT: u64 = 128;
const MAX_JOURNAL_PAGES: usize = 64;

fn validate_journal_page_with_limit(
    session: &str,
    after: u64,
    limit: u64,
    page: &phenix_basic_contracts::EventPage,
) -> Result<(), phenix_basic_contracts::BasicError> {
    let invalid = || {
        phenix_basic_contracts::error(
            phenix_basic_contracts::ErrorCode::InvalidReply,
            "journal returned an invalid event page",
        )
    };
    if limit == 0 || u64::try_from(page.events.len()).map_or(true, |count| count > limit) {
        return Err(invalid());
    }
    let mut sequence = after;
    for event in &page.events {
        sequence = sequence.checked_add(1).ok_or_else(invalid)?;
        if event.sequence != sequence || event.session != session || event.run.is_empty() {
            return Err(invalid());
        }
    }
    if page.next != sequence {
        return Err(invalid());
    }
    Ok(())
}

fn validate_journal_page(
    session: &str,
    after: u64,
    page: &phenix_basic_contracts::EventPage,
) -> Result<(), phenix_basic_contracts::BasicError> {
    validate_journal_page_with_limit(session, after, JOURNAL_PAGE_LIMIT, page)
}

fn admit_catalog_page(
    pages: usize,
    accumulated: usize,
    incoming: usize,
) -> Result<(), phenix_basic_contracts::BasicError> {
    if pages >= MAX_CATALOG_PAGES
        || accumulated
            .checked_add(incoming)
            .is_none_or(|total| total > MAX_CATALOG_TOOLS)
    {
        return Err(phenix_basic_contracts::error(
            phenix_basic_contracts::ErrorCode::InvalidReply,
            "untrusted tool catalog exceeded Basic discovery budget",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod catalog_budget_tests {
    use super::*;

    #[test]
    fn finite_catalog_accepts_last_budgeted_page_and_tool() {
        assert!(admit_catalog_page(0, 0, 64).is_ok());
        assert!(admit_catalog_page(MAX_CATALOG_PAGES - 1, 1023, 1).is_ok());
    }

    #[test]
    fn distinct_unbounded_cursors_and_excessive_tools_fail_closed() {
        assert_eq!(
            admit_catalog_page(MAX_CATALOG_PAGES, 12, 1)
                .unwrap_err()
                .code,
            phenix_basic_contracts::ErrorCode::InvalidReply
        );
        assert_eq!(
            admit_catalog_page(2, MAX_CATALOG_TOOLS, 1)
                .unwrap_err()
                .code,
            phenix_basic_contracts::ErrorCode::InvalidReply
        );
        assert!(admit_catalog_page(1, usize::MAX, 1).is_err());
    }
}

/// Fail closed during the first staged graph integration: a preparer may
/// observe the Basic request but cannot rewrite *any* execution byte yet.
/// Source-backed compaction requires journal/CAS/meter admission in the
/// later selected Advanced graph contribution.
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

#[cfg(test)]
mod journal_page_tests {
    use super::*;
    use phenix_basic_contracts::{Event, EventKind, EventPage};
    use phenix_sdk::Blob;

    fn event(sequence: u64, session: &str) -> Event {
        Event {
            sequence,
            session: session.into(),
            run: "run-1".into(),
            kind: EventKind::TextDelta,
            call_id: String::new(),
            text: "delta".into(),
            data: Blob(vec![]),
        }
    }

    #[test]
    fn valid_contiguous_pages_and_empty_terminal_page() {
        let page = EventPage {
            events: vec![event(11, "s1"), event(12, "s1")],
            next: 12,
        };
        assert!(validate_journal_page("s1", 10, &page).is_ok());
        assert!(validate_journal_page(
            "s1",
            12,
            &EventPage {
                events: vec![],
                next: 12
            }
        )
        .is_ok());
    }

    #[test]
    fn output_page_validation_honors_original_request_limit() {
        let page = EventPage {
            events: vec![event(11, "s1"), event(12, "s1")],
            next: 12,
        };
        assert!(validate_journal_page_with_limit("s1", 10, 2, &page).is_ok());
        assert!(validate_journal_page_with_limit("s1", 10, 1, &page).is_err());
        assert!(validate_journal_page_with_limit("s1", 10, 0, &page).is_err());
        assert!(validate_journal_page_with_limit("s1", 10, u64::MAX, &page).is_ok());
    }

    #[test]
    fn rejects_cross_session_gaps_cursor_mismatch_and_invalid_run() {
        let good = EventPage {
            events: vec![event(11, "s1"), event(12, "s1")],
            next: 12,
        };
        let mut cases = Vec::new();
        let mut foreign = good.clone();
        foreign.events[0].session = "s2".into();
        cases.push(foreign);
        let mut gap = good.clone();
        gap.events[1].sequence = 13;
        cases.push(gap);
        let mut duplicate = good.clone();
        duplicate.events[1].sequence = 11;
        cases.push(duplicate);
        let mut wrong_cursor = good.clone();
        wrong_cursor.next = 13;
        cases.push(wrong_cursor);
        let mut empty_run = good.clone();
        empty_run.events[0].run.clear();
        cases.push(empty_run);
        cases.push(EventPage {
            events: vec![],
            next: 13,
        });
        for case in cases {
            assert_eq!(
                validate_journal_page("s1", 10, &case).unwrap_err().code,
                phenix_basic_contracts::ErrorCode::InvalidReply,
            );
        }
    }

    #[test]
    fn oversized_and_overflowing_pages_fail_closed() {
        let large = EventPage {
            events: (1..=JOURNAL_PAGE_LIMIT + 1)
                .map(|n| event(n, "s1"))
                .collect(),
            next: JOURNAL_PAGE_LIMIT + 1,
        };
        assert!(validate_journal_page("s1", 0, &large).is_err());
        let wrapped = EventPage {
            events: vec![event(0, "s1")],
            next: 0,
        };
        assert!(validate_journal_page("s1", u64::MAX, &wrapped).is_err());
    }
}

#[cfg(all(feature = "graph-v2", feature = "staged-graph"))]
compile_error!("graph-v2 and staged-graph are separate opt-in compositions");

#[cfg(not(any(feature = "staged-graph", feature = "graph-v2")))]
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
    }],
};

#[cfg(all(test, feature = "staged-graph"))]
#[path = "staged_graph_tests.rs"]
mod staged_graph_tests;

#[cfg(not(feature = "graph-v2"))]
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    phenix_sdk::descriptor::static_metadata::graph(GRAPH).native_section();

#[cfg(feature = "graph-v2")]
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph-v2.bin"));

#[phenix_sdk::plugin(
    id = "phenix.basic.agent",
    version = "1.0.0",
    provide(phenix_basic_contracts::BasicInput, slot = "Input"),
    provide(phenix_basic_contracts::BasicTurnPrepare, slot = "Prepare"),
    provide(phenix_basic_contracts::BasicGraphTurn, slot = "GraphTurn"),
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
    // Bound only by the opt-in Advanced graph composition.
    require(phenix_basic_contracts::AdvancedPreparedModelRequest, slot = "Prepared", optional)
)]
mod plugin {
    use phenix_basic_contracts::{
        error, AdvancedPreparedModelRequest, BasicError, BasicInput, BasicOutput, BasicTurnAdvance,
        BasicTurnPrepare, CatalogCursor, Context, ErrorCode, Event, EventJournal, EventKind,
        EventPage, GraphStateChange, JournalAppend, JournalQuery, Message, Model, ModelRequest,
        ObserverCheckpoint, ObserverRequest, PromptSubmitted, RunResult, RunStatus, Scope, Session,
        SessionSnapshot, Skills, ToolCatalog, ToolCompletion, ToolExecute, ToolInfo, ToolOutcome,
        ToolQuery, ToolRequest, TurnAdvanceOutcome, Usage,
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
        graph_reply: Option<phenix_basic_contracts::ModelReply>,
        // The staged request is authoritative in this plugin. The graph
        // consumes a read-only, watched Phenix view of this Rust field.
        #[expose]
        #[watch]
        pending_model_request: Option<ModelRequest>,
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
        let mut pages = 0;
        loop {
            let query = ToolQuery {
                session: session.into(),
                query: String::new(),
                cursor,
                limit: 64,
            };
            let page = host.invoke::<ToolCatalog::list>("Catalog", &query)?;
            page.validate_for(&query).map_err(CallError::Domain)?;
            super::admit_catalog_page(pages, tools.len(), page.tools.len())
                .map_err(CallError::Domain)?;
            pages += 1;
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
        session.history.push(Message::user(input.text.clone()));
        session = checkpoint(host, session)?;
        let initialization = (|| {
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
            Ok((tools, skills, context))
        })();
        let (tools, skills, context) = match initialization {
            Ok(ready) => ready,
            Err(failure) => {
                // No Basic worker was installed. Never strand a durable
                // Running checkpoint if journal/catalog/skills/context fails.
                session.status = RunStatus::Failed;
                checkpoint(host, session)?;
                return Err(failure);
            }
        };
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
        let mut pages = 0;
        loop {
            let page = host.invoke::<EventJournal::read>(
                "Journal",
                &JournalQuery {
                    session: session.into(),
                    after,
                    limit: super::JOURNAL_PAGE_LIMIT,
                },
            )?;
            super::validate_journal_page(session, after, &page).map_err(CallError::Domain)?;
            if page.events.is_empty() {
                return Ok((after, collected));
            }
            // The final empty-page probe must remain possible after exactly
            // MAX_JOURNAL_PAGES full pages. Reject a 65th nonempty page.
            if pages == super::MAX_JOURNAL_PAGES {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidReply,
                    "journal pagination budget exceeded",
                )));
            }
            pages += 1;
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
        // Publication CAS succeeds before the owned request is committed.
        // A failed publish leaves the plugin's previous request untouched.
        #[cfg(not(feature = "graph-v2"))]
        state
            .publish_pending_model_request(host, Some(projected.clone()))
            .map_err(|err| {
                Failure::new(Code::PluginFailed, format!("staged request publish: {err}"))
            })?;
        // V2 passes the request on a typed Flow edge, not a watched-state wave.
        #[cfg(feature = "graph-v2")]
        {
            state.pending_model_request = Some(projected.clone());
        }
        state.active = Some(running);
        Ok(projected)
    }

    enum Advance {
        Pending,
        Done(RunResult),
        RePrepared,
    }

    #[pending(phenix_basic_contracts::BasicGraphTurn::model)]
    fn graph_model(
        state: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> PendingResult<phenix_basic_contracts::ModelReply, BasicError> {
        let admission = (|| {
            let source = state.pending_model_request.as_ref().ok_or_else(|| {
                CallError::Domain(error(ErrorCode::Conflict, "no prepared graph turn"))
            })?;
            super::verify_staged_model_candidate(source, &request).map_err(CallError::Domain)?;
            if state.active.as_ref().is_none_or(|running| {
                running.run != request.run
                    || running.input.session != request.session
                    || running.child.is_some()
            }) {
                return Err(CallError::Domain(error(
                    ErrorCode::Conflict,
                    "no active graph run",
                )));
            }
            // Consume admission before effects so a duplicate stage cannot call Model twice.
            state.pending_model_request = None;
            Ok(())
        })();
        if let Err(failure) = admission {
            return PendingResult::Ready(Err(failure));
        }
        // A final first reply traverses the fixed continuation stage without
        // invoking Model again or accounting the reply twice.
        if let Some(reply) = state.graph_reply.as_ref() {
            if reply.calls.is_empty() {
                return PendingResult::Ready(Ok(reply.clone()));
            }
        }
        match host.begin::<Model::complete>("Model", &request) {
            Ok(ImportCall::Ready(reply)) => graph_model_result(state, host, reply),
            Ok(ImportCall::Pending(child)) => {
                state
                    .active
                    .as_mut()
                    .expect("validated active graph run")
                    .child = Some(child);
                PendingResult::Pending
            }
            Err(failure) => graph_model_result(state, host, Err(CallError::Kernel(failure))),
        }
    }

    fn graph_model_result(
        state: &mut State,
        host: &Host<'_>,
        result: CallResult<phenix_basic_contracts::ModelReply, BasicError>,
    ) -> PendingResult<phenix_basic_contracts::ModelReply, BasicError> {
        match result {
            Ok(reply) => {
                state.graph_reply = Some(reply.clone());
                PendingResult::Ready(Ok(reply))
            }
            Err(failure) => {
                let running = state.active.take().expect("validated active graph run");
                state.graph_reply = None;
                state.pending_model_request = None;
                match failure {
                    CallError::Kernel(failure) => reconcile_model_failure(host, &running, failure),
                    CallError::Domain(problem) => match stopped(host, running.session, &problem) {
                        Ok(()) => PendingResult::Ready(Err(CallError::Domain(problem))),
                        Err(CallError::Domain(cleanup)) => {
                            PendingResult::Ready(Err(CallError::Domain(cleanup)))
                        }
                        Err(CallError::Kernel(cleanup)) => PendingResult::QuiescentFailure(cleanup),
                    },
                }
            }
        }
    }

    #[poll(phenix_basic_contracts::BasicGraphTurn::model)]
    fn poll_graph_model(
        state: &mut State,
        host: &Host<'_>,
    ) -> PendingResult<phenix_basic_contracts::ModelReply, BasicError> {
        let Some(mut child) = state
            .active
            .as_mut()
            .and_then(|running| running.child.take())
        else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "graph pending Model child is missing",
            ));
        };
        match child.poll(host) {
            Ok(ImportPoll::Pending | ImportPoll::Draining) => {
                state.active.as_mut().expect("active graph run").child = Some(child);
                PendingResult::Pending
            }
            Ok(ImportPoll::Ready(reply)) => graph_model_result(state, host, reply),
            Ok(ImportPoll::Cancelled) => graph_model_result(
                state,
                host,
                Err(CallError::Kernel(Failure::new(
                    Code::ScopeRejected,
                    "graph Model child was cancelled",
                ))),
            ),
            Err(failure) => graph_model_result(state, host, Err(CallError::Kernel(failure))),
        }
    }

    #[cancel(phenix_basic_contracts::BasicGraphTurn::model)]
    fn cancel_graph_model(state: &mut State) -> Result<bool, Failure> {
        // Child cancellation and physical drain remain kernel-owned.
        state.active = None;
        state.pending_model_request = None;
        state.graph_reply = None;
        Ok(true)
    }

    #[provide(phenix_basic_contracts::BasicGraphTurn::tool)]
    fn graph_tool(
        state: &mut State,
        host: &Host<'_>,
        reply: phenix_basic_contracts::ModelReply,
    ) -> CallResult<ModelRequest, BasicError> {
        if state.graph_reply.as_ref() != Some(&reply) {
            return Err(CallError::Domain(error(
                ErrorCode::Denied,
                "not the admitted Model reply",
            )));
        }
        let mut running = state
            .active
            .take()
            .ok_or_else(|| CallError::Domain(error(ErrorCode::Conflict, "no active graph run")))?;
        if running.turns != 0 {
            let problem = error(
                ErrorCode::InvalidReply,
                "graph slice requires an initial reply",
            );
            stopped(host, running.session, &problem)?;
            return Err(CallError::Domain(problem));
        }
        if reply.calls.is_empty() {
            let next = super::project_model_request(
                &running.input.session,
                &running.run,
                &running.session.history,
                &running.tools,
                &running.skills,
                &running.context,
                &running.input.limits,
            );
            state.pending_model_request = Some(next.clone());
            state.active = Some(running);
            return Ok(next);
        }
        state.graph_reply = None;
        let next = match (|| {
            handle_model_reply(host, &mut running, Ok(reply))?;
            prepare_next_model_request(host, &mut running)
        })() {
            Ok(next) => next,
            Err(failure) => {
                // Reconcile the durable snapshot, not partially applied in-memory
                // tool results. Uncommitted effects must retain pending calls.
                let current = host.invoke::<Session::open>(
                    "Session",
                    &Scope {
                        session: running.input.session.clone(),
                    },
                )?;
                if current.status == RunStatus::Running {
                    match failure {
                        CallError::Kernel(failure) => {
                            running.session = current;
                            return match reconcile_model_failure(host, &running, failure) {
                                PendingResult::QuiescentFailure(failure) => {
                                    Err(CallError::Kernel(failure))
                                }
                                PendingResult::Ready(result) => result,
                                _ => unreachable!("failure reconciliation is synchronous"),
                            };
                        }
                        CallError::Domain(ref problem) => stopped(host, current, problem)?,
                    }
                }
                return Err(failure);
            }
        };
        state.pending_model_request = Some(next.clone());
        state.active = Some(running);
        Ok(next)
    }

    #[provide(phenix_basic_contracts::BasicGraphTurn::finish)]
    fn graph_finish(
        state: &mut State,
        host: &Host<'_>,
        reply: phenix_basic_contracts::ModelReply,
    ) -> CallResult<RunResult, BasicError> {
        if state.graph_reply.as_ref() != Some(&reply) {
            return Err(CallError::Domain(error(
                ErrorCode::Denied,
                "not the admitted Model reply",
            )));
        }
        state.graph_reply = None;
        let mut running = state
            .active
            .take()
            .ok_or_else(|| CallError::Domain(error(ErrorCode::Conflict, "no active graph run")))?;
        if running.turns > 1 || !reply.calls.is_empty() {
            let problem = error(
                ErrorCode::InvalidReply,
                "graph continuation requires final answer",
            );
            stopped(host, running.session, &problem)?;
            return Err(CallError::Domain(problem));
        }
        handle_model_reply(host, &mut running, Ok(reply))?.ok_or_else(|| {
            CallError::Domain(error(ErrorCode::InvalidReply, "missing graph answer"))
        })
    }

    // Consume one completed Model reply: account usage and streaming text,
    // then either finalize the answer or execute the requested tool calls.
    // Returns Some(result) for a terminal answer and None after tool effects.
    fn handle_model_reply(
        host: &Host<'_>,
        running: &mut Running,
        result: CallResult<phenix_basic_contracts::ModelReply, BasicError>,
    ) -> CallResult<Option<RunResult>, BasicError> {
        let response = match result {
            Ok(response) => response,
            Err(CallError::Domain(problem)) => {
                stopped(host, running.session.clone(), &problem)?;
                return Err(CallError::Domain(problem));
            }
            Err(other) => return Err(other),
        };
        running.turns = running
            .turns
            .checked_add(1)
            .ok_or_else(|| CallError::Domain(error(ErrorCode::TurnLimit, "model turn overflow")))?;
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
            running
                .session
                .history
                .push(Message::assistant(response.text.clone(), vec![]));
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
            return Ok(Some(RunResult {
                session: running.input.session.clone(),
                run: running.run.clone(),
                answer: response.text,
                model_turns: running.turns,
                tool_calls: running.calls,
                usage: running.usage.clone(),
            }));
        }

        let count = u64::try_from(response.calls.len())
            .map_err(|_| CallError::Domain(error(ErrorCode::TurnLimit, "too many tool calls")))?;
        let new_total = running
            .calls
            .checked_add(count)
            .ok_or_else(|| CallError::Domain(error(ErrorCode::TurnLimit, "tool call overflow")))?;
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
                let problem = error(ErrorCode::InvalidReply, "duplicate or empty tool call ID");
                stopped(host, running.session.clone(), &problem)?;
                return Err(CallError::Domain(problem));
            }
        }
        running
            .session
            .history
            .push(Message::assistant(response.text, response.calls.clone()));
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
            let tool_outcome = match outcome {
                Ok(result) => ToolOutcome::Succeeded { text: result.text },
                Err(CallError::Domain(problem)) => ToolOutcome::Failed {
                    output: String::new(),
                    reason: problem.message,
                },
                Err(other) => return Err(other),
            };
            let (completion, event_text) = match &tool_outcome {
                ToolOutcome::Succeeded { text } => (ToolCompletion::Succeeded, text.clone()),
                ToolOutcome::Failed { reason, .. } => (ToolCompletion::Failed, reason.clone()),
            };
            running
                .session
                .history
                .push(Message::tool(call.id.clone(), tool_outcome));
            running
                .session
                .pending
                .retain(|pending| pending.id != call.id);
            running.session = checkpoint(host, running.session.clone())?;
            let data = phenix_basic_contracts::encode(&completion).map_err(CallError::Kernel)?;
            event_with_data(
                host,
                &running.input.session,
                &running.run,
                EventKind::ToolFinished,
                event_text,
                &call.id,
                data,
            )?;
        }
        running.calls = new_total;
        Ok(None)
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
                if let Some(done) = handle_model_reply(host, running, result)? {
                    return Ok(Advance::Done(done));
                }
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

    // Preserve a terminal session checkpoint when a Model import fails at the
    // kernel boundary. The journal and session owner must not disagree after
    // the root fails. If cleanup fails, leave the durable state fail-closed.
    fn reconcile_model_failure<R>(
        host: &Host<'_>,
        running: &Running,
        failure: Failure,
    ) -> PendingResult<R, BasicError> {
        let problem = error(ErrorCode::ModelFailed, "Model execution failed");
        match stopped(host, running.session.clone(), &problem) {
            Ok(()) => PendingResult::QuiescentFailure(failure),
            Err(CallError::Kernel(cleanup)) => PendingResult::QuiescentFailure(cleanup),
            Err(CallError::Domain(cleanup)) => {
                PendingResult::Ready(Err(CallError::Domain(cleanup)))
            }
        }
    }

    fn finish(
        state: &mut State,
        host: &Host<'_>,
        running: Running,
        value: CallResult<Advance, BasicError>,
    ) -> PendingResult<RunResult, BasicError> {
        match value {
            Ok(Advance::Pending) => {
                state.active = Some(running);
                PendingResult::Pending
            }
            Ok(Advance::RePrepared) => PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "default Basic cannot publish a staged continuation",
            )),
            Ok(Advance::Done(reply)) => PendingResult::Ready(Ok(reply)),
            Err(CallError::Domain(error)) => PendingResult::Ready(Err(CallError::Domain(error))),
            Err(CallError::Kernel(failure)) => reconcile_model_failure(host, &running, failure),
        }
    }

    // Staged continuation is a graph wave, not an answer. Do not return a
    // fabricated success with empty text while the Session is still Running.
    fn finish_staged(
        state: &mut State,
        host: &Host<'_>,
        running: Running,
        value: CallResult<Advance, BasicError>,
    ) -> PendingResult<TurnAdvanceOutcome, BasicError> {
        match value {
            Ok(Advance::Pending) => {
                state.active = Some(running);
                PendingResult::Pending
            }
            Ok(Advance::RePrepared) => {
                state.active = Some(running);
                PendingResult::Ready(Ok(TurnAdvanceOutcome::Reprepare))
            }
            Ok(Advance::Done(reply)) => {
                PendingResult::Ready(Ok(TurnAdvanceOutcome::Complete { result: reply }))
            }
            Err(CallError::Domain(problem)) => {
                PendingResult::Ready(Err(CallError::Domain(problem)))
            }
            Err(CallError::Kernel(failure)) => reconcile_model_failure(host, &running, failure),
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
    ) -> PendingResult<TurnAdvanceOutcome, BasicError> {
        if notice.epoch == 0
            || notice.properties != ["phenix.advanced.graph-preparer/prepared_model_request"]
        {
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Denied,
                "Model advance requires a committed prepared-request state change",
            ))));
        }
        let source = state.pending_model_request.clone();
        let selected = match AdvancedPreparedModelRequest::get::locate(host, "Prepared") {
            Ok(selected) => selected,
            Err(failure) => return PendingResult::QuiescentFailure(failure),
        };
        let prepared = match selected.read(host) {
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
        if running.run != source.run
            || running.input.session != source.session
            || running.child.is_some()
        {
            state.active = Some(running);
            return PendingResult::Ready(Err(CallError::Domain(error(
                ErrorCode::Denied,
                "prepared request is not the active Basic turn",
            ))));
        }
        match host.begin::<Model::complete>("Model", &prepared) {
            Ok(ImportCall::Ready(reply)) => {
                let result = staged_reply(state, host, &mut running, reply);
                finish_staged(state, host, running, result)
            }
            Ok(ImportCall::Pending(child)) => {
                running.child = Some(child);
                state.active = Some(running);
                PendingResult::Pending
            }
            Err(failure) => reconcile_model_failure(host, &running, failure),
        }
    }

    // After a tool-bearing Model reply, execute the tool effects and publish
    // the next authoritative request as a fresh watched pending cell. The
    // kernel then re-enters the selected Advanced preparation graph before
    // the next Model call, instead of continuing inside this advance wave.
    fn staged_reply(
        state: &mut State,
        host: &Host<'_>,
        running: &mut Running,
        reply: CallResult<phenix_basic_contracts::ModelReply, BasicError>,
    ) -> CallResult<Advance, BasicError> {
        if let Some(done) = handle_model_reply(host, running, reply)? {
            return Ok(Advance::Done(done));
        }
        let next = prepare_next_model_request(host, running)?;
        state
            .publish_pending_model_request(host, Some(next))
            .map_err(|err| {
                Failure::new(Code::PluginFailed, format!("staged request publish: {err}"))
            })?;
        Ok(Advance::RePrepared)
    }

    #[poll(BasicTurnAdvance::advance)]
    fn poll_model_advance(
        state: &mut State,
        host: &Host<'_>,
    ) -> PendingResult<TurnAdvanceOutcome, BasicError> {
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
                let result = staged_reply(state, host, &mut running, reply);
                finish_staged(state, host, running, result)
            }
            Ok(ImportPoll::Cancelled) => PendingResult::QuiescentFailure(Failure::new(
                Code::ScopeRejected,
                "staged Model child was cancelled",
            )),
            Err(failure) => reconcile_model_failure(host, &running, failure),
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
        if cfg!(any(feature = "staged-graph", feature = "graph-v2")) {
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
                finish(state, host, running, value)
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
                finish(state, host, running, result)
            }
            Ok(ImportPoll::Cancelled) => PendingResult::QuiescentFailure(Failure::new(
                Code::ScopeRejected,
                "Basic Model import was cancelled",
            )),
            Err(failure) => reconcile_model_failure(host, &running, failure),
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
        let page = host.invoke::<EventJournal::read>("Journal", &request)?;
        super::validate_journal_page_with_limit(
            &request.session,
            request.after,
            request.limit,
            &page,
        )
        .map_err(CallError::Domain)?;
        Ok(page)
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
