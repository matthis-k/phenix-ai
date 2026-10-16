//! RED P2-3e: model-token selection counts the *assembled request* through
//! its selected Model adapter, never raw event bytes or guessed tokens.
use phenix_advanced_contracts::CompactionCheckpoint;
use phenix_basic_contracts::{Event, EventKind, Message, ModelRequest, Role, Limits};
use phenix_p2_compaction_replay::{
    select_model_view, span_for_events, ModelSelectionError, ReplayProblem, ViewProblem,
};
use phenix_sdk::Blob;

fn journal() -> Vec<Event> {
    (1..=4).map(|sequence| Event {
        sequence,
        session: "s1".into(),
        run: format!("run-{sequence}"),
        kind: if sequence % 2 == 1 { EventKind::PromptAccepted } else { EventKind::Answer },
        call_id: String::new(),
        text: format!("event-{sequence}"),
        data: Blob(vec![]),
    }).collect()
}

fn checkpoint(source: &[Event], id: &str, revision: u64, parent: Option<&str>, summary: &str) -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: id.into(),
        session: "s1".into(),
        revision,
        parent_checkpoint: parent.map(str::to_owned),
        input: span_for_events(source).unwrap(),
        summarized: vec![span_for_events(&source[..2]).unwrap()],
        retained: vec![span_for_events(&source[2..]).unwrap()],
        excluded: vec![],
        summary: summary.into(),
        policy: "selected-model".into(),
        policy_version: 1,
        model: Some("scripted".into()),
        hydration_sources: vec![],
    }
}

fn request(summary: &str) -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "current-run".into(),
        // The request builder retains authoritative session messages. Journal
        // Event is intentionally not reconstructed into lossy model history.
        history: vec![Message {
            role: Role::User,
            text: "authoritative session input".into(),
            calls: vec![],
            call_id: String::new(),
            error: None,
        }],
        tools: vec![],
        skills: vec![],
        context: vec![phenix_basic_contracts::ContextItem {
            id: "verified-compaction".into(),
            text: summary.into(),
        }],
        limits: Limits::default(),
    }
}

#[test]
fn newest_fitting_checkpoint_is_selected_with_full_request_token_measurement() {
    let events = journal();
    let old = checkpoint(&events, "old", 1, None, "short source-cited summary");
    let newest = checkpoint(&events, "new", 2, Some("old"), "longer source-cited summary");
    let mut considered = Vec::new();
    let selected = select_model_view(
        &[old, newest], &events, 4096, 100,
        |view| {
            considered.push(view.checkpoint_id.clone());
            let summary = view.parts.iter().find_map(|p| match p {
                phenix_p2_compaction_replay::ContextPart::Summary { text, .. } => Some(text.as_str()),
                _ => None,
            }).unwrap();
            // Represents an exact count by the selected Model adapter over
            // every request field (history, tool schemas, context, overhead).
            let tokens = if view.checkpoint_id == "new" { 120 } else { 89 };
            Ok::<_, &'static str>((request(summary), tokens))
        },
    ).unwrap();
    assert_eq!(considered, vec!["new", "old"]);
    assert_eq!(selected.view.checkpoint_id, "old");
    assert_eq!(selected.model_input_tokens, 89);
    assert_eq!(selected.request.context[0].text, "short source-cited summary");
}

#[test]
fn stale_source_must_not_fallback_to_older_checkpoint_or_issue_a_model_request() {
    let events = journal();
    let old = checkpoint(&events, "old", 1, None, "old summary");
    let mut newest = checkpoint(&events, "new", 2, Some("old"), "new summary");
    newest.input.digest_sha256 = "f".repeat(64);
    let mut called = false;
    let result = select_model_view(
        &[old, newest], &events, 4096, 100,
        |_| { called = true; Ok::<_, &'static str>((request("ignored"), 1)) },
    );
    assert_eq!(result.err(), Some(ModelSelectionError::Projection(
        ViewProblem::Replay(ReplayProblem::DigestChanged)
    )));
    assert!(!called);
}

#[test]
fn model_counter_failure_and_no_fit_fail_closed() {
    let events = journal();
    let cp = checkpoint(&events, "cp1", 1, None, "summary");
    let broken: Result<_, ModelSelectionError<&'static str>> = select_model_view(
        std::slice::from_ref(&cp), &events, 4096, 100,
        |_| Err("token counter unavailable"),
    );
    assert_eq!(broken.err(), Some(ModelSelectionError::Model("token counter unavailable")));
    let expensive = select_model_view(
        &[cp], &events, 4096, 10,
        |_| Ok::<_, &'static str>((request("summary"), 11)),
    );
    assert_eq!(expensive.err(), Some(ModelSelectionError::NoFittingView));
}

#[test]
fn zero_budget_and_bytes_cap_cannot_be_treated_as_token_estimates() {
    let events = journal();
    let cp = checkpoint(&events, "cp1", 1, None, "summary");
    let zero = select_model_view(&[cp.clone()], &events, 4096, 0, |_| {
        Ok::<_, &'static str>((request("summary"), 1))
    });
    assert_eq!(zero.err(), Some(ModelSelectionError::InvalidBudget));
    let max_bytes = select_model_view(&[cp], &events, 1, 100, |_| {
        Ok::<_, &'static str>((request("summary"), 1))
    });
    assert_eq!(max_bytes.err(), Some(ModelSelectionError::NoFittingView));
}
