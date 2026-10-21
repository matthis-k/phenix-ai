//! P2-3u RED: checkpoint pressure derives from the selected Model's exact
//! full-request measurement. It does not execute, summarize, or store effects.
use phenix_advanced_contracts::{model_request_sha256, ModelTokenMeasurement, ModelTokenProblem};
use phenix_basic_contracts::{Limits, Message, ModelRequest};
use phenix_p2_compaction_replay::{checkpoint_pressure, CheckpointPressure, PressureProblem};

fn request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "run-1".into(),
        history: vec![Message::user("What happened earlier?")],
        tools: vec![],
        skills: vec![],
        context: vec![],
        limits: Limits {
            model_tokens: Some(500),
            ..Limits::default()
        },
    }
}
fn measurement(request: &ModelRequest, input: u64, window: u64) -> ModelTokenMeasurement {
    ModelTokenMeasurement {
        model: "fixture:selected-model".into(),
        tokenizer_revision: "fixture:tokenizer-v1".into(),
        request_sha256: model_request_sha256(request).unwrap(),
        input_tokens: input,
        context_window_tokens: window,
    }
}

#[test]
fn measured_headroom_decides_checkpoint_before_model_admission() {
    let req = request();
    assert_eq!(
        checkpoint_pressure(
            &req,
            &measurement(&req, 1_000, 10_000),
            "fixture:selected-model",
            80,
        ),
        Ok(CheckpointPressure::EnoughHeadroom)
    );
    assert_eq!(
        checkpoint_pressure(
            &req,
            &measurement(&req, 7_500, 10_000),
            "fixture:selected-model",
            80,
        ),
        Ok(CheckpointPressure::AtThreshold)
    );
    assert_eq!(
        checkpoint_pressure(
            &req,
            &measurement(&req, 9_501, 10_000),
            "fixture:selected-model",
            80,
        ),
        Ok(CheckpointPressure::OverContextWindow)
    );
}

#[test]
fn measurement_must_identify_the_exact_request_and_selected_model() {
    let req = request();
    let mut meter = measurement(&req, 7_500, 10_000);
    meter.request_sha256 = "0".repeat(64);
    assert_eq!(
        checkpoint_pressure(&req, &meter, "fixture:selected-model", 80),
        Err(PressureProblem::Measurement(
            ModelTokenProblem::RequestChanged
        )),
    );
    let meter = measurement(&req, 9_900, 10_000);
    assert_eq!(
        checkpoint_pressure(&req, &meter, "different-provider", 80),
        Err(PressureProblem::Measurement(
            ModelTokenProblem::ModelChanged
        )),
    );
}

#[test]
fn selected_output_reserve_and_policy_threshold_are_mandatory() {
    let req = request();
    let meter = measurement(&req, 7_500, 10_000);
    let mut no_limit = req.clone();
    no_limit.limits.model_tokens = None;
    let meter_without_limit = measurement(&no_limit, 7_500, 10_000);
    assert_eq!(
        checkpoint_pressure(
            &no_limit,
            &meter_without_limit,
            "fixture:selected-model",
            80
        ),
        Err(PressureProblem::Measurement(ModelTokenProblem::OverBudget)),
    );
    // Even deleting all input cannot make an output reserve as large as the
    // full context window fit. Do not trigger a futile retry loop.
    let impossible = measurement(&req, 1, 500);
    assert_eq!(
        checkpoint_pressure(&req, &impossible, "fixture:selected-model", 80),
        Err(PressureProblem::Measurement(ModelTokenProblem::OverBudget)),
    );
    for bad in [0, 101, u64::MAX] {
        assert_eq!(
            checkpoint_pressure(&req, &meter, "fixture:selected-model", bad),
            Err(PressureProblem::InvalidThreshold),
        );
    }
}

#[test]
fn threshold_arithmetic_does_not_overflow_for_large_valid_measurements() {
    let req = request();
    let meter = measurement(&req, u64::MAX - 1000, u64::MAX);
    assert_eq!(
        checkpoint_pressure(&req, &meter, "fixture:selected-model", 80),
        Ok(CheckpointPressure::AtThreshold)
    );
}
