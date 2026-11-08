use phenix_storage::{scoped::*, snapshot::*, *};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, PartialEq)]
struct Number(u64);
#[derive(Default)]
struct Format;
thread_local! {
    static DECODE_HOOK: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}
impl Stored for Number {
    type Format = Format;
}
impl Codec for Format {
    type Value = Number;
    fn schema(&self) -> Schema {
        Schema {
            name: "test.number".into(),
            revision: 1,
            structure: "u64".into(),
            codec: "be".into(),
        }
    }
    fn encode(&self, v: &Number) -> phenix_storage::Result<Vec<u8>> {
        Ok(v.0.to_be_bytes().to_vec())
    }
    fn decode(&self, b: &[u8]) -> phenix_storage::Result<Number> {
        DECODE_HOOK.with(|hook| {
            if let Some(hook) = &*hook.borrow() {
                hook();
            }
        });
        Ok(Number(u64::from_be_bytes(b.try_into().map_err(|_| {
            phenix_storage::Error::Codec("length".into())
        })?)))
    }
}
fn addr(mount: &str, dataset: &str) -> Address {
    Address::parse(&format!("store://{mount}/{dataset}")).unwrap()
}
fn grant(a: &Address, ops: impl IntoIterator<Item = Operation>) -> Grant {
    Grant::new(Scope::Dataset(a.clone()), ops)
}
fn request<'a>(address: &'a Address, key: &'a str, schema: &'a Schema) -> ReadRequest<'a> {
    ReadRequest {
        address,
        key,
        schema,
    }
}

#[test]
fn ordered_duplicates_absence_revisions_and_no_notices() {
    for sqlite in [false, true] {
        let r = Registry::new();
        if sqlite {
            r.mount_sqlite("main", ":memory:").unwrap();
        } else {
            r.mount_memory("main").unwrap();
        }
        let a = addr("main", "a");
        let b = addr("main", "b");
        let schema = Format.schema();
        for address in [&a, &b] {
            r.register(address, schema.clone()).unwrap();
        }
        let s = r.session([Grant::new(
            Scope::Mount("main".into()),
            [
                Operation::Read,
                Operation::Write,
                Operation::Subscribe,
                Operation::SqlAdmin,
            ],
        )]);
        s.put(&a, "value", &Number(3), &Format).unwrap();
        let mut notices = s.subscribe(Scope::Mount("main".into()), 8).unwrap();
        let batch = [
            request(&a, "value", &schema),
            request(&b, "missing", &schema),
            request(&a, "value", &schema),
        ];
        let snapshot = s
            .read_many(&batch, ReadConsistency::AtomicWithinMount)
            .unwrap();
        let e = snapshot.entries();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].address(), &a);
        assert_eq!(e[1].key(), "missing");
        assert!(e[1].value().is_none());
        assert_eq!(e[0].value(), e[2].value());
        assert_eq!(
            e[0].value().unwrap().revision,
            s.get_revisioned(&a, "value", &Format)
                .unwrap()
                .unwrap()
                .revision
        );
        assert!(
            s.read_many(&[], ReadConsistency::AtomicWithinMount)
                .unwrap()
                .entries()
                .is_empty()
        );
        let mut bad = schema.clone();
        bad.codec = "wrong".into();
        assert!(matches!(
            s.read_many(
                &[request(&a, "value", &schema), request(&b, "value", &bad)],
                ReadConsistency::AtomicWithinMount
            ),
            Err(phenix_storage::Error::SchemaMismatch)
        ));
        if sqlite {
            s.transaction(&a, |_| {
                assert!(matches!(
                    s.read_many(&batch, ReadConsistency::AtomicWithinMount),
                    Err(phenix_storage::Error::Reentrant)
                ));
                Ok(())
            })
            .unwrap();
        }
        assert!(notices.drain().unwrap().is_empty());
    }
}

struct Probe(Arc<AtomicUsize>, MemoryBackend);
impl Backend for Probe {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(&mut self.1)
    }
}
#[test]
fn all_grants_then_all_schemas_before_backend_io_and_mixed_mount_reject() {
    let r = Registry::new();
    let calls = Arc::new(AtomicUsize::new(0));
    r.mount("main", Probe(calls.clone(), MemoryBackend::default()))
        .unwrap();
    r.mount_memory("other").unwrap();
    let a = addr("main", "a");
    let b = addr("main", "b");
    let missing = addr("missing", "unknown");
    let other = addr("other", "a");
    let schema = Format.schema();
    for x in [&a, &b, &other] {
        r.register(x, schema.clone()).unwrap();
    }
    let denied = r.session([grant(&missing, [Operation::Read])]);
    assert!(matches!(
        denied.read_many(
            &[request(&missing, "v", &schema), request(&a, "v", &schema)],
            ReadConsistency::AtomicWithinMount
        ),
        Err(phenix_storage::Error::Denied)
    ));
    let s = r.session([
        grant(&a, [Operation::Read]),
        grant(&b, [Operation::Read]),
        grant(&other, [Operation::Read]),
    ]);
    let mut bad = schema.clone();
    bad.revision += 1;
    assert!(
        s.read_many(
            &[request(&a, "v", &schema), request(&b, "v", &bad)],
            ReadConsistency::AtomicWithinMount
        )
        .is_err()
    );
    assert!(matches!(
        s.read_many(
            &[request(&a, "v", &schema), request(&other, "v", &schema)],
            ReadConsistency::AtomicWithinMount
        ),
        Err(phenix_storage::Error::MixedSnapshotSources)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.read_many(
            &[request(&other, "v", &schema), request(&a, "v", &schema)],
            ReadConsistency::BestEffort
        )
        .unwrap()
        .consistency(),
        ReadConsistency::BestEffort
    );
}

struct Legacy(MemoryBackend);
impl Backend for Legacy {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(self)
    }
}
impl TypedKv for Legacy {
    fn get(&mut self, d: &str, k: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
        self.0.get(d, k)
    }
    fn put(&mut self, d: &str, k: &str, v: Vec<u8>) -> phenix_storage::Result<()> {
        self.0.put(d, k, v)
    }
    fn delete(&mut self, d: &str, k: &str) -> phenix_storage::Result<bool> {
        self.0.delete(d, k)
    }
    fn get_revisioned(
        &mut self,
        d: &str,
        k: &str,
    ) -> phenix_storage::Result<Option<Revisioned<Vec<u8>>>> {
        self.0.get_revisioned(d, k)
    }
}
#[test]
fn external_backend_must_opt_in_to_atomic_reads() {
    let r = Registry::new();
    r.mount("main", Legacy(MemoryBackend::default())).unwrap();
    let a = addr("main", "a");
    let schema = Format.schema();
    r.register(&a, schema.clone()).unwrap();
    let s = r.session([grant(&a, [Operation::Read])]);
    let keys = [request(&a, "v", &schema)];
    assert!(matches!(
        s.read_many(&keys, ReadConsistency::AtomicWithinMount),
        Err(phenix_storage::Error::UnsupportedCapability {
            capability: Capability::AtomicReadMany
        })
    ));
    s.read_many(&keys, ReadConsistency::BestEffort).unwrap();
    let identity = Identity::user("u").unwrap().project("p").unwrap();
    let mut catalog = ResourceCatalog::new();
    catalog
        .bind(A.resource_spec().resolve(&identity).unwrap(), a)
        .unwrap();
    let storage = catalog.freeze().access(s, identity);
    let (cell,) = storage.bind((A.read(),)).unwrap();
    assert!(matches!(
        SnapshotReader::new(vec![cell.source()], ReadConsistency::AtomicWithinMount),
        Err(phenix_storage::Error::UnsupportedCapability {
            capability: Capability::AtomicReadMany
        })
    ));
    SnapshotReader::new(vec![cell.source()], ReadConsistency::BestEffort)
        .unwrap()
        .read()
        .unwrap();
}

const A: CellSpec<Number> = CellSpec::project_state("test.values/a");
const B: CellSpec<Number> = CellSpec::project_state("test.values/b");
struct Wrong;
#[derive(Default)]
struct WrongFormat;
impl Stored for Wrong {
    type Format = WrongFormat;
}
impl Codec for WrongFormat {
    type Value = Wrong;
    fn schema(&self) -> Schema {
        let mut s = Format.schema();
        s.codec = "wrong".into();
        s
    }
    fn encode(&self, _: &Wrong) -> phenix_storage::Result<Vec<u8>> {
        panic!("must not encode")
    }
    fn decode(&self, _: &[u8]) -> phenix_storage::Result<Wrong> {
        panic!("must not decode")
    }
}
#[test]
fn cells_pin_scope_authority_schema_optional_and_independent_sessions() {
    let r = Registry::new();
    r.mount_memory("main").unwrap();
    let a = addr("main", "a");
    let b = addr("main", "b");
    r.register(&a, Format.schema()).unwrap();
    r.register(&b, Format.schema()).unwrap();
    let identity = Identity::user("u").unwrap().project("p").unwrap();
    let mut catalog = ResourceCatalog::new();
    catalog
        .bind(A.resource_spec().resolve(&identity).unwrap(), a.clone())
        .unwrap();
    catalog
        .bind(B.resource_spec().resolve(&identity).unwrap(), b.clone())
        .unwrap();
    assert!(matches!(
        catalog.bind(
            ResourceId::new(
                ScopeId::Project("q".into()),
                StorageClass::State,
                "test.values/a"
            )
            .unwrap(),
            a.clone()
        ),
        Err(phenix_storage::scoped::Error::PhysicalAlias)
    ));
    let bindings = catalog.freeze();
    let sa = bindings.access(r.session([grant(&a, [Operation::Read])]), identity.clone());
    let sb = bindings.access(r.session([grant(&b, [Operation::Read])]), identity.clone());
    let (ca,) = sa.bind((A.read(),)).unwrap();
    let (cb,) = sb.bind((B.read(),)).unwrap();
    sa.bind((A.read(), A.read())).unwrap();
    sa.bind((A.read(), A.read(), A.read())).unwrap();
    sa.bind((A.read(), A.read(), A.read(), A.read())).unwrap();
    let wrong: CellSpec<Wrong> = CellSpec::project_state("test.values/a");
    assert!(matches!(
        sa.bind((wrong.read(),)),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::SchemaMismatch
        ))
    ));
    let _clone = ca.clone(); // Neither Number nor Format implements Clone.
    assert!(matches!(
        sa.bind((B.read(),)),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
    let reader = SnapshotReader::new(
        vec![ca.source(), cb.source(), ca.source()],
        ReadConsistency::AtomicWithinMount,
    )
    .unwrap();
    let snapshot = reader.read().unwrap();
    assert!(matches!(
        ca.decode(&snapshot.entries()[0]),
        Err(phenix_storage::Error::MissingInput { .. })
    ));
    assert!(
        ca.optional()
            .decode(&snapshot.entries()[0])
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        cb.decode(&snapshot.entries()[0]),
        Err(phenix_storage::Error::SnapshotEntryMismatch)
    ));
    assert!(ca.changed(1).is_err());
    let writer = r.session([grant(&a, [Operation::Write])]);
    writer.put(&a, "value", &Number(7), &Format).unwrap();
    let reentrant = r.session([grant(&a, [Operation::Read])]);
    let reentrant_address = a.clone();
    DECODE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            reentrant
                .capabilities(&reentrant_address, Operation::Read)
                .unwrap();
        }))
    });
    assert_eq!(ca.get().unwrap().value, Number(7));
    DECODE_HOOK.with(|hook| *hook.borrow_mut() = None);
    let other_scope = bindings.access(
        r.session([grant(&a, [Operation::Read])]),
        Identity::user("u").unwrap().project("q").unwrap(),
    );
    assert!(matches!(
        other_scope.bind((A.read(),)),
        Err(phenix_storage::scoped::Error::UnknownResource)
    ));
    let no_project = bindings.access(r.session([]), Identity::user("u").unwrap());
    assert!(matches!(
        no_project.bind((A.read(),)),
        Err(phenix_storage::scoped::Error::MissingProject)
    ));
    let foreign = Registry::new();
    foreign.mount_memory("main").unwrap();
    foreign.register(&a, Format.schema()).unwrap();
    let other = bindings
        .access(foreign.session([grant(&a, [Operation::Read])]), identity)
        .bind((A.read(),))
        .unwrap()
        .0;
    assert!(matches!(
        SnapshotReader::new(
            vec![ca.source(), other.source()],
            ReadConsistency::AtomicWithinMount
        ),
        Err(phenix_storage::Error::MixedSnapshotSources)
    ));
    SnapshotReader::new(
        vec![ca.source(), other.source()],
        ReadConsistency::BestEffort,
    )
    .unwrap()
    .read()
    .unwrap();
}

#[test]
fn sqlite_external_transactions_never_tear_and_uncommitted_rollback_is_invisible() {
    let path = std::env::temp_dir().join(format!(
        "phenix-snapshot-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let r = Registry::new();
    r.mount_sqlite("main", &path).unwrap();
    let a = addr("main", "a");
    let schema = Format.schema();
    r.register(&a, schema.clone()).unwrap();
    let s = r.session([grant(
        &a,
        [Operation::Read, Operation::Write, Operation::Subscribe],
    )]);
    for key in ["a", "b"] {
        s.put(&a, key, &Number(0), &Format).unwrap();
    }
    let mut notices = s.subscribe(Scope::Dataset(a.clone()), 4).unwrap();
    let mut raw = rusqlite::Connection::open(&path).unwrap();
    raw.pragma_update(None, "journal_mode", "WAL").unwrap();
    let batch: Vec<_> = (0..1000)
        .flat_map(|_| [request(&a, "a", &schema), request(&a, "b", &schema)])
        .collect();
    let tx = raw.transaction().unwrap();
    tx.execute(
        "UPDATE __phenix_kv SET value = ?1",
        [99u64.to_be_bytes().as_slice()],
    )
    .unwrap();
    let snapshot = s
        .read_many(&batch, ReadConsistency::AtomicWithinMount)
        .unwrap();
    assert!(
        snapshot
            .entries()
            .iter()
            .all(|e| e.value().unwrap().value == 0u64.to_be_bytes())
    );
    tx.rollback().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let start = barrier.clone();
    let writer = std::thread::spawn(move || {
        raw.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        start.wait();
        for value in 1u64..=150 {
            let tx = raw.transaction().unwrap();
            for key in ["a", "b"] {
                tx.execute(
                    "UPDATE __phenix_kv SET value = ?1, revision = ?2 WHERE key = ?3",
                    rusqlite::params![value.to_be_bytes().as_slice(), value as i64 + 2, key],
                )
                .unwrap();
                std::thread::yield_now();
            }
            tx.execute(
                "UPDATE __phenix_revision_clock SET sequence = ?1",
                [value as i64 + 2],
            )
            .unwrap();
            tx.commit().unwrap();
        }
    });
    barrier.wait();
    for _ in 0..30 {
        let snapshot = s
            .read_many(&batch, ReadConsistency::AtomicWithinMount)
            .unwrap();
        let first = &snapshot.entries()[0].value().unwrap().value;
        assert!(
            snapshot
                .entries()
                .iter()
                .all(|e| &e.value().unwrap().value == first)
        );
    }
    writer.join().unwrap();
    assert!(notices.drain().unwrap().is_empty());
    drop(s);
    drop(r);
    std::fs::remove_file(&path).unwrap();
}
