//! ACP v1 turn-codec conformance through native selected Frontend.
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

fn selection(operation: &str) -> Selection {
    Selection {
        interface: "phenix.basic.frontend@1".into(),
        operation: operation.into(),
        provider: "phenix.basic.frontend-acp".into(),
    }
}
fn call<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    op: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(op))?;
    match root.invoke(&request.to_value())? {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "ACP DEFINITIONS OUTPUT");
    let resources = BTreeSet::from(["phenix.session.s1".to_owned()]);
    let ops = ["decode", "render", "cancel"];
    let kernel = Prepared::prepare(Composition {
        format: "phenix.composition.v2".into(),
        artifacts: vec![args[1].clone()],
        definitions: vec![args[2].clone()],
        entry: selection("decode"),
        slots: vec![],
        grants: ops
            .into_iter()
            .map(|op| ScopedGrant {
                caller: "root".into(),
                interface: "phenix.basic.frontend@1".into(),
                operation: op.into(),
                resources: resources.clone(),
            })
            .collect(),
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: ops
                .into_iter()
                .map(|op| format!("phenix.basic.frontend@1/{op}"))
                .collect(),
            resources,
        },
        resource: Some("phenix.session.s1".into()),
    })
    .unwrap()
    .load()
    .unwrap();

    let message = r#"{"jsonrpc":"2.0","id":8,"method":"session/prompt","params":{"sessionId":"s1","prompt":[{"type":"text","text":"hello"}]}}"#;
    let accepted: Submission = call(
        &kernel,
        "decode",
        ClientInput {
            session: "s1".into(),
            line: message.into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted.action, "phenix.basic.submit");
    let prompt: PromptSubmitted = phenix_basic_contracts::decode(&accepted.payload).unwrap();
    assert_eq!(prompt.text, "hello");
    assert_eq!(prompt.session, "s1");
    let cancel: Submission = call(
        &kernel,
        "decode",
        ClientInput {
            session: "s1".into(),
            line: r#"{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"s1"}}"#
                .into(),
        },
    )
    .unwrap()
    .unwrap();
    assert!(cancel.cancel);
    assert_eq!(call::<Submission>(
        &kernel, "decode", ClientInput {
            session: "s1".into(),
            line: r#"{"jsonrpc":"2.0","id":8,"method":"session/prompt","params":{"sessionId":"s2","prompt":[{"type":"text","text":"forged"}]}}"#.into(),
        },
    ).unwrap().unwrap_err().code, ErrorCode::Denied);
    assert_eq!(call::<Submission>(
        &kernel, "decode", ClientInput {
            session: "s1".into(),
            line: r#"{"jsonrpc":"2.0","id":8,"method":"session/prompt","params":{"sessionId":"s1","prompt":[{"type":"image","data":"abc"}]}}"#.into(),
        },
    ).unwrap().unwrap_err().code, ErrorCode::InvalidInput);
    let output: Vec<String> = call(
        &kernel,
        "render",
        RenderRequest {
            session: "s1".into(),
            events: vec![Event {
                sequence: 1,
                session: "s1".into(),
                run: "run-1".into(),
                kind: EventKind::TextDelta,
                call_id: String::new(),
                text: "mock:hello".into(),
                data: Blob(vec![]),
            }],
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(output.len(), 1);
    let notification: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(notification["method"], "session/update");
    assert_eq!(notification["params"]["sessionId"], "s1");
    assert_eq!(
        notification["params"]["update"]["sessionUpdate"],
        "agent_message_chunk"
    );
    assert_eq!(
        notification["params"]["update"]["content"]["text"],
        "mock:hello"
    );
    assert_eq!(
        call::<()>(
            &kernel,
            "cancel",
            Scope {
                session: "s1".into()
            }
        )
        .unwrap()
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );

    let output_dir = Path::new(&args[3]);
    fs::create_dir_all(output_dir).unwrap();
    fs::write(
        output_dir.join("acp-codec.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.f2.acp-codec.v1",
            "cases": ["acp-v1-text-prompt", "cancel-notification",
                "session-scope-denial", "unsupported-content-rejection",
                "session-update-notification", "cancel-no-fake-success"],
            "connection_lifecycle_or_streaming_verified": false
        }))
        .unwrap(),
    )
    .unwrap();
}
