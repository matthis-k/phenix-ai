use crate::{memory_factory, memory_manifest};
use phenix_core::{Kernel, KernelConfig, LocalPersistence, PhenixValue, SessionId};
use phenix_sdk::{
    memory_service, MemoryCommand, MemoryKind, MemoryRecord, MemoryResponse, MemoryScope,
    MemorySourceReference,
};
use std::{
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
        "phenix-memory-restart-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn memory_kernel(path: &PathBuf) -> Kernel {
    let manifest = memory_manifest();
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, memory_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: MemoryCommand) -> MemoryResponse {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &memory_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    output.project().unwrap()
}

#[test]
fn disabling_and_reenabling_memory_preserves_compatible_durable_state() {
    let path = temp_db();
    let record = MemoryRecord {
        id: "durable-fact".into(),
        kind: MemoryKind::Fact,
        scope: MemoryScope::Session {
            session_id: SessionId::parse("root").unwrap(),
        },
        content: "persistent memory".into(),
        source_refs: vec![MemorySourceReference {
            service: phenix_core::ServiceId::parse("fixture.history@1").unwrap(),
            resource: "turn/1".into(),
            start: None,
            end: None,
        }],
        supporting_dependencies: Vec::new(),
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at: 10,
    };

    let mut enabled = memory_kernel(&path);
    assert_eq!(
        invoke(
            &mut enabled,
            MemoryCommand::Record {
                record: record.clone(),
            },
        ),
        MemoryResponse::Record {
            record: record.clone(),
        }
    );
    drop(enabled);

    let disabled_persistence = LocalPersistence::open(&path).unwrap();
    let disabled = Kernel::with_persistence(
        KernelConfig::new(Vec::<phenix_core::PluginManifest>::new()).unwrap(),
        disabled_persistence,
    );
    drop(disabled);

    let mut reenabled = memory_kernel(&path);
    assert_eq!(
        invoke(
            &mut reenabled,
            MemoryCommand::Get {
                id: record.id.clone(),
            },
        ),
        MemoryResponse::Memory {
            record: Some(record),
        }
    );

    drop(reenabled);
    let _ = fs::remove_file(path);
}
