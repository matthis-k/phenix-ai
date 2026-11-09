use phenix_storage::*;
use std::sync::{Arc, Barrier};

struct Text;
impl Codec for Text {
    type Value = String;
    fn schema(&self) -> Schema {
        Schema {
            name: "text".into(),
            revision: 1,
            structure: "utf8".into(),
            codec: "utf8".into(),
        }
    }
    fn encode(&self, value: &String) -> Result<Vec<u8>> {
        Ok(value.as_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> Result<String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| Error::Codec(e.to_string()))
    }
}
fn address() -> Address {
    Address::parse("store://main/items").unwrap()
}
fn session(registry: &Registry) -> Session {
    registry.session([Grant::new(
        Scope::Mount("main".into()),
        [
            Operation::Read,
            Operation::Write,
            Operation::Delete,
            Operation::Subscribe,
            Operation::SqlAdmin,
        ],
    )])
}
fn setup(sqlite: bool) -> (Registry, Session) {
    let registry = Registry::new();
    if sqlite {
        registry.mount_sqlite("main", ":memory:").unwrap();
    } else {
        registry.mount_memory("main").unwrap();
    }
    registry.register(&address(), Text.schema()).unwrap();
    let session = session(&registry);
    (registry, session)
}
fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "phenix-revisions-{label}-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn revisions_conflicts_aba_and_notices() {
    for sqlite in [false, true] {
        let (_registry, session) = setup(sqlite);
        let a = address();
        assert!(
            session
                .capabilities(&a, Operation::Read)
                .unwrap()
                .compare_exchange
        );
        assert!(session.get_revisioned(&a, "k", &Text).unwrap().is_none());
        let mut notices = session.subscribe(Scope::Dataset(a.clone()), 16).unwrap();
        let first = session
            .compare_exchange(&a, "k", None, &"a".into(), &Text)
            .unwrap();
        let found = session.get_revisioned(&a, "k", &Text).unwrap().unwrap();
        assert_eq!(
            found,
            Revisioned {
                value: "a".into(),
                revision: first.clone()
            }
        );
        notices.drain().unwrap();
        assert!(
            matches!(session.compare_exchange(&a, "k", None, &"b".into(), &Text), Err(Error::Conflict { current: Some(r) }) if r == first)
        );
        assert!(notices.drain().unwrap().is_empty());
        let second = session
            .compare_exchange(&a, "k", Some(&first), &"b".into(), &Text)
            .unwrap();
        assert_ne!(first, second);
        session.put(&a, "k", &"a".into(), &Text).unwrap();
        let third = session
            .get_revisioned(&a, "k", &Text)
            .unwrap()
            .unwrap()
            .revision;
        assert_ne!(first, third);
        notices.drain().unwrap();
        assert!(
            matches!(session.compare_exchange(&a, "k", Some(&first), &"stale".into(), &Text), Err(Error::Conflict { current: Some(r) }) if r == third)
        );
        assert!(notices.drain().unwrap().is_empty());
        assert!(session.delete(&a, "k").unwrap());
        notices.drain().unwrap();
        assert!(matches!(
            session.compare_exchange(&a, "k", Some(&third), &"stale".into(), &Text),
            Err(Error::Conflict { current: None })
        ));
        assert!(notices.drain().unwrap().is_empty());
        let recreated = session
            .compare_exchange(&a, "k", None, &"a".into(), &Text)
            .unwrap();
        assert_ne!(third, recreated);
        notices.drain().unwrap();
        assert!(
            matches!(session.compare_exchange(&a, "k", Some(&third), &"stale".into(), &Text), Err(Error::Conflict { current: Some(r) }) if r == recreated)
        );
        assert!(notices.drain().unwrap().is_empty());
    }
}

#[test]
fn concurrent_cas_has_one_winner_and_one_notice() {
    for sqlite in [false, true] {
        let (_registry, session) = setup(sqlite);
        for create in [true, false] {
            let key = if create { "create" } else { "replace" };
            let expected = if create {
                None
            } else {
                Some(
                    session
                        .compare_exchange(&address(), key, None, &"initial".into(), &Text)
                        .unwrap(),
                )
            };
            let barrier = Arc::new(Barrier::new(12));
            let mut notices = session.subscribe(Scope::Dataset(address()), 16).unwrap();
            let results = std::thread::scope(|scope| {
                let workers: Vec<_> = (0..12)
                    .map(|i| {
                        let session = &session;
                        let barrier = barrier.clone();
                        let expected = expected.clone();
                        scope.spawn(move || {
                            barrier.wait();
                            session.compare_exchange(
                                &address(),
                                key,
                                expected.as_ref(),
                                &i.to_string(),
                                &Text,
                            )
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|w| w.join().unwrap())
                    .collect::<Vec<_>>()
            });
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            let winner = session
                .get_revisioned(&address(), key, &Text)
                .unwrap()
                .unwrap()
                .revision;
            for result in results {
                match result {
                    Ok(r) => assert_eq!(r, winner),
                    Err(Error::Conflict { current }) => assert_eq!(current, Some(winner.clone())),
                    other => panic!("unexpected CAS result: {other:?}"),
                }
            }
            assert_eq!(notices.drain().unwrap().len(), 1);
        }
    }
}

#[test]
fn sqlite_migration_persistence_and_cross_connection_cas() {
    let path = path("migration");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE __phenix_kv (dataset TEXT NOT NULL, key TEXT NOT NULL, value BLOB NOT NULL, PRIMARY KEY(dataset, key)); INSERT INTO __phenix_kv VALUES ('items', 'legacy', X'61')").unwrap();
    }
    let mut previous = None;
    for _ in 0..3 {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
        let session = session(&registry);
        let found = session
            .get_revisioned(&address(), "legacy", &Text)
            .unwrap()
            .unwrap();
        assert_eq!(found.value, "a");
        if let Some(previous) = previous {
            assert_eq!(found.revision, previous);
        }
        session.delete(&address(), "legacy").unwrap();
        let revision = session
            .compare_exchange(&address(), "legacy", None, &"a".into(), &Text)
            .unwrap();
        assert_ne!(found.revision, revision);
        assert!(matches!(
            session.compare_exchange(
                &address(),
                "legacy",
                Some(&found.revision),
                &"bad".into(),
                &Text
            ),
            Err(Error::Conflict { .. })
        ));
        previous = Some(revision);
    }
    let registries = [Registry::new(), Registry::new()];
    for registry in &registries {
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
    }
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = registries
            .iter()
            .map(|registry| {
                let barrier = &barrier;
                let expected = previous.clone();
                scope.spawn(move || {
                    barrier.wait();
                    session(registry).compare_exchange(
                        &address(),
                        "legacy",
                        expected.as_ref(),
                        &"winner".into(),
                        &Text,
                    )
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::Conflict { current: Some(_) })))
            .count(),
        1
    );
    drop(registries);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn legacy_backend_revision_capability_is_explicitly_unsupported() {
    struct Legacy;
    impl Backend for Legacy {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            Some(self)
        }
    }
    impl TypedKv for Legacy {
        fn get(&mut self, _: &str, _: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }
        fn put(&mut self, _: &str, _: &str, _: Vec<u8>) -> Result<()> {
            panic!("CAS must not call put")
        }
        fn delete(&mut self, _: &str, _: &str) -> Result<bool> {
            Ok(false)
        }
    }
    let registry = Registry::new();
    registry.mount("main", Legacy).unwrap();
    registry.register(&address(), Text.schema()).unwrap();
    let session = session(&registry);
    let caps = session.capabilities(&address(), Operation::Read).unwrap();
    assert!(caps.typed_kv);
    assert!(!caps.compare_exchange);
    let mut notices = session.subscribe(Scope::Dataset(address()), 2).unwrap();
    assert!(matches!(
        session.get_revisioned(&address(), "k", &Text),
        Err(Error::UnsupportedCapability {
            capability: Capability::RevisionedRead
        })
    ));
    assert!(matches!(
        session.compare_exchange(&address(), "k", None, &"v".into(), &Text),
        Err(Error::UnsupportedCapability {
            capability: Capability::CompareExchange
        })
    ));
    assert!(notices.drain().unwrap().is_empty());
}

#[test]
fn revision_methods_check_authority_and_schema_before_codec() {
    struct Wrong;
    impl Codec for Wrong {
        type Value = String;
        fn schema(&self) -> Schema {
            let mut s = Text.schema();
            s.revision += 1;
            s
        }
        fn encode(&self, _: &String) -> Result<Vec<u8>> {
            panic!("wrong schema")
        }
        fn decode(&self, _: &[u8]) -> Result<String> {
            panic!("wrong schema")
        }
    }
    for sqlite in [false, true] {
        let (registry, full) = setup(sqlite);
        let denied = registry.session([]);
        assert!(matches!(
            denied.get_revisioned(&address(), "k", &Wrong),
            Err(Error::Denied)
        ));
        assert!(matches!(
            denied.compare_exchange(&address(), "k", None, &"v".into(), &Wrong),
            Err(Error::Denied)
        ));
        assert!(matches!(
            full.get_revisioned(&address(), "k", &Wrong),
            Err(Error::SchemaMismatch)
        ));
        assert!(matches!(
            full.compare_exchange(&address(), "k", None, &"v".into(), &Wrong),
            Err(Error::SchemaMismatch)
        ));
    }
}

#[test]
fn failed_encoding_and_sqlite_writes_do_not_publish_or_advance_revisions() {
    struct Failing;
    impl Codec for Failing {
        type Value = String;
        fn schema(&self) -> Schema {
            Text.schema()
        }
        fn encode(&self, _: &String) -> Result<Vec<u8>> {
            Err(Error::Codec("encode".into()))
        }
        fn decode(&self, _: &[u8]) -> Result<String> {
            Err(Error::Codec("decode".into()))
        }
    }
    for sqlite in [false, true] {
        let (_registry, session) = setup(sqlite);
        let first = session
            .compare_exchange(&address(), "k", None, &"a".into(), &Text)
            .unwrap();
        let mut notices = session.subscribe(Scope::Dataset(address()), 4).unwrap();
        assert!(matches!(
            session.compare_exchange(&address(), "k", Some(&first), &"b".into(), &Failing),
            Err(Error::Codec(_))
        ));
        assert!(matches!(
            session.get_revisioned(&address(), "k", &Failing),
            Err(Error::Codec(_))
        ));
        assert!(notices.drain().unwrap().is_empty());
        assert_eq!(
            session
                .get_revisioned(&address(), "k", &Text)
                .unwrap()
                .unwrap()
                .revision,
            first
        );
    }

    let path = path("write-failure");
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
        session(&registry)
            .compare_exchange(&address(), "k", None, &"a".into(), &Text)
            .unwrap();
    }
    // A trusted host fixture injects a backend statement failure after clock
    // allocation, proving that the clock and value roll back together.
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TRIGGER reject_update BEFORE UPDATE ON __phenix_kv BEGIN SELECT RAISE(ABORT, 'fixture failure'); END").unwrap();
    }
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
        let session = session(&registry);
        let first = session
            .get_revisioned(&address(), "k", &Text)
            .unwrap()
            .unwrap();
        let mut notices = session.subscribe(Scope::Dataset(address()), 4).unwrap();
        assert!(matches!(
            session.compare_exchange(&address(), "k", Some(&first.revision), &"b".into(), &Text),
            Err(Error::Sql(_))
        ));
        assert!(notices.drain().unwrap().is_empty());
        assert_eq!(
            session
                .get_revisioned(&address(), "k", &Text)
                .unwrap()
                .unwrap(),
            first
        );
    }
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT sequence FROM __phenix_revision_clock", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        connection.execute_batch("DROP TRIGGER reject_update; UPDATE __phenix_revision_clock SET sequence = 9223372036854775807").unwrap();
    }
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
        let session = session(&registry);
        let mut notices = session.subscribe(Scope::Dataset(address()), 4).unwrap();
        assert!(matches!(
            session.compare_exchange(&address(), "new", None, &"v".into(), &Text),
            Err(Error::RevisionExhausted)
        ));
        assert!(
            session
                .get_revisioned(&address(), "new", &Text)
                .unwrap()
                .is_none()
        );
        assert!(notices.drain().unwrap().is_empty());
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn missing_or_regressed_clock_fails_closed_without_reusing_tokens() {
    for damage in [
        "DROP TABLE __phenix_revision_clock",
        "DELETE FROM __phenix_revision_clock",
        "UPDATE __phenix_revision_clock SET sequence = 0",
    ] {
        let path = path("damaged-clock");
        {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path).unwrap();
            registry.register(&address(), Text.schema()).unwrap();
            session(&registry)
                .compare_exchange(&address(), "k", None, &"a".into(), &Text)
                .unwrap();
        }
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection.execute_batch(damage).unwrap();
        }
        let registry = Registry::new();
        assert!(
            matches!(
                registry.mount_sqlite("main", &path),
                Err(Error::SchemaMismatch)
            ),
            "{damage}"
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn cas_compares_after_encoding_and_does_not_hold_storage_locks_in_codec() {
    struct Racing<'a>(&'a Session);
    impl Codec for Racing<'_> {
        type Value = String;
        fn schema(&self) -> Schema {
            Text.schema()
        }
        fn encode(&self, value: &String) -> Result<Vec<u8>> {
            self.0.put(&address(), "k", &"concurrent".into(), &Text)?;
            Text.encode(value)
        }
        fn decode(&self, bytes: &[u8]) -> Result<String> {
            Text.decode(bytes)
        }
    }
    for sqlite in [false, true] {
        let (_registry, session) = setup(sqlite);
        let first = session
            .compare_exchange(&address(), "k", None, &"initial".into(), &Text)
            .unwrap();
        let mut notices = session.subscribe(Scope::Dataset(address()), 4).unwrap();
        let result = session.compare_exchange(
            &address(),
            "k",
            Some(&first),
            &"stale".into(),
            &Racing(&session),
        );
        let found = session
            .get_revisioned(&address(), "k", &Text)
            .unwrap()
            .unwrap();
        assert_eq!(found.value, "concurrent");
        assert!(matches!(result, Err(Error::Conflict { current: Some(r) }) if r == found.revision));
        assert_eq!(notices.drain().unwrap().len(), 1);
    }
}

#[test]
fn simultaneous_legacy_migration_and_registration_and_empty_reopen_preserve_clock() {
    let path = path("startup-race");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE __phenix_kv(dataset TEXT NOT NULL, key TEXT NOT NULL, value BLOB NOT NULL, PRIMARY KEY(dataset,key)); INSERT INTO __phenix_kv VALUES('items','legacy',X'61')").unwrap();
    }
    let barrier = Barrier::new(6);
    let registries = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    let registry = Registry::new();
                    barrier.wait();
                    registry.mount_sqlite("main", path).unwrap();
                    barrier.wait();
                    registry.register(&address(), Text.schema()).unwrap();
                    registry
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    let kv = session(&registries[0]);
    let stale = kv
        .compare_exchange(&address(), "k", None, &"a".into(), &Text)
        .unwrap();
    kv.delete(&address(), "k").unwrap();
    kv.delete(&address(), "legacy").unwrap();
    drop(kv);
    drop(registries);
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
        let session = session(&registry);
        let new = session
            .compare_exchange(&address(), "k", None, &"a".into(), &Text)
            .unwrap();
        assert_ne!(new, stale);
        assert!(
            matches!(session.compare_exchange(&address(), "k", Some(&stale), &"bad".into(), &Text), Err(Error::Conflict { current: Some(r) }) if r == new)
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn migration_blocks_derived_sql_and_rolls_back_all_schema_changes() {
    let path = path("migration-trigger");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE __phenix_kv(dataset TEXT NOT NULL, key TEXT NOT NULL, value BLOB NOT NULL, PRIMARY KEY(dataset,key)); INSERT INTO __phenix_kv VALUES('items','legacy',X'61'); CREATE TABLE __phenix_revision_clock(id INTEGER PRIMARY KEY, sequence INTEGER NOT NULL); CREATE TABLE stolen(value); CREATE TRIGGER migration_leak AFTER INSERT ON __phenix_revision_clock BEGIN INSERT INTO stolen SELECT value FROM __phenix_kv; END").unwrap();
    }
    let registry = Registry::new();
    assert!(registry.mount_sqlite("main", &path).is_err());
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(__phenix_kv)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(columns, ["dataset", "key", "value"]);
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name = '__phenix_schema'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM stolen", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM __phenix_revision_clock", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        // A trusted host can remove the incompatible trigger and retry cleanly.
        connection
            .execute_batch("DROP TRIGGER migration_leak")
            .unwrap();
    }
    registry.mount_sqlite("main", &path).unwrap();
    registry.register(&address(), Text.schema()).unwrap();
    assert_eq!(
        session(&registry)
            .get_revisioned(&address(), "legacy", &Text)
            .unwrap()
            .unwrap()
            .value,
        "a"
    );
    drop(registry);
    std::fs::remove_file(path).unwrap();
}
