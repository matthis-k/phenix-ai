//! Two independently packaged frontend codecs with the same public types.
use phenix_basic_contracts::{
    BasicError, ClientInput, ErrorCode, Event, EventKind, PromptSubmitted, RenderRequest, Scope,
    Submission,
};
use phenix_contract::{Authority, Blob, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selection(operation: &str, provider: &str) -> Selection {
    Selection {
        interface: "phenix.basic.frontend@1".into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn invoke<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    provider: &str,
    op: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(op, provider))?;
    match root.invoke(&request.to_value())? {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}
fn verify(path: String, defs: String, provider: &str, jsonl: bool) {
    let resource = "phenix.session.s1";
    let ops = ["decode", "render", "cancel"];
    let resources = BTreeSet::from([resource.to_owned()]);
    let grants = ops
        .into_iter()
        .map(|op| ScopedGrant {
            caller: "root".into(),
            interface: "phenix.basic.frontend@1".into(),
            operation: op.into(),
            resources: resources.clone(),
        })
        .collect();
    let composition = Composition {
        format: "phenix.composition.v2".into(),
        artifacts: vec![path],
        definitions: vec![defs],
        entry: selection("decode", provider),
        slots: vec![],
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: ops
                .into_iter()
                .map(|op| format!("phenix.basic.frontend@1/{op}"))
                .collect(),
            resources,
        },
        resource: Some(resource.into()),
    };
    let kernel = Prepared::prepare(composition).unwrap().load().unwrap();
    let line = if jsonl {
        r#"{"type":"prompt","session":"s1","text":"hello","limits":{"model_turns":2}}"#
    } else {
        "hello"
    };
    let accepted: Submission = invoke(
        &kernel,
        provider,
        "decode",
        ClientInput {
            session: "s1".into(),
            line: line.into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted.action, "phenix.basic.submit");
    assert!(!accepted.cancel);
    let prompt: PromptSubmitted = phenix_basic_contracts::decode(&accepted.payload).unwrap();
    assert_eq!(prompt.session, "s1");
    assert_eq!(prompt.text, "hello");
    assert_eq!(
        prompt.limits.model_turns,
        if jsonl { Some(2) } else { None }
    );
    let canceled: Submission = invoke(
        &kernel,
        provider,
        "decode",
        ClientInput {
            session: "s1".into(),
            line: if jsonl {
                r#"{"type":"cancel","session":"s1"}"#.into()
            } else {
                "/cancel".into()
            },
        },
    )
    .unwrap()
    .unwrap();
    assert!(canceled.cancel);
    let output: Vec<String> = invoke(
        &kernel,
        provider,
        "render",
        RenderRequest {
            session: "s1".into(),
            events: vec![Event {
                sequence: 1,
                session: "s1".into(),
                run: "run-1".into(),
                kind: EventKind::Answer,
                call_id: String::new(),
                text: "mock:hello".into(),
                data: Blob(vec![]),
            }],
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(output.len(), 1);
    assert!(output[0].contains("mock:hello"));
    if jsonl {
        assert_eq!(
            invoke::<Submission>(
                &kernel,
                provider,
                "decode",
                ClientInput {
                    session: "s1".into(),
                    line: r#"{"type":"prompt","session":"s2","text":"forged"}"#.into(),
                }
            )
            .unwrap()
            .unwrap_err()
            .code,
            ErrorCode::Denied
        );
    }
    // The codec cannot cancel an active root by itself. It emits a typed
    // cancellation request for the controlling host, never a fake success.
    assert_eq!(
        invoke::<()>(
            &kernel,
            provider,
            "cancel",
            Scope {
                session: "s1".into()
            },
        )
        .unwrap()
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        invoke::<Submission>(
            &kernel,
            provider,
            "decode",
            ClientInput {
                session: "s2".into(),
                line: String::from("unauthorized")
            }
        )
        .unwrap()
        .unwrap_err()
        .code,
        ErrorCode::Denied
    );
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "TERMINAL JSONL DEFINITIONS OUTPUT");
    verify(
        args[1].clone(),
        args[3].clone(),
        "phenix.basic.frontend-terminal",
        false,
    );
    verify(
        args[2].clone(),
        args[3].clone(),
        "phenix.basic.frontend-jsonl",
        true,
    );
    let output = Path::new(&args[4]);
    fs::create_dir_all(output).unwrap();
    fs::write(
        output.join("frontends.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.f1.frontend.v1",
            "cases": ["terminal-and-jsonl-codecs", "common-submission",
                "caller-limits", "cross-session-denial", "typed-cancellation",
                "rendering", "cancel-does-not-fake-success"],
            "live_host_or_acp_integration": false
        }))
        .unwrap(),
    )
    .unwrap();
}
