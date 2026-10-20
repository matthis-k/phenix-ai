//! P2 graph-extraction prerequisite: preserve the exact authoritative
//! Basic-to-Model request value without calling a model or a second loop.
use phenix_basic_contracts::{ContextItem, Limits, Message, Skill, ToolCall, ToolInfo};
use phenix_plugin_basic_agent::project_model_request;

#[test]
fn projection_retains_entire_authoritative_pending_turn_without_mutating_inputs() {
    let history = vec![
        Message::user("read the file"),
        Message::assistant(
            "reading",
            vec![ToolCall {
                id: "c1".into(),
                tool: "read".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            }],
        ),
        Message::tool(
            "c1",
            phenix_basic_contracts::ToolOutcome::Succeeded {
                text: "fn main() {}".into(),
            },
        ),
        Message::user("now fix the code"),
    ];
    let tools = vec![ToolInfo {
        id: "read".into(),
        description: "Read a file".into(),
        arguments_schema: r#"{"type":"object"}"#.into(),
        idempotent: true,
    }];
    let skills = vec![Skill {
        id: "code".into(),
        description: "Code review".into(),
        instructions: "Preserve provenance".into(),
    }];
    let context = vec![ContextItem {
        id: "project".into(),
        text: "No speculative edits".into(),
    }];
    let limits = Limits {
        model_tokens: Some(1024),
        model_turns: Some(3),
        tool_calls: Some(5),
        http_timeout_ms: Some(30_000),
    };
    let request =
        project_model_request("s1", "run-4", &history, &tools, &skills, &context, &limits);

    assert_eq!(request.session, "s1");
    assert_eq!(request.run, "run-4");
    assert_eq!(request.history, history);
    assert_eq!(request.tools, tools);
    assert_eq!(request.skills, skills);
    assert_eq!(request.context, context);
    assert_eq!(request.limits, limits);
    // A plugin may propose a derived request, but the authoritative Basic
    // session and pending tool provenance remain unchanged.
    let mut projection = request.clone();
    projection.history[0].text.push_str(" (modified)");
    projection.context.clear();
    projection.tools.clear();
    assert_eq!(history[0].text, "read the file");
    assert_eq!(context.len(), 1);
    assert_eq!(tools.len(), 1);
}

#[test]
fn projection_is_not_a_scheduler_or_agent_run() {
    let history = vec![Message::user("hello")];
    let limits = Limits::default();
    let first = project_model_request("s2", "run-1", &history, &[], &[], &[], &limits);
    let second = project_model_request("s2", "run-1", &history, &[], &[], &[], &limits);
    assert_eq!(first, second);
    assert_eq!(first.history, history);
    assert_eq!(first.limits, limits);
    assert!(first.tools.is_empty() && first.skills.is_empty() && first.context.is_empty());
}
