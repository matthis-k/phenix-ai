//! Real kernel admission check for the selected Model and its preflight meter.
//! Synthetic token semantics belong to the selected fixture Model only.
use phenix_advanced_contracts::{
    validate_model_token_measurement, ModelTokenMeasurement, ModelTokenProblem,
};
use phenix_basic_contracts::{
    ContextItem, Limits, Message, ModelReply, ModelRequest, Skill, ToolInfo,
};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Call, Outcome, Poll,
};
use std::{collections::BTreeSet, fs, path::Path};

const PLUGIN: &str = "phenix.model.meter-fixture";
const MODEL_ID: &str = "phenix.fixture.portable-byte-model@1";
const RESOURCE: &str = "phenix.session.s1";
const MODEL: &str = "phenix.basic.model@1";
const TOKENS: &str = "phenix.advanced.model-tokens@1";

fn grant(interface: &str, operation: &str) -> ScopedGrant {
    ScopedGrant {
        caller: "root".into(),
        interface: interface.into(),
        operation: operation.into(),
        resources: BTreeSet::from([RESOURCE.into()]),
    }
}

fn invoke(
    artifacts: &[String],
    definitions: &[String],
    interface: &str,
    operation: &str,
    input: Value,
) -> Option<Outcome> {
    let grants = vec![grant(MODEL, "complete"), grant(TOKENS, "measure")];
    let composition = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: artifacts.to_vec(),
        definitions: definitions.to_vec(),
        entry: Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: PLUGIN.into(),
        },
        slots: vec![],
        authority: Authority {
            operations: grants.iter().map(ScopedGrant::key).collect(),
            resources: BTreeSet::from([RESOURCE.into()]),
        },
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: Some(RESOURCE.into()),
    };
    let kernel = Prepared::prepare(composition).ok()?.load().ok()?;
    let root = kernel.admit_default_root().ok()?;
    // Model is a pending-capable contract even when the fixture resolves
    // immediately. Root::invoke accepts only synchronous implementations.
    // Exercise the same authenticated kernel lifecycle as every caller.
    match root.begin(&input).ok()? {
        Call::Ready(outcome) => Some(outcome),
        Call::Pending(ticket) => {
            for _ in 0..512 {
                match root.poll(&ticket).ok()? {
                    Poll::Pending | Poll::Draining | Poll::Cancelled { quiescent: false } => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Poll::Ready(result) => {
                        root.release(&ticket).ok()?;
                        return result.ok();
                    }
                    Poll::Cancelled { quiescent: true } => {
                        root.release(&ticket).ok()?;
                        return None;
                    }
                }
            }
            root.cancel(&ticket).ok()?;
            None
        }
    }
}

fn request() -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message::user("hello")],
        tools: vec![ToolInfo {
            id: "read".into(),
            description: "Read a file".into(),
            arguments_schema: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.into(),
            idempotent: true,
        }],
        skills: vec![Skill {
            id: "writing".into(),
            description: "Writing".into(),
            instructions: "Cite the source".into(),
        }],
        context: vec![ContextItem {
            id: "source".into(),
            text: "Source evidence".into(),
        }],
        limits: Limits::default(),
    }
}

fn accepted<T: Portable>(
    artifacts: &[String],
    definitions: &[String],
    interface: &str,
    operation: &str,
    input: Value,
) -> T {
    let outcome = invoke(artifacts, definitions, interface, operation, input).unwrap();
    let Outcome::Success(value) = outcome else {
        panic!("expected {interface}/{operation} success, got {outcome:?}");
    };
    T::from_value(value).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 5, "FIXTURE BASIC-DEFS ADVANCED-DEFS OUTDIR");
    let artifacts = vec![args[1].clone()];
    let definitions = args[2..=3].to_vec();
    let input = request();
    let count: ModelTokenMeasurement = accepted(
        &artifacts,
        &definitions,
        TOKENS,
        "measure",
        input.to_value(),
    );
    let output: ModelReply = accepted(
        &artifacts,
        &definitions,
        MODEL,
        "complete",
        input.to_value(),
    );
    let mut changed_tool = input.clone();
    changed_tool.tools[0].arguments_schema.push_str("more");
    let changed_tool_count: ModelTokenMeasurement = accepted(
        &artifacts,
        &definitions,
        TOKENS,
        "measure",
        changed_tool.to_value(),
    );
    let mut changed_context = input.clone();
    changed_context.context[0].text.push_str("more");
    let changed_context_count: ModelTokenMeasurement = accepted(
        &artifacts,
        &definitions,
        TOKENS,
        "measure",
        changed_context.to_value(),
    );
    let mut wrong_session = input.clone();
    wrong_session.session = "s2".into();
    let denied = !matches!(
        invoke(
            &artifacts,
            &definitions,
            TOKENS,
            "measure",
            wrong_session.to_value()
        ),
        Some(Outcome::Success(_))
    );
    let serialized = input.to_value().encode().unwrap();
    let report = serde_json::json!({
        "model": count.model,
        "tokenizer": count.tokenizer_revision,
        "response": output.text,
        "model_and_meter_share_provider": true,
        "full_request_measurement": count.input_tokens == serialized.len() as u64,
        "input_tokens_match_model_usage": output.usage.input_tokens == count.input_tokens,
        "changed_tool_schema_counted": changed_tool_count.input_tokens > count.input_tokens,
        "changed_context_counted": changed_context_count.input_tokens > count.input_tokens,
        "request_change_denied": validate_model_token_measurement(
            &changed_tool, &count, MODEL_ID, 512,
        ) == Err(ModelTokenProblem::RequestChanged),
        "cross_session_denied": denied,
    });
    let out = Path::new(&args[4]);
    fs::create_dir_all(out).unwrap();
    fs::write(
        out.join("model-meter-product.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
