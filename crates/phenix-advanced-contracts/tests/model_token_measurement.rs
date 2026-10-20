//! RED P2-3k: measure the complete request for the selected model.
//! The returned fingerprint binds the count to exact history, tools, skills,
//! context and limits. A count for another request or model is never reusable.
use phenix_advanced_contracts::{
    model_request_sha256, validate_model_token_measurement, ModelTokenMeasurement,
    ModelTokenProblem,
};
use phenix_basic_contracts::{ContextItem, Limits, Message, ModelRequest, Skill, ToolInfo};

fn request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message::user("Explain the result")],
        tools: vec![ToolInfo {
            id: "read".into(),
            description: "Read a file".into(),
            arguments_schema: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.into(),
            idempotent: true,
        }],
        skills: vec![Skill {
            id: "writing".into(),
            description: "Writing guidance".into(),
            instructions: "Cite the source".into(),
        }],
        context: vec![ContextItem {
            id: "source:1".into(),
            text: "Verified source context".into(),
        }],
        limits: Limits::default(),
    }
}

fn measured(request: &ModelRequest) -> ModelTokenMeasurement {
    ModelTokenMeasurement {
        model: "test-model-v1".into(),
        tokenizer_revision: "exact-fixture-bytes-v1".into(),
        request_sha256: model_request_sha256(request).unwrap(),
        input_tokens: 200,
        context_window_tokens: 250,
    }
}

#[test]
fn count_binds_to_every_part_of_submitted_request() {
    let original = request();
    let count = measured(&original);
    assert_eq!(
        validate_model_token_measurement(&original, &count, "test-model-v1", 50),
        Ok(())
    );
    let mut altered = original.clone();
    altered.history[0].text.push('!');
    assert_eq!(
        validate_model_token_measurement(&altered, &count, "test-model-v1", 50),
        Err(ModelTokenProblem::RequestChanged)
    );
    let mut altered = original.clone();
    altered.tools[0].arguments_schema.push(' ');
    assert_eq!(
        validate_model_token_measurement(&altered, &count, "test-model-v1", 50),
        Err(ModelTokenProblem::RequestChanged)
    );
    let mut altered = original.clone();
    altered.skills[0].instructions.push('!');
    assert_eq!(
        validate_model_token_measurement(&altered, &count, "test-model-v1", 50),
        Err(ModelTokenProblem::RequestChanged)
    );
    let mut altered = original.clone();
    altered.context[0].text.push('!');
    assert_eq!(
        validate_model_token_measurement(&altered, &count, "test-model-v1", 50),
        Err(ModelTokenProblem::RequestChanged)
    );
    let mut altered = original.clone();
    altered.limits.model_tokens = Some(49);
    assert_eq!(
        validate_model_token_measurement(&altered, &count, "test-model-v1", 50),
        Err(ModelTokenProblem::RequestChanged)
    );
}

#[test]
fn wrong_model_or_budget_denies_before_model_call() {
    let input = request();
    let count = measured(&input);
    assert_eq!(
        validate_model_token_measurement(&input, &count, "other-model", 1),
        Err(ModelTokenProblem::ModelChanged)
    );
    assert_eq!(
        validate_model_token_measurement(&input, &count, "test-model-v1", 51),
        Err(ModelTokenProblem::OverBudget)
    );
    assert_eq!(
        validate_model_token_measurement(&input, &count, "test-model-v1", u64::MAX),
        Err(ModelTokenProblem::OverBudget)
    );
    let mut invalid = count.clone();
    invalid.input_tokens = 0;
    assert_eq!(
        validate_model_token_measurement(&input, &invalid, "test-model-v1", 1),
        Err(ModelTokenProblem::InvalidMeasurement)
    );
    invalid = count.clone();
    invalid.tokenizer_revision.clear();
    assert_eq!(
        validate_model_token_measurement(&input, &invalid, "test-model-v1", 1),
        Err(ModelTokenProblem::InvalidMeasurement)
    );
    invalid = count;
    invalid.request_sha256 = "0".repeat(64);
    assert_eq!(
        validate_model_token_measurement(&input, &invalid, "test-model-v1", 1),
        Err(ModelTokenProblem::RequestChanged)
    );
}
