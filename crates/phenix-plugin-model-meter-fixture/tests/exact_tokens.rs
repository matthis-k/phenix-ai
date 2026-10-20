//! RED P2-3k: a selected verification Model counts the complete request
//! with its own exact deterministic tokenizer (one Portable byte per token).
use phenix_advanced_contracts::{
    model_request_sha256, validate_model_token_measurement, ModelTokenProblem,
};
use phenix_basic_contracts::{ContextItem, Limits, Message, ModelRequest, Skill, ToolInfo};
use phenix_plugin_model_meter_fixture::{complete, measure, MODEL_ID};

fn request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message::user("hello")],
        tools: vec![ToolInfo {
            id: "read".into(),
            description: "Read file".into(),
            arguments_schema: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.into(),
            idempotent: true,
        }],
        skills: vec![Skill {
            id: "one".into(),
            description: "Skill".into(),
            instructions: "Keep evidence".into(),
        }],
        context: vec![ContextItem {
            id: "source".into(),
            text: "source bytes".into(),
        }],
        limits: Limits::default(),
    }
}
#[test]
fn exact_fixture_meter_and_model_share_full_request_semantics() {
    let input = request();
    let count = measure(&input).unwrap();
    assert_eq!(count.model, MODEL_ID);
    assert_eq!(count.request_sha256, model_request_sha256(&input).unwrap());
    assert_eq!(
        validate_model_token_measurement(&input, &count, MODEL_ID, 512),
        Ok(())
    );
    let output = complete(&input).unwrap();
    assert_eq!(output.text, "fixture:hello");
    assert_eq!(output.usage.input_tokens, count.input_tokens);
    assert_eq!(output.calls.len(), 0);
}

#[test]
fn exact_fixture_counts_tools_skills_context_and_previous_turns() {
    let base = request();
    let baseline = measure(&base).unwrap();
    let mut changed = base.clone();
    changed.tools[0].arguments_schema.push_str("more");
    let tools = measure(&changed).unwrap();
    assert!(tools.input_tokens > baseline.input_tokens);
    assert_ne!(tools.request_sha256, baseline.request_sha256);
    assert_eq!(
        validate_model_token_measurement(&changed, &baseline, MODEL_ID, 512),
        Err(ModelTokenProblem::RequestChanged),
    );
    changed = base.clone();
    changed.skills[0].instructions.push_str("more");
    assert!(measure(&changed).unwrap().input_tokens > baseline.input_tokens);
    changed = base.clone();
    changed.context[0].text.push_str("more");
    assert!(measure(&changed).unwrap().input_tokens > baseline.input_tokens);
    changed = base;
    changed.history.insert(
        0,
        Message::assistant("extra historical message", vec![]),
    );
    assert!(measure(&changed).unwrap().input_tokens > baseline.input_tokens);
}
