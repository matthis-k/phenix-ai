use crate::{session_factory, session_manifest};
use phenix_core::{CallableId, Kernel, KernelConfig, LocalPersistence, PhenixValue, SessionId};
use phenix_sdk::{
    session_history_resource, session_service, SessionCommand, SessionHistoryContentPart,
    SessionHistoryDraft, SessionHistoryFinishReason, SessionHistoryRole, SessionHistoryToolCall,
    SessionHistoryToolOutcome, SessionHistoryToolResult, SessionHistoryUsage, SessionHistoryValue,
    SessionResponse,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-session-history-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let manifest = session_manifest();
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, session_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: SessionCommand) -> SessionResponse {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &session_service(),
            &input,
            &session_manifest().maximum_authority,
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    output.project().unwrap()
}

#[test]
fn retained_portable_model_and_tool_history_is_exactly_addressable() {
    let path = temp_db();
    let session_id = SessionId::parse("memory-source").unwrap();
    let callable_id = CallableId::parse("tools.read_file").unwrap();
    let arguments = SessionHistoryValue::from(PhenixValue::Map(BTreeMap::from([(
        "path".into(),
        PhenixValue::String("README.md".into()),
    )])));
    let result = SessionHistoryValue::from(PhenixValue::String("contents".into()));
    let draft = SessionHistoryDraft {
        role: SessionHistoryRole::Assistant,
        content: vec![SessionHistoryContentPart::Text {
            text: "I will inspect the repository.".into(),
        }],
        tool_calls: vec![SessionHistoryToolCall {
            call_id: "call-1".into(),
            callable_id: callable_id.clone(),
            arguments: arguments.clone(),
        }],
        tool_results: vec![SessionHistoryToolResult {
            call_id: "call-1".into(),
            callable_id,
            result: SessionHistoryToolOutcome::Success {
                value: result.clone(),
            },
        }],
        finish_reason: Some(SessionHistoryFinishReason::Complete),
        usage: Some(SessionHistoryUsage {
            input_tokens: Some(12),
            output_tokens: Some(8),
        }),
        context_revision: "context-7".into(),
        instruction_revision: "instructions-3".into(),
    };

    let mut kernel = kernel_with(&path);
    invoke(
        &mut kernel,
        SessionCommand::Create {
            session: phenix_sdk::SessionRecord::new(session_id.clone()),
        },
    );
    let appended = invoke(
        &mut kernel,
        SessionCommand::AppendHistory {
            id: session_id.clone(),
            entry: draft,
        },
    );
    let SessionResponse::HistoryAppended { entry } = appended else {
        panic!("append history must return the durable entry");
    };
    assert_eq!(entry.sequence, 1);

    let resource = session_history_resource(&session_id, entry.sequence);
    let resolved = invoke(&mut kernel, SessionCommand::ResolveHistory { resource });
    let SessionResponse::HistoryEntry {
        entry: Some(resolved),
    } = resolved
    else {
        panic!("history resource must resolve to the retained entry");
    };

    assert_eq!(resolved, entry);
    assert_eq!(resolved.tool_calls[0].arguments, arguments);
    assert!(matches!(
        &resolved.tool_results[0].result,
        SessionHistoryToolOutcome::Success { value } if value == &result
    ));

    drop(kernel);
    let mut restored = kernel_with(&path);
    let resource = session_history_resource(&session_id, 1);
    assert_eq!(
        invoke(&mut restored, SessionCommand::ResolveHistory { resource }),
        SessionResponse::HistoryEntry { entry: Some(entry) }
    );
    let _ = fs::remove_file(path);
}
