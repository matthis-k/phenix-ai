//! Real SQLite restoration and provider-visible history. No fake storage.

use phenix_agent::{
    AgentEvent,
    conversation::{Conversation, TurnCodec, TurnRecord},
    harness::{self, Composition, Environment},
    model::{context::ModelContext, fake::provider},
};
use phenix_storage::{Address, Codec, Grant, Operation, Registry, Scope};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn provision(path: &Path) -> Conversation {
    let registry = Registry::new();
    registry.mount_sqlite("conversation", path).unwrap();
    let address = Address::parse("store://conversation/turns").unwrap();
    registry.register(&address, TurnCodec.schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Dataset(address.clone()),
        [Operation::Read, Operation::Write],
    )]);
    Conversation::new(session, address, 8, 128 * 1024)
}

#[test]
fn reopen_storage_and_observe_previous_turn_in_actual_model_context() {
    let path = std::env::temp_dir().join(format!(
        "phenix-history-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let first = TurnRecord {
        prompt: "previous question".into(),
        assistant: "previous answer".into(),
        used_tools: false,
    };
    {
        let history = provision(&path);
        history.append(0, &first).unwrap();
        assert!(history.append(0, &first).is_err());
    }
    {
        let history = provision(&path);
        assert_eq!(history.load().unwrap(), vec![first.clone()]);
        let host = harness::compose(
            |context| provider(history.context(context.clone())),
            Environment::new(),
            BTreeMap::new(),
            Composition::default(),
        )
        .unwrap();
        let generation = host.active().unwrap();
        let expected = ModelContext {
            tools: BTreeMap::from([("local".into(), BTreeMap::new())]),
            history: vec![first.clone()],
            ..Default::default()
        };
        let mut response = String::new();
        history
            .run_turn(&generation, "inspect restored history".into(), 256, |event| {
                if let AgentEvent::Delta(text) = event {
                    response.push_str(text);
                }
            })
            .unwrap();
        assert_eq!(response, format!("{expected:#?}"));
        println!("[restored-model-context] {response}");
        let turns = history.load().unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].assistant, response);
        assert!(
            history
                .run_turn(&generation, "cancel before execution".into(), 0, |_| {})
                .is_err()
        );
        assert_eq!(history.load().unwrap(), turns);
    }
    std::fs::remove_file(path).unwrap();
}
