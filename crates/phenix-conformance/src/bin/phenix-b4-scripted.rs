//! Native black-box gate for the independently packaged scripted model.
use phenix_basic_contracts::{
    BasicError, ErrorCode, Limits, Message, ModelReply, ModelRequest, ToolInfo,
};
use phenix_contract::{Authority, Code, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn request(text: &str) -> ModelRequest {
    ModelRequest {
        session: "s1".into(),
        run: "r1".into(),
        history: vec![Message::user(text)],
        tools: ["uppercase", "length"]
            .map(|id| ToolInfo {
                id: id.into(),
                description: String::new(),
                arguments_schema: String::new(),
                idempotent: true,
            })
            .to_vec(),
        skills: vec![],
        context: vec![],
        limits: Limits::default(),
    }
}

fn invoke(
    kernel: &phenix_kernel::Kernel,
    request: &ModelRequest,
) -> Result<Result<ModelReply, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_default_root()?;
    match root.invoke(&request.to_value())? {
        Outcome::Success(value) => Ok(Ok(ModelReply::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "SCRIPTED_MODEL BASIC_DEFINITIONS OUTPUT");
    let output = Path::new(&args[3]);
    fs::create_dir_all(output).unwrap();
    let operation = "phenix.basic.model@1/complete";
    let resource = "phenix.session.s1";
    let resource_set = BTreeSet::from([resource.to_owned()]);
    let policy = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: vec![args[1].clone()],
        definitions: vec![args[2].clone()],
        entry: Selection {
            interface: "phenix.basic.model@1".into(),
            operation: "complete".into(),
            provider: "phenix.model.scripted".into(),
        },
        slots: vec![],
        grants: vec![ScopedGrant {
            caller: "root".into(),
            interface: "phenix.basic.model@1".into(),
            operation: "complete".into(),
            resources: resource_set.clone(),
        }],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: BTreeSet::from([operation.to_owned()]),
            resources: resource_set,
        },
        resource: Some(resource.into()),
    };
    let kernel = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let direct = invoke(&kernel, &request("hello")).unwrap().unwrap();
    assert_eq!(direct.text, "mock:hello");
    assert!(direct.usage.input_tokens > 0 && direct.usage.output_tokens > 0);

    let mut scripted = request("tools:hello");
    let first = invoke(&kernel, &scripted).unwrap().unwrap();
    assert_eq!(first.calls.len(), 2);
    scripted
        .history
        .push(Message::assistant(String::new(), first.calls));
    scripted.history.push(Message::tool(
        "call-1",
        phenix_basic_contracts::ToolOutcome::Succeeded {
            text: "HELLO".into(),
        },
    ));
    scripted.history.push(Message::tool(
        "call-2",
        phenix_basic_contracts::ToolOutcome::Succeeded { text: "5".into() },
    ));
    assert_eq!(
        invoke(&kernel, &scripted).unwrap().unwrap().text,
        "mock:call-1:HELLO|call-2:5"
    );

    scripted.history.pop();
    assert_eq!(
        invoke(&kernel, &scripted).unwrap().unwrap_err().code,
        ErrorCode::InvalidReply
    );
    let mut cross_session = request("hello");
    cross_session.session = "s2".into();
    assert_eq!(
        invoke(&kernel, &cross_session).unwrap().unwrap_err().code,
        ErrorCode::Denied
    );
    assert_eq!(
        invoke(&kernel, &request("model-error"))
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::ModelFailed
    );
    let mut limited = request("hello");
    limited.limits.model_tokens = Some(0);
    assert_eq!(
        invoke(&kernel, &limited).unwrap().unwrap_err().code,
        ErrorCode::TurnLimit
    );
    drop(kernel);

    let mut denied = policy;
    denied.grants.clear();
    // Preparation rejects an ungranted default entry before loading a native
    // image. No constructor, admission or provider effect is possible.
    assert_eq!(Prepared::prepare(denied).err().unwrap().code, Code::Denied);
    let result = serde_json::json!({
        "format": "phenix.b4.scripted.v1",
        "provider": "phenix.model.scripted",
        "cases": [
            "direct", "multiple-tool-calls", "transcript-continuation",
            "missing-result", "cross-session-denial", "model-domain-error",
            "caller-token-limit", "missing-grant-zero-effects"
        ],
        "usage_provided": true,
        "external_sdk_only": true
    });
    fs::write(
        output.join("scripted-model.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("B4 scripted native model gate passed");
}
