use phenix_storage::{
    scoped::{CellSpec, Identity, ResourceCatalog, ResourceRequest, Storage, Stored},
    snapshot::{ReadConsistency, ReadKey, ReadRequest, SnapshotReader},
    *,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering::SeqCst},
};

const REV: u8 = 1;
const BATCH: u8 = 2;
const CAS: u8 = 4;
const CELL: CellSpec<Number> = CellSpec::project_state("test.capabilities/value");

#[derive(Debug, PartialEq)]
struct Number(u8);
#[derive(Default)]
struct Format;
impl Stored for Number {
    type Format = Format;
}
impl Codec for Format {
    type Value = Number;
    fn schema(&self) -> Schema {
        Schema {
            name: "number".into(),
            revision: 1,
            structure: "u8".into(),
            codec: "raw".into(),
        }
    }
    fn encode(&self, value: &Number) -> Result<Vec<u8>> {
        Ok(vec![value.0])
    }
    fn decode(&self, bytes: &[u8]) -> Result<Number> {
        Ok(Number(bytes[0]))
    }
}

#[derive(Default)]
struct Control {
    flags: AtomicU8,
    probes: AtomicUsize,
    lose_on_probe: AtomicUsize,
    lose_after_revision: AtomicBool,
    plain: AtomicUsize,
    revision: AtomicUsize,
    batch: AtomicUsize,
    cas: AtomicUsize,
}
struct Independent {
    control: Arc<Control>,
    values: MemoryBackend,
}
impl Backend for Independent {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        let probe = self.control.probes.fetch_add(1, SeqCst) + 1;
        if probe == self.control.lose_on_probe.load(SeqCst) {
            self.control.flags.store(0, SeqCst);
        }
        Some(self)
    }
}
impl TypedKv for Independent {
    fn get(&mut self, dataset: &str, key: &str) -> Result<Option<Vec<u8>>> {
        self.control.plain.fetch_add(1, SeqCst);
        self.values.get(dataset, key)
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> Result<()> {
        self.values.put(dataset, key, value)
    }
    fn delete(&mut self, dataset: &str, key: &str) -> Result<bool> {
        self.values.delete(dataset, key)
    }
    fn revisioned_read(&mut self) -> Option<&mut dyn phenix_storage::RevisionedRead> {
        (self.control.flags.load(SeqCst) & REV != 0).then_some(self)
    }
    fn atomic_read_many(&mut self) -> Option<&mut dyn phenix_storage::AtomicReadMany> {
        (self.control.flags.load(SeqCst) & BATCH != 0).then_some(self)
    }
    fn atomic_compare_exchange(&mut self) -> Option<&mut dyn phenix_storage::CompareExchange> {
        (self.control.flags.load(SeqCst) & CAS != 0).then_some(self)
    }
}
impl phenix_storage::RevisionedRead for Independent {
    fn get_revisioned(&mut self, dataset: &str, key: &str) -> Result<Option<Revisioned<Vec<u8>>>> {
        self.control.revision.fetch_add(1, SeqCst);
        let value = self.values.get_revisioned(dataset, key);
        if self.control.lose_after_revision.load(SeqCst) {
            self.control.flags.store(0, SeqCst);
        }
        value
    }
}
impl phenix_storage::AtomicReadMany for Independent {
    fn read_many(&mut self, keys: &[ReadKey<'_>]) -> Result<Vec<Option<Revisioned<Vec<u8>>>>> {
        self.control.batch.fetch_add(1, SeqCst);
        self.values.read_many(keys)
    }
}
impl phenix_storage::CompareExchange for Independent {
    fn compare_exchange(
        &mut self,
        dataset: &str,
        key: &str,
        expected: Option<&Revision>,
        value: Vec<u8>,
    ) -> Result<Revision> {
        self.control.cas.fetch_add(1, SeqCst);
        self.values.compare_exchange(dataset, key, expected, value)
    }
}

fn setup(
    flags: u8,
    operations: &[Operation],
) -> (Registry, Session, Storage, Arc<Control>, Address) {
    let registry = Registry::new();
    let control = Arc::new(Control::default());
    control.flags.store(flags, SeqCst);
    let mut values = MemoryBackend::default();
    values.put("values", "value", vec![7]).unwrap();
    values.put("values", "other", vec![9]).unwrap();
    registry
        .mount(
            "main",
            Independent {
                control: control.clone(),
                values,
            },
        )
        .unwrap();
    let address = Address::parse("store://main/values").unwrap();
    registry.register(&address, Format.schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Dataset(address.clone()),
        operations.iter().copied(),
    )]);
    let identity = Identity::user("u").unwrap().project("p").unwrap();
    let mut catalog = ResourceCatalog::new();
    catalog
        .bind(
            CELL.resource_spec().resolve(&identity).unwrap(),
            address.clone(),
        )
        .unwrap();
    let storage = catalog.freeze().access(session.clone(), identity);
    (registry, session, storage, control, address)
}

fn unsupported<T>(result: Result<T>, expected: Capability) {
    assert!(
        matches!(result, Err(Error::UnsupportedCapability { capability }) if capability == expected)
    );
}
fn counts(control: &Control) -> [usize; 4] {
    [
        &control.plain,
        &control.revision,
        &control.batch,
        &control.cas,
    ]
    .map(|n| n.load(SeqCst))
}

#[test]
fn plain_and_revision_only_scoped_reads_are_independent() {
    for flags in [0, REV] {
        let (_registry, session, storage, control, address) = setup(flags, &[Operation::Read]);
        let kv = storage
            .project()
            .unwrap()
            .state::<Number>("test.capabilities/value")
            .unwrap();
        assert_eq!(kv.get("value").unwrap(), Some(Number(7)));
        assert_eq!(kv.get("absent").unwrap(), None);
        let caps = session.capabilities(&address, Operation::Read).unwrap();
        assert!(caps.typed_kv);
        assert_eq!(caps.revisioned_read, flags == REV);
        assert!(!caps.atomic_read_many && !caps.compare_exchange);
        if flags == REV {
            assert_eq!(
                kv.get_revisioned("value").unwrap().unwrap().value,
                Number(7)
            );
        } else {
            unsupported(kv.get_revisioned("value"), Capability::RevisionedRead);
        }
        assert!(matches!(
            storage.bind(CELL.read()),
            Err(phenix_storage::scoped::Error::Storage(
                Error::UnsupportedCapability {
                    capability: Capability::AtomicReadMany
                }
            ))
        ));
        assert!(matches!(
            kv.compare_exchange("new", None, &Number(1)),
            Err(Error::Denied)
        ));
        // The read-only session rejects writes before probing any write capability.
        assert!(matches!(kv.put("value", &Number(1)), Err(Error::Denied)));
        assert_eq!(counts(&control), [2, usize::from(flags == REV), 0, 0]);
    }
}

#[test]
fn batch_only_populated_cells_duplicates_and_absence_use_only_batch() {
    let (_registry, session, storage, control, address) = setup(BATCH, &[Operation::Read]);
    let cell = storage.bind(CELL.read()).unwrap();
    assert_eq!(counts(&control), [0; 4]);
    assert_eq!(cell.get().unwrap().value, Number(7));
    let schema = Format.schema();
    let requests: Vec<_> = ["value", "other", "value", "absent"]
        .into_iter()
        .map(|key| ReadRequest {
            address: &address,
            key,
            schema: &schema,
        })
        .collect();
    for consistency in [
        ReadConsistency::AtomicWithinMount,
        ReadConsistency::BestEffort,
    ] {
        let snapshot = session.read_many(&requests, consistency).unwrap();
        assert_eq!(snapshot.consistency(), consistency);
        let entries = snapshot.entries();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].value().unwrap().value, [7]);
        assert_eq!(entries[1].value().unwrap().value, [9]);
        assert_eq!(entries[0].value(), entries[2].value());
        assert!(entries[3].value().is_none());
    }
    unsupported(
        session.get_revisioned(&address, "value", &Format),
        Capability::RevisionedRead,
    );
    assert_eq!(counts(&control), [0, 0, 3, 0]);
}

#[test]
fn cas_only_write_authority_can_create_and_update_without_reads() {
    let (_registry, session, storage, control, address) = setup(CAS, &[Operation::Write]);
    let kv = storage
        .project()
        .unwrap()
        .state::<Number>("test.capabilities/value")
        .unwrap();
    let first = kv.compare_exchange("new", None, &Number(1)).unwrap();
    let next = kv
        .compare_exchange("new", Some(&first), &Number(2))
        .unwrap();
    assert_ne!(first, next);
    assert!(matches!(kv.get("new"), Err(Error::Denied)));
    assert!(matches!(kv.get_revisioned("new"), Err(Error::Denied)));
    assert!(matches!(
        storage.bind(CELL.read()),
        Err(phenix_storage::scoped::Error::Storage(Error::Denied))
    ));
    let caps = session.capabilities(&address, Operation::Write).unwrap();
    assert!(caps.compare_exchange && !caps.revisioned_read && !caps.atomic_read_many);
    assert_eq!(counts(&control), [0, 0, 0, 2]);
}

#[test]
fn cell_requests_only_require_batch_for_read_and_requirements_are_idempotent() {
    for ops in [vec![], vec![Operation::Write], vec![Operation::Subscribe]] {
        assert!(CELL.request(ops).required.is_empty());
    }
    let read = CELL.request([Operation::Read, Operation::Write]);
    assert_eq!(read.required, [Capability::AtomicReadMany].into());
    assert!(
        ResourceRequest::new(CELL.resource_spec(), [Operation::Read])
            .required
            .is_empty()
    );
    let request = read
        .require(Capability::AtomicReadMany)
        .require(Capability::CompareExchange);
    assert_eq!(request.required.len(), 2);
}

#[test]
fn snapshot_capability_loss_is_checked_after_preflight_and_between_fallback_reads() {
    for flags in [BATCH, REV] {
        let (_registry, session, storage, control, address) = setup(flags, &[Operation::Read]);
        if flags == BATCH {
            let cell = storage.bind(CELL.read()).unwrap();
            for consistency in [
                ReadConsistency::AtomicWithinMount,
                ReadConsistency::BestEffort,
            ] {
                control.flags.store(BATCH, SeqCst);
                let reader = SnapshotReader::new(vec![cell.source()], consistency).unwrap();
                control
                    .lose_on_probe
                    .store(control.probes.load(SeqCst) + 2, SeqCst);
                unsupported(
                    reader.read(),
                    if consistency == ReadConsistency::AtomicWithinMount {
                        Capability::AtomicReadMany
                    } else {
                        Capability::RevisionedRead
                    },
                );
            }
        } else {
            let schema = Format.schema();
            let requests = ["value", "other"].map(|key| ReadRequest {
                address: &address,
                key,
                schema: &schema,
            });
            control
                .lose_on_probe
                .store(control.probes.load(SeqCst) + 3, SeqCst);
            unsupported(
                session.read_many(&requests, ReadConsistency::BestEffort),
                Capability::RevisionedRead,
            );
            assert_eq!(counts(&control), [0; 4]);
            control.lose_on_probe.store(0, SeqCst);
            control.flags.store(REV, SeqCst);
            control.lose_after_revision.store(true, SeqCst);
            unsupported(
                session.read_many(&requests, ReadConsistency::BestEffort),
                Capability::RevisionedRead,
            );
            assert_eq!(counts(&control), [0, 1, 0, 0]);
        }
        control.flags.store(0, SeqCst);
        unsupported(
            session.get_revisioned(&address, "value", &Format),
            Capability::RevisionedRead,
        );
        if flags == BATCH {
            assert_eq!(counts(&control), [0; 4]);
        }
    }
}

#[test]
fn cas_capability_loss_during_encoding_prevents_mutation() {
    struct LoseCapability(Arc<Control>);
    impl Codec for LoseCapability {
        type Value = Number;
        fn schema(&self) -> Schema {
            Format.schema()
        }
        fn encode(&self, value: &Number) -> Result<Vec<u8>> {
            self.0.flags.store(0, SeqCst);
            Format.encode(value)
        }
        fn decode(&self, _: &[u8]) -> Result<Number> {
            panic!("write must not decode")
        }
    }
    let (_registry, session, _storage, control, address) = setup(CAS, &[Operation::Write]);
    unsupported(
        session.compare_exchange(
            &address,
            "new",
            None,
            &Number(1),
            &LoseCapability(control.clone()),
        ),
        Capability::CompareExchange,
    );
    assert_eq!(counts(&control), [0; 4]);
}

#[test]
fn implementing_capability_traits_without_exposing_them_does_not_opt_in() {
    struct NoOptIn;
    impl Backend for NoOptIn {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            Some(self)
        }
    }
    impl TypedKv for NoOptIn {
        fn get(&mut self, _: &str, _: &str) -> Result<Option<Vec<u8>>> {
            Ok(Some(vec![7]))
        }
        fn put(&mut self, _: &str, _: &str, _: Vec<u8>) -> Result<()> {
            panic!("unexpected put")
        }
        fn delete(&mut self, _: &str, _: &str) -> Result<bool> {
            panic!("unexpected delete")
        }
    }
    impl phenix_storage::RevisionedRead for NoOptIn {
        fn get_revisioned(&mut self, _: &str, _: &str) -> Result<Option<Revisioned<Vec<u8>>>> {
            panic!("capability reference must gate revision payload")
        }
    }
    impl phenix_storage::AtomicReadMany for NoOptIn {
        fn read_many(&mut self, _: &[ReadKey<'_>]) -> Result<Vec<Option<Revisioned<Vec<u8>>>>> {
            panic!("capability reference must gate batch payload")
        }
    }
    impl phenix_storage::CompareExchange for NoOptIn {
        fn compare_exchange(
            &mut self,
            _: &str,
            _: &str,
            _: Option<&Revision>,
            _: Vec<u8>,
        ) -> Result<Revision> {
            panic!("capability reference must gate CAS payload")
        }
    }
    let registry = Registry::new();
    registry.mount("main", NoOptIn).unwrap();
    let address = Address::parse("store://main/values").unwrap();
    let schema = Format.schema();
    registry.register(&address, schema.clone()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Dataset(address.clone()),
        [Operation::Read, Operation::Write],
    )]);
    let caps = session.capabilities(&address, Operation::Read).unwrap();
    assert!(!caps.revisioned_read && !caps.atomic_read_many && !caps.compare_exchange);
    assert_eq!(
        session.get(&address, "value", &Format).unwrap(),
        Some(Number(7))
    );
    unsupported(
        session.get_revisioned(&address, "value", &Format),
        Capability::RevisionedRead,
    );
    unsupported(
        session.compare_exchange(&address, "new", None, &Number(1), &Format),
        Capability::CompareExchange,
    );
    let requests = [ReadRequest {
        address: &address,
        key: "value",
        schema: &schema,
    }];
    unsupported(
        session.read_many(&requests, ReadConsistency::AtomicWithinMount),
        Capability::AtomicReadMany,
    );
    unsupported(
        session.read_many(&requests, ReadConsistency::BestEffort),
        Capability::RevisionedRead,
    );
}
