//! RED P2-3f: build model input only from the authoritative Basic
//! SessionSnapshot history, aligned byte-for-byte against source journal.
use phenix_advanced_contracts::CompactionCheckpoint;
use phenix_basic_contracts::{
    ContextItem, Event, EventKind, Limits, Message, ModelRequest, Role, ToolCall, ToolInfo,
};
use phenix_p2_compaction_replay::{
    assemble_verified_basic_request, compacted_view, select_model_view, span_for_events,
    TranscriptProblem,
};
use phenix_sdk::Blob;

fn event(sequence: u64, kind: EventKind, text: &str) -> Event {
    Event {
        sequence,
        session: "s1".into(),
        run: format!("run-{}", sequence.div_ceil(2)),
        kind,
        call_id: String::new(),
        text: text.into(),
        data: Blob(vec![]),
    }
}
fn source() -> Vec<Event> {
    vec![
        event(1, EventKind::PromptAccepted, "older question"),
        event(2, EventKind::Answer, "older answer"),
        event(3, EventKind::PromptAccepted, "recent question"),
        event(4, EventKind::Answer, "recent answer"),
        event(5, EventKind::PromptAccepted, "current question"),
    ]
}
fn checkpoint(events: &[Event]) -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: "cp-1".into(),
        session: "s1".into(),
        revision: 1,
        parent_checkpoint: None,
        input: span_for_events(events).unwrap(),
        summarized: vec![span_for_events(&events[..2]).unwrap()],
        retained: vec![span_for_events(&events[2..]).unwrap()],
        excluded: vec![],
        summary: "Old turn anchored in journal".into(),
        policy: "text-only".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}
fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        text: text.into(),
        calls: vec![],
        call_id: String::new(),
        error: None,
    }
}
fn original_request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "run-3".into(),
        history: vec![
            message(Role::User, "older question"),
            message(Role::Assistant, "older answer"),
            message(Role::User, "recent question"),
            message(Role::Assistant, "recent answer"),
            message(Role::User, "current question"),
        ],
        tools: vec![ToolInfo {
            id: "uppercase".into(),
            description: "source tool".into(),
            arguments_schema: r#"{"type":"string"}"#.into(),
            idempotent: true,
        }],
        skills: vec![],
        context: vec![ContextItem {
            id: "existing".into(),
            text: "unchanged".into(),
        }],
        limits: Limits::default(),
    }
}

#[test]
fn compacts_only_verified_prefix_and_preserves_complete_model_request_fields() {
    let events = source();
    let cp = checkpoint(&events);
    let view = compacted_view(&cp, &events, 4096).unwrap();
    let full = original_request();
    let actual = assemble_verified_basic_request(&full, &events, &cp, &view).unwrap();
    assert_eq!(actual.history, full.history[2..]);
    assert_eq!(actual.context[0], full.context[0]);
    assert_eq!(actual.context[1].id, "compaction:cp-1");
    assert_eq!(actual.context[1].text, "Old turn anchored in journal");
    assert_eq!(actual.tools, full.tools);
    assert_eq!(actual.skills, full.skills);
    assert_eq!(actual.limits, full.limits);
    assert_eq!(actual.session, full.session);
    assert_eq!(actual.run, full.run);
    assert_eq!(
        full.history.len(),
        5,
        "must not mutate Basic source history"
    );
}

#[test]
fn mismatch_or_missing_session_message_fails_before_model_submission() {
    let events = source();
    let cp = checkpoint(&events);
    let view = compacted_view(&cp, &events, 4096).unwrap();
    let mut forged = original_request();
    forged.history[1].text = "rewritten answer".into();
    assert_eq!(
        assemble_verified_basic_request(&forged, &events, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );
    forged = original_request();
    forged.history.remove(0);
    assert_eq!(
        assemble_verified_basic_request(&forged, &events, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );
    forged = original_request();
    forged.session = "other".into();
    assert_eq!(
        assemble_verified_basic_request(&forged, &events, &cp, &view),
        Err(TranscriptProblem::WrongSession),
    );
}

#[test]
fn tool_calls_are_not_reconstructed_from_incomplete_journal_events() {
    let events = source();
    let cp = checkpoint(&events);
    let view = compacted_view(&cp, &events, 4096).unwrap();
    let mut tools = original_request();
    tools.history[3].calls.push(ToolCall {
        id: "t1".into(),
        tool: "uppercase".into(),
        arguments: r#""secret argument""#.into(),
    });
    assert_eq!(
        assemble_verified_basic_request(&tools, &events, &cp, &view),
        Err(TranscriptProblem::UnsupportedToolHistory),
    );
    let mut recorded_tool = events.clone();
    recorded_tool[2].kind = EventKind::ToolStarted;
    recorded_tool[2].call_id = "t1".into();
    recorded_tool[3].kind = EventKind::ToolFinished;
    recorded_tool[3].call_id = "t1".into();
    let tool_cp = checkpoint(&recorded_tool);
    let tool_view = compacted_view(&tool_cp, &recorded_tool, 4096).unwrap();
    assert_eq!(
        assemble_verified_basic_request(&original_request(), &recorded_tool, &tool_cp, &tool_view),
        Err(TranscriptProblem::UnsupportedToolHistory),
    );
}

#[test]
fn splitting_a_completed_user_answer_turn_fails_closed() {
    let events = source();
    let mut cp = checkpoint(&events);
    cp.summarized = vec![span_for_events(&events[..1]).unwrap()];
    cp.retained = vec![span_for_events(&events[1..]).unwrap()];
    let view = compacted_view(&cp, &events, 4096).unwrap();
    assert_eq!(
        assemble_verified_basic_request(&original_request(), &events, &cp, &view),
        Err(TranscriptProblem::SplitTurn),
    );
}

#[test]
fn cannot_override_existing_context_id_or_omit_current_prompt() {
    let events = source();
    let cp = checkpoint(&events);
    let view = compacted_view(&cp, &events, 4096).unwrap();
    let mut request = original_request();
    request.context.push(ContextItem {
        id: "compaction:cp-1".into(),
        text: "forged prior summary".into(),
    });
    assert_eq!(
        assemble_verified_basic_request(&request, &events, &cp, &view),
        Err(TranscriptProblem::ContextConflict),
    );
    let mut source_only_old = original_request();
    source_only_old.history.truncate(4);
    assert_eq!(
        assemble_verified_basic_request(&source_only_old, &events, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );
}

#[test]
fn selected_provider_measures_the_same_provenance_checked_request_it_will_submit() {
    let events = source();
    let cp = checkpoint(&events);
    let authoritative = original_request();
    let selected = select_model_view(
        std::slice::from_ref(&cp),
        &events,
        4096,
        150,
        |view| assemble_verified_basic_request(&authoritative, &events, &cp, view),
        |request| {
            // The provider receives the full exact request, not a journal
            // reconstruction. Tool definitions, context, and session history
            // must all participate in actual model-specific accounting.
            assert_eq!(request.history, authoritative.history[2..]);
            assert_eq!(request.tools, authoritative.tools);
            assert_eq!(request.context.len(), 2);
            Ok::<_, TranscriptProblem>(120)
        },
    )
    .unwrap();
    assert_eq!(selected.model_input_tokens, 120);
    assert_eq!(selected.request.history.len(), 3);
    assert_eq!(selected.view.checkpoint_id, "cp-1");
    assert_eq!(authoritative.history.len(), 5);
}

#[test]
fn forged_projection_cannot_inject_uncited_model_context() {
    let events = source();
    let cp = checkpoint(&events);
    let mut view = compacted_view(&cp, &events, 4096).unwrap();
    if let phenix_p2_compaction_replay::ContextPart::Summary { text, .. } = &mut view.parts[0] {
        *text = "unsupported fabricated memory".into();
    } else {
        panic!("expected prefix summary");
    }
    assert_eq!(
        assemble_verified_basic_request(&original_request(), &events, &cp, &view),
        Err(TranscriptProblem::UnalignedSource),
    );
}

fn tool_source() -> Vec<Event> {
    let mut events = vec![
        event(1, EventKind::PromptAccepted, "older question"),
        event(2, EventKind::Answer, "older answer"),
        event(3, EventKind::PromptAccepted, "tool question"),
        event(4, EventKind::ModelStarted, ""),
        event(5, EventKind::ToolStarted, "uppercase"),
        event(6, EventKind::ToolFinished, "HELLO"),
        event(7, EventKind::ModelStarted, ""),
        event(8, EventKind::Answer, "tool completed"),
        event(9, EventKind::PromptAccepted, "current question"),
    ];
    for entry in &mut events[2..8] {
        entry.run = "run-2".into();
    }
    events[8].run = "run-3".into();
    events[4].call_id = "t1".into();
    events[5].call_id = "t1".into();
    events
}
fn tool_request() -> ModelRequest {
    let mut request = original_request();
    request.history = vec![
        message(Role::User, "older question"),
        message(Role::Assistant, "older answer"),
        message(Role::User, "tool question"),
        Message {
            role: Role::Assistant,
            text: "calling tool".into(),
            calls: vec![ToolCall {
                id: "t1".into(),
                tool: "uppercase".into(),
                arguments: r#""hello""#.into(),
            }],
            call_id: String::new(),
            error: None,
        },
        Message {
            role: Role::Tool,
            text: "HELLO".into(),
            calls: vec![],
            call_id: "t1".into(),
            error: None,
        },
        message(Role::Assistant, "tool completed"),
        message(Role::User, "current question"),
    ];
    request
}
fn tool_checkpoint(events: &[Event]) -> CompactionCheckpoint {
    let mut cp = checkpoint(events);
    cp.retained = vec![span_for_events(&events[2..]).unwrap()];
    cp
}

#[test]
fn compact_old_text_but_retain_complete_authoritative_tool_turn() {
    let journal = tool_source();
    let cp = tool_checkpoint(&journal);
    let view = compacted_view(&cp, &journal, 4096).unwrap();
    let authoritative = tool_request();
    let result = assemble_verified_basic_request(&authoritative, &journal, &cp, &view).unwrap();
    assert_eq!(result.history, authoritative.history[2..]);
    assert_eq!(result.history[1].calls[0].arguments, r#""hello""#);
    assert_eq!(result.history[2].text, "HELLO");
    assert_eq!(result.context[1].text, "Old turn anchored in journal");
    assert_eq!(authoritative.history.len(), 7);
}

#[test]
fn tool_history_rejects_modified_results_missing_source_and_tool_elision() {
    let journal = tool_source();
    let cp = tool_checkpoint(&journal);
    let view = compacted_view(&cp, &journal, 4096).unwrap();
    let mut forged = tool_request();
    forged.history[4].text = "FORGED".into();
    assert_eq!(
        assemble_verified_basic_request(&forged, &journal, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );

    let mut forged = tool_request();
    forged.history[3].calls[0].id = "other".into();
    assert_eq!(
        assemble_verified_basic_request(&forged, &journal, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );

    // The legacy journal records IDs, tool names and results, not arguments.
    // Even an otherwise matched tool turn must never enter a summary.
    let mut elided = tool_checkpoint(&journal);
    elided.summarized = vec![span_for_events(&journal[..8]).unwrap()];
    elided.retained = vec![span_for_events(&journal[8..]).unwrap()];
    let elided_view = compacted_view(&elided, &journal, 4096).unwrap();
    assert_eq!(
        assemble_verified_basic_request(&tool_request(), &journal, &elided, &elided_view),
        Err(TranscriptProblem::UnsupportedToolHistory),
    );
}

#[test]
fn in_progress_tool_result_may_continue_same_model_turn_without_answer() {
    let mut journal = tool_source();
    journal.truncate(7);
    // This is the next ModelStarted after the full tool call/result pair.
    let mut cp = tool_checkpoint(&journal);
    cp.retained = vec![span_for_events(&journal[2..]).unwrap()];
    let view = compacted_view(&cp, &journal, 4096).unwrap();
    let mut authoritative = tool_request();
    authoritative.history.truncate(5);
    let input = assemble_verified_basic_request(&authoritative, &journal, &cp, &view).unwrap();
    assert_eq!(input.history, authoritative.history[2..]);
    assert_eq!(input.history.last().unwrap().role, Role::Tool);
    assert_eq!(input.history[1].calls[0].arguments, r#""hello""#);

    // The next ModelStarted journal event is needed to prove that the
    // submitted request follows the completed tool response.
    let mut incomplete = journal.clone();
    incomplete.pop();
    let cp = tool_checkpoint(&incomplete);
    let view = compacted_view(&cp, &incomplete, 4096).unwrap();
    assert_eq!(
        assemble_verified_basic_request(&authoritative, &incomplete, &cp, &view),
        Err(TranscriptProblem::MissingCurrentPrompt),
    );
}

#[test]
fn tool_failure_status_must_match_authoritative_session_message() {
    let mut journal = tool_source();
    journal[5].text = "tool failed".into();
    journal[5].data =
        phenix_basic_contracts::encode(&phenix_basic_contracts::ToolCompletion { failed: true })
            .unwrap();
    let cp = tool_checkpoint(&journal);
    let view = compacted_view(&cp, &journal, 4096).unwrap();
    let mut request = tool_request();
    request.history[4].text.clear();
    request.history[4].error = Some("tool failed".into());
    assert!(assemble_verified_basic_request(&request, &journal, &cp, &view).is_ok());
    request.history[4].error = None;
    assert_eq!(
        assemble_verified_basic_request(&request, &journal, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );
}

#[test]
fn all_events_of_tool_bearing_run_must_remain_visible() {
    let journal = tool_source();
    let mut cp = tool_checkpoint(&journal);
    cp.retained = vec![
        span_for_events(&journal[2..6]).unwrap(),
        span_for_events(&journal[7..]).unwrap(),
    ];
    cp.excluded = vec![span_for_events(&journal[6..7]).unwrap()];
    // ToolStarted and ToolFinished are both retained. The otherwise-valid
    // projection excludes their following ModelStarted in the same run.
    // Retained tool-message lineage must reject this partial journal.
    let view = compacted_view(&cp, &journal, 4096).unwrap();
    assert_eq!(
        assemble_verified_basic_request(&tool_request(), &journal, &cp, &view),
        Err(TranscriptProblem::UnsupportedToolHistory),
    );
}

#[test]
fn active_model_run_must_match_final_authoritative_session_run() {
    let journal = tool_source();
    let cp = tool_checkpoint(&journal);
    let view = compacted_view(&cp, &journal, 4096).unwrap();
    let mut authoritative = tool_request();
    authoritative.run = "run-999".into();
    assert_eq!(
        assemble_verified_basic_request(&authoritative, &journal, &cp, &view),
        Err(TranscriptProblem::UnalignedHistory),
    );
}
