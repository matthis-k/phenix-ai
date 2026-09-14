from pathlib import Path


def read(path):
    return Path(path).read_text()


def write(path, text):
    Path(path).write_text(text)


def replace_once(path, old, new):
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old[:80]!r}")
    write(path, text.replace(old, new, 1))


def replace_after(path, anchor, old, new):
    text = read(path)
    start = text.find(anchor)
    if start < 0:
        raise SystemExit(f"{path}: missing anchor {anchor!r}")
    before, tail = text[:start], text[start:]
    count = tail.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one post-anchor match, found {count}: {old[:80]!r}")
    write(path, before + tail.replace(old, new, 1))


persistence = "rust/crates/phenix-core/src/persistence.rs"
replace_once(
    persistence,
    "use rusqlite::{params, Connection, OptionalExtension, Transaction};\nuse serde::{Deserialize, Serialize};\nuse std::{collections::BTreeSet, path::Path};",
    "use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Transaction};\nuse serde::{Deserialize, Serialize};\nuse std::{collections::BTreeSet, ops::Bound, path::Path};",
)

scan_types = r'''
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableRecord {
    pub key: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScanDirection {
    #[default]
    Forward,
    Reverse,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableKeyRange {
    lower: Bound<String>,
    upper: Bound<String>,
}

impl DurableKeyRange {
    #[must_use]
    pub fn new(lower: Bound<String>, upper: Bound<String>) -> Self {
        Self { lower, upper }
    }

    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn prefix(prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        let upper = prefix_successor(&prefix)
            .map(Bound::Excluded)
            .unwrap_or(Bound::Unbounded);
        Self {
            lower: Bound::Included(prefix),
            upper,
        }
    }

    #[must_use]
    pub fn lower(&self) -> &Bound<String> {
        &self.lower
    }

    #[must_use]
    pub fn upper(&self) -> &Bound<String> {
        &self.upper
    }
}

impl Default for DurableKeyRange {
    fn default() -> Self {
        Self {
            lower: Bound::Unbounded,
            upper: Bound::Unbounded,
        }
    }
}

fn prefix_successor(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let mut next = u32::from(last).checked_add(1)?;
        if (0xD800..=0xDFFF).contains(&next) {
            next = 0xE000;
        }
        if let Some(next) = char::from_u32(next) {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

'''
replace_once(
    persistence,
    "#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]\npub enum TransactionOp",
    scan_types + "#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]\npub enum TransactionOp",
)

trait_read = '''    fn read(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, PersistenceError>;

    fn transact_many('''
trait_scan = '''    fn read(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, PersistenceError>;

    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: usize,
    ) -> Result<Vec<DurableRecord>, PersistenceError>;

    fn transact_many('''
replace_once(persistence, trait_read, trait_scan)

local_scan = r'''    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: usize,
    ) -> Result<Vec<DurableRecord>, PersistenceError> {
        self.require_owner(caller, namespace)?;
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut sql = String::from(
            "SELECT record_key, record_value FROM kernel_plugin_records WHERE namespace = ?1",
        );
        let mut values = vec![Value::Text(namespace.as_str().to_owned())];
        match range.lower() {
            Bound::Included(key) => {
                let index = values.len() + 1;
                sql.push_str(&format!(" AND record_key >= ?{index}"));
                values.push(Value::Text(key.clone()));
            }
            Bound::Excluded(key) => {
                let index = values.len() + 1;
                sql.push_str(&format!(" AND record_key > ?{index}"));
                values.push(Value::Text(key.clone()));
            }
            Bound::Unbounded => {}
        }
        match range.upper() {
            Bound::Included(key) => {
                let index = values.len() + 1;
                sql.push_str(&format!(" AND record_key <= ?{index}"));
                values.push(Value::Text(key.clone()));
            }
            Bound::Excluded(key) => {
                let index = values.len() + 1;
                sql.push_str(&format!(" AND record_key < ?{index}"));
                values.push(Value::Text(key.clone()));
            }
            Bound::Unbounded => {}
        }
        match direction {
            ScanDirection::Forward => sql.push_str(" ORDER BY record_key ASC"),
            ScanDirection::Reverse => sql.push_str(" ORDER BY record_key DESC"),
        }
        let limit_index = values.len() + 1;
        sql.push_str(&format!(" LIMIT ?{limit_index}"));
        values.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));

        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok(DurableRecord {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

'''
replace_after(
    persistence,
    "impl PersistenceBackend for LocalPersistence {",
    "    fn transact_many(\n        &mut self,\n        transactions: &[NamespaceTransaction],\n    ) -> Result<(), PersistenceError> {",
    local_scan + "    fn transact_many(\n        &mut self,\n        transactions: &[NamespaceTransaction],\n    ) -> Result<(), PersistenceError> {",
)

prefix_test = r'''    #[test]
    fn prefix_range_covers_only_keys_with_the_prefix() {
        let range = DurableKeyRange::prefix("items/beta");
        assert_eq!(range.lower(), &Bound::Included("items/beta".into()));
        assert_eq!(range.upper(), &Bound::Excluded("items/betb".into()));

        let max = DurableKeyRange::prefix(format!("items/{}", char::MAX));
        assert_eq!(max.lower(), &Bound::Included(format!("items/{}", char::MAX)));
        assert_eq!(max.upper(), &Bound::Excluded("itemt".into()));
    }

'''
replace_once(
    persistence,
    "    #[test]\n    fn multi_plugin_transaction_commits_or_rolls_back_as_one_unit()",
    prefix_test + "    #[test]\n    fn multi_plugin_transaction_commits_or_rolls_back_as_one_unit()",
)

replace_once(
    "rust/crates/phenix-core/src/lib.rs",
    "    BackendFeature, DurableSchema, LocalPersistence, NamespaceTransaction, PersistenceBackend,\n    PersistenceError, SchemaMigration, TransactionOp,",
    "    BackendFeature, DurableKeyRange, DurableRecord, DurableSchema, LocalPersistence,\n    NamespaceTransaction, PersistenceBackend, PersistenceError, ScanDirection, SchemaMigration,\n    TransactionOp,",
)

replace_once(
    "rust/crates/phenix-core/src/runtime.rs",
    "    DurableSchema, EventAdmissionReceipt, EventBus, EventEnvelope, EventError, EventHandler,",
    "    DurableKeyRange, DurableRecord, DurableSchema, EventAdmissionReceipt, EventBus, EventEnvelope,\n    EventError, EventHandler,",
)
replace_once(
    "rust/crates/phenix-core/src/runtime.rs",
    "    ResolvedServiceChain, ResourceNamespace, RuntimeId, SchemaMigration, ServiceId, ServiceRole,\n    SkillResourceMetadata, TaskRuntime, TaskScope, TransactionOp,",
    "    ResolvedServiceChain, ResourceNamespace, RuntimeId, ScanDirection, SchemaMigration, ServiceId,\n    ServiceRole, SkillResourceMetadata, TaskRuntime, TaskScope, TransactionOp,",
)

host = "rust/crates/phenix-core/src/runtime/host.rs"
host_read = '''    pub fn read_durable(
        &self,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KernelError> {
        self.require_persistence_operation(PERSISTENCE_READ, namespace)?;
        self.persistence
            .lock()
            .read(self.plugin, namespace, key)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

'''
host_scan = host_read + '''    pub fn scan_durable(
        &self,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: usize,
    ) -> Result<Vec<DurableRecord>, KernelError> {
        self.require_persistence_operation(PERSISTENCE_READ, namespace)?;
        self.persistence
            .lock()
            .scan(self.plugin, namespace, range, direction, limit)
            .map_err(|error| self.persistence_error(error.to_string()))
    }

'''
replace_once(host, host_read, host_scan)

context = "rust/crates/phenix-core/src/plugin_context.rs"
replace_once(
    context,
    "    DurableSchema, EventAdmissionReceipt, EventError, EventTypeId, Exact, GraphGenerationId,",
    "    DurableKeyRange, DurableRecord, DurableSchema, EventAdmissionReceipt, EventError, EventTypeId,\n    Exact, GraphGenerationId,",
)
replace_once(
    context,
    "    ResourceNamespace, SchemaMigration, ServiceId, TaskScope, TransactionOp, ValueError,",
    "    ResourceNamespace, ScanDirection, SchemaMigration, ServiceId, TaskScope, TransactionOp,\n    ValueError,",
)
context_read = '''    pub fn read_durable(
        &self,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KernelError> {
        self.host.read_durable(namespace, key)
    }

'''
context_scan = context_read + '''    pub fn scan_durable(
        &self,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: usize,
    ) -> Result<Vec<DurableRecord>, KernelError> {
        self.host.scan_durable(namespace, range, direction, limit)
    }

'''
replace_once(context, context_read, context_scan)

conformance = "rust/crates/phenix-core/tests/persistence_backend_conformance.rs"
replace_once(
    conformance,
    "    BackendFeature, DurableSchema, LocalPersistence, NamespaceTransaction, PersistenceBackend,\n    PersistenceError, PluginId, ResourceNamespace, SchemaMigration, TransactionOp,",
    "    BackendFeature, DurableKeyRange, DurableRecord, DurableSchema, LocalPersistence,\n    NamespaceTransaction, PersistenceBackend, PersistenceError, PluginId, ResourceNamespace,\n    ScanDirection, SchemaMigration, TransactionOp,",
)
replace_once(
    conformance,
    "use std::collections::{BTreeMap, BTreeSet};",
    "use std::{\n    collections::{BTreeMap, BTreeSet},\n    ops::Bound,\n};",
)
memory_scan = r'''    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: usize,
    ) -> Result<Vec<DurableRecord>, PersistenceError> {
        self.require_owner(caller, namespace)?;
        let in_range = |key: &str| {
            let above_lower = match range.lower() {
                Bound::Included(bound) => key >= bound.as_str(),
                Bound::Excluded(bound) => key > bound.as_str(),
                Bound::Unbounded => true,
            };
            let below_upper = match range.upper() {
                Bound::Included(bound) => key <= bound.as_str(),
                Bound::Excluded(bound) => key < bound.as_str(),
                Bound::Unbounded => true,
            };
            above_lower && below_upper
        };
        let mut records: Vec<_> = self
            .records
            .iter()
            .filter(|((record_namespace, key), _)| {
                record_namespace == namespace && in_range(key)
            })
            .map(|((_, key), value)| DurableRecord {
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        if direction == ScanDirection::Reverse {
            records.reverse();
        }
        records.truncate(limit);
        Ok(records)
    }

'''
replace_after(
    conformance,
    "impl PersistenceBackend for MemoryPersistence {",
    "    fn transact_many(\n        &mut self,\n        transactions: &[NamespaceTransaction],\n    ) -> Result<(), PersistenceError> {",
    memory_scan + "    fn transact_many(\n        &mut self,\n        transactions: &[NamespaceTransaction],\n    ) -> Result<(), PersistenceError> {",
)
scan_assertions = r'''    backend
        .transact(
            &first_owner,
            &first_namespace,
            &[
                TransactionOp::Put {
                    key: "alpha".into(),
                    value: b"a".to_vec(),
                },
                TransactionOp::Put {
                    key: "beta".into(),
                    value: b"b".to_vec(),
                },
                TransactionOp::Put {
                    key: "beta-2".into(),
                    value: b"b2".to_vec(),
                },
                TransactionOp::Put {
                    key: "gamma".into(),
                    value: b"g".to_vec(),
                },
            ],
        )
        .unwrap();

    let bounded = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::new(
                Bound::Included("alpha".into()),
                Bound::Excluded("gamma".into()),
            ),
            ScanDirection::Forward,
            10,
        )
        .unwrap();
    assert_eq!(
        bounded.iter().map(|record| record.key.as_str()).collect::<Vec<_>>(),
        vec!["alpha", "beta", "beta-2"]
    );

    let prefixed = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::prefix("beta"),
            ScanDirection::Forward,
            10,
        )
        .unwrap();
    assert_eq!(
        prefixed.iter().map(|record| record.key.as_str()).collect::<Vec<_>>(),
        vec!["beta", "beta-2"]
    );

    let reverse = backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Reverse,
            2,
        )
        .unwrap();
    assert_eq!(
        reverse.iter().map(|record| record.key.as_str()).collect::<Vec<_>>(),
        vec!["record", "gamma"]
    );
    assert!(backend
        .scan(
            &first_owner,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            0,
        )
        .unwrap()
        .is_empty());
    assert!(matches!(
        backend.scan(
            &outsider,
            &first_namespace,
            &DurableKeyRange::all(),
            ScanDirection::Forward,
            1,
        ),
        Err(PersistenceError::WrongNamespaceOwner { .. })
    ));
    assert_eq!(
        backend
            .scan(
                &second_owner,
                &second_namespace,
                &DurableKeyRange::all(),
                ScanDirection::Forward,
                10,
            )
            .unwrap(),
        vec![DurableRecord {
            key: "record".into(),
            value: b"second".to_vec(),
        }]
    );

'''
replace_once(
    conformance,
    "    let migrated_schema = DurableSchema::requiring(",
    scan_assertions + "    let migrated_schema = DurableSchema::requiring(",
)

bootstrap = "rust/crates/phenix-core/src/runtime/persistence_bootstrap.rs"
replace_once(
    bootstrap,
    "        BackendFeature, DurableSchema, NamespaceTransaction, ResourceNamespace, TransactionOp,",
    "        BackendFeature, DurableKeyRange, DurableRecord, DurableSchema, NamespaceTransaction,\n        ResourceNamespace, ScanDirection, TransactionOp,",
)
recording_scan = '''        fn scan(
            &self,
            _caller: &PluginId,
            _namespace: &ResourceNamespace,
            _range: &DurableKeyRange,
            _direction: ScanDirection,
            _limit: usize,
        ) -> Result<Vec<DurableRecord>, PersistenceError> {
            Ok(Vec::new())
        }

'''
replace_after(
    bootstrap,
    "impl PersistenceBackend for RecordingBackend {",
    "        fn transact_many(\n            &mut self,\n            _transactions: &[NamespaceTransaction],\n        ) -> Result<(), PersistenceError> {",
    recording_scan + "        fn transact_many(\n            &mut self,\n            _transactions: &[NamespaceTransaction],\n        ) -> Result<(), PersistenceError> {",
)

session_tree = "rust/crates/phenix-plugin-session-tree/src/session_tree_atomicity_regression.rs"
replace_once(
    session_tree,
    "    Authority, BackendFeature, DurableSchema, Kernel, KernelConfig, LocalPersistence,\n    NamespaceTransaction, PersistenceBackend, PersistenceError, PhenixValue, PluginId, Project,\n    ResolvedHarness, ResolvedHarnessActivation, ResourceNamespace, SchemaMigration, SessionId,\n    TransactionOp,",
    "    Authority, BackendFeature, DurableKeyRange, DurableRecord, DurableSchema, Kernel, KernelConfig,\n    LocalPersistence, NamespaceTransaction, PersistenceBackend, PersistenceError, PhenixValue,\n    PluginId, Project, ResolvedHarness, ResolvedHarnessActivation, ResourceNamespace, ScanDirection,\n    SchemaMigration, SessionId, TransactionOp,",
)
wrapper_scan = '''    fn scan(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        range: &DurableKeyRange,
        direction: ScanDirection,
        limit: usize,
    ) -> Result<Vec<DurableRecord>, PersistenceError> {
        self.inner.scan(caller, namespace, range, direction, limit)
    }

'''
replace_after(
    session_tree,
    "impl PersistenceBackend for FailMultiNamespaceTransaction {",
    "    fn transact_many(\n        &mut self,\n        transactions: &[NamespaceTransaction],\n    ) -> Result<(), PersistenceError> {",
    wrapper_scan + "    fn transact_many(\n        &mut self,\n        transactions: &[NamespaceTransaction],\n    ) -> Result<(), PersistenceError> {",
)
