use crate::{memory_factory, memory_manifest};
use phenix_core::{Kernel, KernelConfig, LocalPersistence, PhenixValue, SessionId};
use phenix_sdk::{
    context_compaction_service, memory_service, MemoryCommand, MemoryKind, MemoryRecord,
    MemoryScope, MemorySourceReference,
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
        "phenix-memory-provenance-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

#[test]
fn durable_memory_rejects_summary_only_provenance() {
    let path = temp_db();
    let manifest = memory_manifest();
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(&path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, memory_factory)
        .unwrap();
    kernel.activate_all().unwrap();

    let command = MemoryCommand::Record {
        record: MemoryRecord {
            id: "derived-from-summary".into(),
            kind: MemoryKind::Fact,
            scope: MemoryScope::Session {
                session_id: SessionId::parse("root").unwrap(),
            },
            content: "summary-only memory".into(),
            source_refs: vec![MemorySourceReference {
                service: context_compaction_service(),
                resource: "checkpoint/summary".into(),
                start: None,
                end: None,
            }],
            supporting_dependencies: Vec::new(),
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            created_at: 10,
        },
    };
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let error = kernel
        .invoke(
            &memory_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("must reference raw durable evidence"));
    let _ = fs::remove_file(path);
}
