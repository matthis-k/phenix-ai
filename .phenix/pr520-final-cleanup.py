from pathlib import Path


def replace_exact(path, old, new, expected=1):
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} matches, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new))


durable = "rust/crates/phenix-sdk/src/durable.rs"

replace_exact(
    durable,
    '''impl DurableKeyCodec<String> for Utf8HexKeyCodec {
    fn encode_key(&self, key: &String) -> Result<String, DurableCollectionError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(key.len() * 2);
        for byte in key.as_bytes() {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
        Ok(encoded)
    }
''',
    '''fn encode_hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

impl DurableKeyCodec<String> for Utf8HexKeyCodec {
    fn encode_key(&self, key: &String) -> Result<String, DurableCollectionError> {
        Ok(encode_hex_bytes(key.as_bytes()))
    }
''',
)
replace_exact(
    durable,
    "if encoded.len() % 2 != 0 {",
    "if !encoded.len().is_multiple_of(2) {",
)
replace_exact(
    durable,
    "for pair in bytes.chunks_exact(2) {",
    "for pair in bytes.as_chunks::<2>().0 {",
)
replace_exact(
    durable,
    '''    fn entry_prefix(&self) -> String {
        format!("{}/map/", self.collection)
    }''',
    '''    fn entry_prefix(&self) -> String {
        format!("{}/map/", encode_hex_bytes(self.collection.as_bytes()))
    }''',
)
replace_exact(
    durable,
    '''    fn tail_key(&self) -> String {
        format!("{}/log/@tail", self.collection)
    }

    fn entry_prefix(&self) -> String {
        format!("{}/log/entry/", self.collection)
    }''',
    '''    fn tail_key(&self) -> String {
        format!(
            "{}/log/@tail",
            encode_hex_bytes(self.collection.as_bytes())
        )
    }

    fn entry_prefix(&self) -> String {
        format!(
            "{}/log/entry/",
            encode_hex_bytes(self.collection.as_bytes())
        )
    }''',
)
replace_exact(
    durable,
    '''        let map = DurableMap::<String, String>::new(namespace.clone(), "users");
        let other = DurableMap::<String, String>::new(namespace.clone(), "other");''',
    '''        let map = DurableMap::<String, String>::new(namespace.clone(), "users");
        let other = DurableMap::<String, String>::new(namespace.clone(), "users/map/other");

        assert_eq!(map.entry_prefix(), "7573657273/map/");''',
)
replace_exact(
    durable,
    '''        let log = DurableLog::<String>::new(namespace.clone(), "events");

        assert_eq!(log.append_with(&access, &"first".to_owned()).unwrap(), 0);''',
    '''        let log = DurableLog::<String>::new(namespace.clone(), "events");

        assert_eq!(log.tail_key(), "6576656e7473/log/@tail");
        assert!(log
            .entries_with(&access, ScanDirection::Forward, None)
            .unwrap()
            .is_empty());
        assert_eq!(log.latest_with(&access).unwrap(), None);

        assert_eq!(log.append_with(&access, &"first".to_owned()).unwrap(), 0);''',
)

runtime_tests = "rust/crates/phenix-core/src/runtime/tests.rs"
replace_exact(
    runtime_tests,
    '''    let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
    kernel
        .persistence
        .lock()
        .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))''',
    '''    let kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
    kernel
        .persistence
        .lock()
        .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))''',
)

hooks = "rust/crates/phenix-plugin-hooks/src/implementation.rs"
replace_exact(
    hooks,
    '''            assert!(duplicate_error.contains("transaction assertion failed"));
            assert!(duplicate_error.contains("configuration/config-1"));''',
    '''            assert!(duplicate_error.contains("persistence assertion conflicted"));
            assert!(duplicate_error.contains("configuration/config-1"));''',
)

session_tree = "rust/crates/phenix-plugin-session-tree/src/session_tree_atomicity_regression.rs"
replace_exact(
    session_tree,
    '''    assert!(error.contains("transaction assertion failed"));
    assert!(session_exists(&mut kernel, "root"));''',
    '''    assert!(error.contains("persistence assertion conflicted"));
    assert!(session_exists(&mut kernel, "root"));''',
)

persistence_doc = "spec/plugin-persistence.md"
replace_exact(
    persistence_doc,
    "  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs\n",
    "  - rust/crates/phenix-sdk/tests/plugin_attribute_only_gate.rs\n  - rust/crates/phenix-sdk/src/durable.rs\n",
)
replace_exact(
    persistence_doc,
    '''- exact key reads;
- atomic namespace transactions;''',
    '''- exact key reads;
- bounded ordered record scans;
- atomic namespace transactions;''',
)
replace_exact(
    persistence_doc,
    '''## Backend features

A durable schema may require generic features such as transactions, unique keys, or migrations.

A Provider is eligible only when it supports every required feature in the resolved schema set. Unsupported requirements fail before the namespace is claimed or the target Store is opened.''',
    '''## Backend features

`Migrations` is the only optional backend operation today. Atomic transactions, one value per `(namespace, record_key)`, and bounded ordered scans are baseline `PersistenceBackend` behavior.

A Provider is eligible only when it supports every optional feature required by the resolved schema set. Unsupported requirements fail before the namespace is claimed or the target Store is opened.''',
)

durable_doc = "spec/plugin-durable-data.md"
replace_exact(
    durable_doc,
    '''The generic contract should support only the smallest useful query set:

- exact key lookup;
- bounded scan/list;
- declared indexed equality/range filters;
- declared insert/update/delete or append-only mutation;
- transaction-scoped reads where required.

FTS, vector search, graph traversal, semantic ranking, and other specialized query systems should remain separate service capabilities.''',
    '''The generic contract supports the smallest useful record operations:

- exact key lookup;
- bounded ordered scan/list;
- atomic put/delete mutations;
- value assertions for optimistic compare-and-swap coordination.

`phenix-sdk` builds typed collections on that contract. `DurableMap` provides deterministic encoded-key lookup and scans. `DurableLog` uses fixed-width sequence keys plus a tail assertion in one transaction. Collection names and map keys are encoded so collection prefixes cannot overlap through separator characters.

FTS, vector search, graph traversal, semantic ranking, and other specialized query systems remain separate service capabilities.''',
)
replace_exact(
    durable_doc,
    '''Schema contracts expose required generic backend features such as transactions, unique keys, foreign keys, ordered append, or indexed range operations.

An incompatible backend is rejected before store activation.''',
    '''Schema contracts may require optional backend operations. `Migrations` is the only optional backend feature today; transactions, unique record keys, and bounded ordered scans are baseline behavior.

An incompatible backend is rejected before store activation.''',
)
