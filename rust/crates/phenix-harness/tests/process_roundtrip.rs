use phenix_core::{ContextResourceId, PhenixValue, Project, SessionId, ValueError};
use phenix_plugin_catalog::{
    ContextCommand, ContextResourceKind, ContextResponse, ContextScope, SessionCommand,
    SessionRecord, SessionResponse,
};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn run_harness(state: &Path, requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phenix-harness"))
        .env("PHENIX_STATE_DB", state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("supported Harness binary must start");
    {
        let mut stdin = child.stdin.take().expect("Harness stdin must be piped");
        for request in requests {
            serde_json::to_writer(&mut stdin, request).unwrap();
            stdin.write_all(b"\n").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Harness process failed: {output:?}"
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn structural_input<T>(value: &T) -> Value
where
    for<'value> PhenixValue: From<&'value T>,
{
    serde_json::to_value(PhenixValue::from(value)).unwrap()
}

fn structural_output<T>(response: &Value) -> T
where
    for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let value: PhenixValue = serde_json::from_value(response.clone()).unwrap();
    T::try_from(Project(&value)).unwrap()
}

#[test]
fn process_roundtrip_routes_and_restores_plugin_owned_state() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let state = std::env::temp_dir().join(format!(
        "phenix-harness-process-roundtrip-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let _ = fs::remove_file(&state);

    let session_id = SessionId::parse("process-session").unwrap();
    let context_id = ContextResourceId::parse("process:context").unwrap();
    let first = run_harness(
        &state,
        &[
            serde_json::json!({
                "id": 1,
                "service": "phenix.sessions@1",
                "input": structural_input(&SessionCommand::Create { session: phenix_plugin_catalog::SessionRecord::new(session_id.clone()) })
            }),
            serde_json::json!({
                "id": 2,
                "service": "phenix.context@1",
                "input": structural_input(&ContextCommand::Register {
                    resource_id: context_id.clone(),
                    kind: ContextResourceKind::External,
                    source: "process-roundtrip".into(),
                    scope: ContextScope::Workspace,
                    content: b"process".to_vec().into(),
                })
            }),
        ],
    );
    assert_eq!(first.len(), 2);
    assert_eq!(first[0]["status"], "ok");
    assert_eq!(
        structural_output::<SessionResponse>(&first[0]["output"]),
        SessionResponse::Created {
            session: SessionRecord::new(session_id.clone()),
        }
    );
    assert_eq!(first[1]["status"], "ok");
    let ContextResponse::Registered { resource } = structural_output(&first[1]["output"]) else {
        panic!("context register returned the wrong response")
    };
    assert_eq!(resource.descriptor.resource_id, context_id);

    let second = run_harness(
        &state,
        &[
            serde_json::json!({
                "id": 4,
                "service": "phenix.sessions@1",
                "input": structural_input(&SessionCommand::Get { id: session_id.clone() })
            }),
            serde_json::json!({
                "id": 5,
                "service": "phenix.context@1",
                "input": structural_input(&ContextCommand::List)
            }),
        ],
    );
    assert_eq!(second.len(), 2);
    assert_eq!(second[0]["status"], "ok");
    assert_eq!(
        structural_output::<SessionResponse>(&second[0]["output"]),
        SessionResponse::Session {
            session: Some(SessionRecord::new(session_id)),
        }
    );
    assert_eq!(second[1]["status"], "ok");
    let ContextResponse::Resources { descriptors } = structural_output(&second[1]["output"]) else {
        panic!("context list returned the wrong response")
    };
    assert!(
        descriptors
            .iter()
            .any(|descriptor| descriptor.resource_id.as_str() == "process:context")
    );

    let _ = fs::remove_file(&state);
}

#[test]
fn portable_config_file_and_cli_profile_produce_identical_product_graph() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!(
        "phenix-config-parity-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&workspace).unwrap();
    let config = workspace.join("composition.json");
    fs::write(&config, r#"{"profile":"phenix.product.basic"}"#).unwrap();

    let run = |args: &[&str], config_environment: bool, suffix: &str| -> Value {
        let mut process = Command::new(env!("CARGO_BIN_EXE_phenix-harness"));
        process
            .args(args)
            .env("PHENIX_STATE_DB", workspace.join(format!("{suffix}.sqlite")))
            .env_remove("PHENIX_ENABLED_PLUGINS")
            .env_remove("PHENIX_CONFIG_FILE")
            .env_remove("PHENIX_LAYER_POLICY")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if config_environment {
            process.env("PHENIX_CONFIG_FILE", &config);
        }
        let output = process.output().expect("harness must launch");
        assert!(
            output.status.success(),
            "composition failed ({}): {}",
            suffix,
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("list-services emits JSON")
    };

    let via_cli = run(
        &["--profile", "phenix.product.basic", "--list-services"],
        false,
        "cli",
    );
    let via_file = run(
        &[
            "--config",
            config.to_str().expect("UTF-8 temporary file path"),
            "--list-services",
        ],
        false,
        "file",
    );
    let via_deployment = run(&["--list-services"], true, "deployment");

    assert_eq!(via_cli["plugins"], via_file["plugins"]);
    assert_eq!(via_cli["services"], via_file["services"]);
    assert_eq!(via_cli["plugins"], via_deployment["plugins"]);
    assert_eq!(via_cli["services"], via_deployment["services"]);
    assert!(via_file["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == "phenix.product.basic"));
    assert!(!via_file["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == "phenix.product.full"));

    let _ = fs::remove_dir_all(&workspace);
}
