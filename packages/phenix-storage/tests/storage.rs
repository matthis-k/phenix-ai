use phenix_storage::*;

#[test]
fn external_optional_backend_is_object_safe() {
    struct External(MemoryBackend);
    impl Backend for External {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            Some(&mut self.0)
        }
    }
    struct Empty;
    impl Backend for Empty {}
    let registry = Registry::new();
    registry
        .mount("main", External(MemoryBackend::default()))
        .unwrap();
    registry.register(&address(), Text.schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::Read, Operation::Write],
    )]);
    session
        .put(&address(), "key", &"external".into(), &Text)
        .unwrap();
    assert_eq!(
        session.get(&address(), "key", &Text).unwrap(),
        Some("external".into())
    );
    registry.mount("empty", Empty).unwrap();
    let empty = Address::parse("store://empty/items").unwrap();
    registry.register(&empty, Text.schema()).unwrap();
    let session = registry.session([Grant::new(Scope::Mount("empty".into()), [Operation::Read])]);
    assert!(
        !session
            .capabilities(&empty, Operation::Read)
            .unwrap()
            .typed_kv
    );
    assert!(matches!(
        session.get(&empty, "key", &Text),
        Err(Error::UnsupportedCapability { .. })
    ));
}

#[test]
fn per_mount_locking_and_reentrant_error() {
    let (registry, session, a) = setup(true);
    registry.mount_memory("other").unwrap();
    let other = Address::parse("store://other/items").unwrap();
    registry.register(&other, Text.schema()).unwrap();
    let memory = registry.session([Grant::new(
        Scope::Mount("other".into()),
        [Operation::Read, Operation::Write],
    )]);
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        session
            .transaction(&a, |_| {
                assert!(matches!(
                    session.transaction(&a, |_| Ok(())),
                    Err(Error::Reentrant)
                ));
                assert!(matches!(
                    session.capabilities(&a, Operation::Sql),
                    Err(Error::Reentrant)
                ));
                assert!(matches!(
                    registry.register(&a, Text.schema()),
                    Err(Error::Reentrant)
                ));
                entered.send(()).unwrap();
                released
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Ok(())
            })
            .unwrap();
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    memory
        .put(&other, "key", &"independent".into(), &Text)
        .unwrap();
    assert_eq!(
        memory.get(&other, "key", &Text).unwrap(),
        Some("independent".into())
    );
    release.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn granular_sql_authorizer_and_incremental_rows() {
    let (registry, admin, a) = setup(true);
    admin
        .transaction(&a, |sql| {
            sql.execute("CREATE TABLE t (x INTEGER)", &[])?;
            sql.execute("CREATE TABLE audit (x INTEGER)", &[])?;
            sql.execute(
                "CREATE TRIGGER log AFTER INSERT ON t BEGIN INSERT INTO audit VALUES (new.x); END",
                &[],
            )?;
            Ok(())
        })
        .unwrap();
    let read = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlRead],
    )]);
    let write = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlWrite],
    )]);
    for denied in [
        "INSERT INTO t VALUES (1)",
        "DELETE FROM t",
        "UPDATE t SET x = 2",
        "CREATE TABLE forbidden (x)",
        "DROP TABLE t",
        "PRAGMA user_version = 2",
        "ATTACH DATABASE ':memory:' AS escape",
        "SAVEPOINT escape",
        "CREATE TEMP TABLE forbidden (x)",
    ] {
        assert!(
            read.transaction(&a, |sql| sql.execute(denied, &[]))
                .is_err(),
            "{denied}"
        );
    }
    for denied in [
        "CREATE TABLE forbidden (x)",
        "DROP TABLE t",
        "CREATE INDEX forbidden ON t(x)",
        "PRAGMA foreign_keys = OFF",
    ] {
        assert!(
            write
                .transaction(&a, |sql| sql.execute(denied, &[]))
                .is_err(),
            "{denied}"
        );
    }
    assert!(
        read.transaction(&a, |sql| sql.returning(
            "INSERT INTO t VALUES (1) RETURNING x",
            &[],
            |_| Ok(())
        ))
        .is_err()
    );
    let mut returned = Vec::new();
    write
        .transaction(&a, |sql| {
            sql.returning(
                "INSERT INTO t VALUES (1), (2), (3) RETURNING x",
                &[],
                |row| {
                    returned.push(row.get::<i64>(0)?);
                    Ok(())
                },
            )
        })
        .unwrap();
    assert_eq!(returned, vec![1, 2, 3]);
    let mut visited = 0;
    let result = read.transaction(&a, |sql| {
        sql.query_each("SELECT x FROM audit", &[], |_| {
            visited += 1;
            Err(Error::Codec("stop".into()))
        })
    });
    assert!(matches!(result, Err(Error::Codec(_))));
    assert_eq!(visited, 1);
    let result = write.transaction(&a, |sql| {
        sql.returning("INSERT INTO t VALUES (4) RETURNING x", &[], |_| {
            Err(Error::Codec("rollback".into()))
        })
    });
    assert!(result.is_err());
    assert_eq!(
        read.transaction(&a, |sql| sql
            .query("SELECT count(*) FROM t", &[], |row| row.get::<i64>(0)))
            .unwrap(),
        vec![3]
    );
    let admin_only = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlAdmin],
    )]);
    admin_only
        .transaction(&a, |sql| sql.execute("CREATE INDEX allowed ON t(x)", &[]))
        .unwrap();
    let narrow = registry.session([Grant::new(
        Scope::Dataset(a.clone()),
        [Operation::SqlRead, Operation::SqlWrite, Operation::SqlAdmin],
    )]);
    assert!(matches!(
        narrow.transaction(&a, |_| Ok(())),
        Err(Error::Denied)
    ));
}

#[test]
fn sqlite_persists_across_registry_reopen() {
    let path = std::env::temp_dir().join(format!(
        "phenix-storage-persistence-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for first in [true, false] {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Text.schema()).unwrap();
        let session = registry.session([Grant::new(Scope::Mount("main".into()), [Operation::Sql])]);
        session
            .transaction(&address(), |sql| {
                if first {
                    sql.execute("CREATE TABLE persisted (value TEXT)", &[])?;
                    sql.execute("INSERT INTO persisted VALUES (?)", &[&"durable"])?;
                }
                assert_eq!(
                    sql.query("SELECT value FROM persisted", &[], |row| row
                        .get::<String>(0))?,
                    vec!["durable"]
                );
                Ok(())
            })
            .unwrap();
    }
    std::fs::remove_file(path).unwrap();
}

struct Text;
impl Codec for Text {
    type Value = String;
    fn schema(&self) -> Schema {
        Schema {
            name: "text".into(),
            revision: 1,
            structure: "utf8-string".into(),
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
fn setup(sql: bool) -> (Registry, Session, Address) {
    let registry = Registry::new();
    if sql {
        registry.mount_sqlite("main", ":memory:").unwrap();
    } else {
        registry.mount_memory("main").unwrap();
    }
    let a = address();
    registry.register(&a, Text.schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [
            Operation::Read,
            Operation::Write,
            Operation::Delete,
            Operation::Subscribe,
            Operation::Sql,
        ],
    )]);
    (registry, session, a)
}

#[test]
fn strict_addresses() {
    for invalid in [
        "",
        "main/items",
        "store://main",
        "store://main/",
        "store:///items",
        "STORE://main/items",
        "store://Main/items",
        "store://main/..",
        "store://main/./items",
        "store://main/a/b",
        "store://main/%61",
        "store://main/a%2fb",
        "store://main/a?x",
        "store://main/a#x",
        "store://main/a\\b",
        "store://main/é",
        "store://main/ a",
        "store://u@main/a",
        "store://main:3/a",
    ] {
        assert!(
            matches!(Address::parse(invalid), Err(Error::InvalidAddress)),
            "{invalid}"
        );
    }
    let a = Address::parse("store://main-1/items_2").unwrap();
    assert_eq!(a.to_string(), "store://main-1/items_2");
}

#[test]
fn registry_duplicates_and_missing() {
    let (registry, session, a) = setup(false);
    assert!(matches!(
        registry.mount_memory("main"),
        Err(Error::DuplicateMount)
    ));
    assert!(matches!(
        registry.register(&a, Text.schema()),
        Err(Error::DuplicateDataset)
    ));
    assert!(matches!(
        registry.mount_memory("../bad"),
        Err(Error::InvalidAddress)
    ));
    let missing = Address::parse("store://main/missing").unwrap();
    assert!(matches!(
        session.get(&missing, "x", &Text),
        Err(Error::UnknownDataset)
    ));
    let missing = Address::parse("store://other/items").unwrap();
    assert!(matches!(
        registry.register(&missing, Text.schema()),
        Err(Error::UnknownMount)
    ));
}

#[test]
fn exact_grants_and_no_authority_leak() {
    let (registry, full, a) = setup(false);
    let other = Address::parse("store://main/other").unwrap();
    registry.register(&other, Text.schema()).unwrap();
    let read = registry.session([Grant::new(Scope::Dataset(a.clone()), [Operation::Read])]);
    assert!(read.get(&a, "x", &Text).unwrap().is_none());
    assert!(matches!(read.get(&other, "x", &Text), Err(Error::Denied)));
    assert!(matches!(
        read.put(&a, "x", &"x".into(), &Text),
        Err(Error::Denied)
    ));
    assert!(matches!(read.delete(&a, "x"), Err(Error::Denied)));
    assert!(matches!(
        read.subscribe(Scope::Dataset(a.clone()), 2),
        Err(Error::Denied)
    ));
    assert!(matches!(
        read.capabilities(&a, Operation::Sql),
        Err(Error::Denied)
    ));
    assert!(full.capabilities(&a, Operation::Read).unwrap().typed_kv);
}

#[test]
fn memory_roundtrip_delete_and_isolation() {
    let (registry, session, a) = setup(false);
    let b = Address::parse("store://main/other").unwrap();
    registry.register(&b, Text.schema()).unwrap();
    session
        .put(&a, "../%arbitrary-key", &"hello".into(), &Text)
        .unwrap();
    assert_eq!(
        session.get(&a, "../%arbitrary-key", &Text).unwrap(),
        Some("hello".into())
    );
    assert_eq!(session.get(&b, "../%arbitrary-key", &Text).unwrap(), None);
    assert!(session.delete(&a, "../%arbitrary-key").unwrap());
    assert!(!session.delete(&a, "../%arbitrary-key").unwrap());
    assert!(matches!(
        session.transaction(&a, |_| Ok(())),
        Err(Error::UnsupportedCapability {
            capability: Capability::Sql
        })
    ));
}

struct Wrong(Schema);
impl Codec for Wrong {
    type Value = String;
    fn schema(&self) -> Schema {
        self.0.clone()
    }
    fn encode(&self, _: &String) -> Result<Vec<u8>> {
        panic!("schema must be checked first")
    }
    fn decode(&self, _: &[u8]) -> Result<String> {
        panic!("schema must be checked first")
    }
}
#[test]
fn every_schema_field_is_authoritative() {
    let (_registry, session, a) = setup(false);
    session.put(&a, "k", &"v".into(), &Text).unwrap();
    for field in 0..4 {
        let mut schema = Text.schema();
        match field {
            0 => schema.name.push('x'),
            1 => schema.revision += 1,
            2 => schema.structure.push('x'),
            _ => schema.codec.push('x'),
        }
        assert!(matches!(
            session.get(&a, "k", &Wrong(schema.clone())),
            Err(Error::SchemaMismatch)
        ));
        assert!(matches!(
            session.put(&a, "k", &"v".into(), &Wrong(schema)),
            Err(Error::SchemaMismatch)
        ));
    }
}

mod producer {
    use phenix_storage::*;
    pub struct Value(pub String);
    pub struct Utf8;
    impl Codec for Utf8 {
        type Value = Value;
        fn schema(&self) -> Schema {
            Schema {
                name: "text".into(),
                revision: 1,
                structure: "utf8-string".into(),
                codec: "utf8".into(),
            }
        }
        fn encode(&self, value: &Value) -> Result<Vec<u8>> {
            Ok(value.0.as_bytes().to_vec())
        }
        fn decode(&self, bytes: &[u8]) -> Result<Value> {
            String::from_utf8(bytes.to_vec())
                .map(Value)
                .map_err(|e| Error::Codec(e.to_string()))
        }
    }
}

mod consumer {
    use phenix_storage::*;
    #[derive(Debug, PartialEq)]
    pub struct Value(pub Vec<u8>);
    pub struct Utf8;
    impl Codec for Utf8 {
        type Value = Value;
        fn schema(&self) -> Schema {
            Schema {
                name: "text".into(),
                revision: 1,
                structure: "utf8-string".into(),
                codec: "utf8".into(),
            }
        }
        fn encode(&self, value: &Value) -> Result<Vec<u8>> {
            std::str::from_utf8(&value.0).map_err(|e| Error::Codec(e.to_string()))?;
            Ok(value.0.clone())
        }
        fn decode(&self, bytes: &[u8]) -> Result<Value> {
            std::str::from_utf8(bytes).map_err(|e| Error::Codec(e.to_string()))?;
            Ok(Value(bytes.to_vec()))
        }
    }
}

#[test]
fn independent_types_interoperate_through_matching_schemas() {
    let (_registry, session, a) = setup(false);
    session
        .put(&a, "k", &producer::Value("hello".into()), &producer::Utf8)
        .unwrap();
    assert_eq!(
        session.get(&a, "k", &consumer::Utf8).unwrap(),
        Some(consumer::Value(b"hello".to_vec()))
    );
    session
        .put(
            &a,
            "k",
            &consumer::Value(b"world".to_vec()),
            &consumer::Utf8,
        )
        .unwrap();
    assert_eq!(
        session.get(&a, "k", &producer::Utf8).unwrap().unwrap().0,
        "world"
    );
}

#[test]
fn subscription_drop_and_drain_inside_transaction_callback() {
    for panics in [false, true] {
        let (done, finished) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (_registry, session, a) = setup(true);
            let mut observer = session.subscribe(Scope::Dataset(a.clone()), 2).unwrap();
            let sub = session.subscribe(Scope::Dataset(a.clone()), 1).unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.transaction(&a, |sql| {
                    // Both operations must work while the registry lock is held.
                    assert!(observer.drain()?.is_empty());
                    sql.execute("CREATE TABLE dropped (x)", &[])?;
                    if panics {
                        let _sub = sub;
                        panic!("drop subscription during unwind");
                    }
                    drop(sub);
                    Ok(())
                })
            }));
            if panics {
                assert!(result.is_err());
                assert!(observer.drain().unwrap().is_empty());
            } else {
                result.unwrap().unwrap();
                assert_eq!(observer.drain().unwrap().len(), 1);
            }
            assert!(session.capabilities(&a, Operation::Sql).unwrap().sql);
            done.send(()).unwrap();
        });
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("subscription operation deadlocked inside transaction");
        worker.join().unwrap();
    }
}

#[test]
fn panicking_waker_does_not_poison_storage_or_skip_subscribers() {
    struct PanicWake;
    impl std::task::Wake for PanicWake {
        fn wake(self: std::sync::Arc<Self>) {
            panic!("subscriber panic");
        }
    }
    for sql in [false, true] {
        let (_registry, session, a) = setup(sql);
        let mut first = session.subscribe(Scope::Dataset(a.clone()), 4).unwrap();
        let mut second = session.subscribe(Scope::Dataset(a.clone()), 4).unwrap();
        let waker = std::task::Waker::from(std::sync::Arc::new(PanicWake));
        first.set_waker(waker.clone()).unwrap();
        for _ in 0..2 {
            if sql {
                session
                    .transaction(&a, |sql| {
                        sql.execute("CREATE TABLE IF NOT EXISTS wake_test (x)", &[])?;
                        sql.execute("INSERT INTO wake_test VALUES (1)", &[])?;
                        Ok(())
                    })
                    .unwrap();
            } else {
                session.put(&a, "k", &"v".into(), &Text).unwrap();
                assert_eq!(session.get(&a, "k", &Text).unwrap(), Some("v".into()));
            }
            // Registration with an already-ready queue also invokes the waker.
            first.set_waker(waker.clone()).unwrap();
            let notices = first.drain().unwrap();
            assert_eq!(notices.len(), 1);
            assert_eq!(notices, second.drain().unwrap());
        }
        if !sql {
            assert!(session.delete(&a, "k").unwrap());
            let notices = first.drain().unwrap();
            assert_eq!(notices.len(), 1);
            assert_eq!(notices, second.drain().unwrap());
        }
        session.capabilities(&a, Operation::Subscribe).unwrap();
    }
}

#[test]
fn mount_coverage_kv_invalidates_other_datasets_without_key_precision() {
    #[derive(Default)]
    struct CoarseKv(MemoryBackend);
    impl Backend for CoarseKv {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            Some(&mut self.0)
        }
        // Use the default Mount observation coverage.
    }
    let registry = Registry::new();
    registry.mount("main", CoarseKv::default()).unwrap();
    let a = Address::parse("store://main/items").unwrap();
    let b = Address::parse("store://main/other").unwrap();
    registry.register(&a, Text.schema()).unwrap();
    registry.register(&b, Text.schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::Write, Operation::Delete, Operation::Subscribe],
    )]);
    let mut dataset = session.subscribe(Scope::Dataset(b), 4).unwrap();
    let mut mount = session.subscribe(Scope::Mount("main".into()), 4).unwrap();
    session.put(&a, "k", &"v".into(), &Text).unwrap();
    assert!(session.delete(&a, "k").unwrap());
    assert!(!session.delete(&a, "missing").unwrap());
    let notices = dataset.drain().unwrap();
    assert_eq!(notices.len(), 2);
    assert_eq!(notices, mount.drain().unwrap());
    for (index, notice) in notices.iter().enumerate() {
        let Notice::Changed(event) = notice else {
            panic!("expected change")
        };
        assert_eq!(event.scope, Scope::Mount("main".into()));
        assert_eq!(event.key, None);
        assert_eq!(event.observation.coverage, ObservationCoverage::Mount);
        assert_eq!(event.sequence, index as u64 + 1);
    }
}

#[test]
fn subscription_order_overflow_scope_and_raii() {
    let (registry, session, a) = setup(false);
    let b = Address::parse("store://main/other").unwrap();
    registry.register(&b, Text.schema()).unwrap();
    assert!(matches!(
        session.subscribe(Scope::Dataset(a.clone()), 0),
        Err(Error::InvalidCapacity)
    ));
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 2).unwrap();
    session.put(&b, "x", &"v".into(), &Text).unwrap();
    assert!(sub.drain().unwrap().is_empty());
    for key in ["a", "b"] {
        session.put(&a, key, &"v".into(), &Text).unwrap();
    }
    let events = sub.drain().unwrap();
    assert!(
        matches!(&events[..], [Notice::Changed(x), Notice::Changed(y)] if x.sequence < y.sequence && x.key.as_deref() == Some("a") && y.key.as_deref() == Some("b"))
    );
    let Notice::Changed(event) = &events[0] else {
        unreachable!()
    };
    assert_eq!(
        event.observation,
        session
            .capabilities(&a, Operation::Read)
            .unwrap()
            .observation
    );
    assert_eq!(event.observation.source, ObservationSource::RegistryWrites);
    assert_eq!(event.observation.coverage, ObservationCoverage::Key);
    assert_eq!(
        event.observation.delivery,
        ObservationDelivery::BoundedNonDurableWithOverflow
    );
    for _ in 0..4 {
        session.put(&a, "k", &"v".into(), &Text).unwrap();
    }
    assert!(matches!(
        &sub.drain().unwrap()[..],
        [Notice::Overflow { through: 7 }]
    ));
    assert!(sub.drain().unwrap().is_empty());
    session.delete(&a, "k").unwrap();
    assert_eq!(sub.drain().unwrap().len(), 1);
    drop(sub);
    // A dropped registry/session does not keep a subscription's adapter alive.
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 1).unwrap();
    session
        .put(&a, "retained", &"queued".into(), &Text)
        .unwrap();
    drop(session);
    drop(registry);
    assert!(matches!(sub.drain(), Err(Error::SubscriptionClosed)));
}

#[test]
fn sqlite_native_binding_rows_commit_rollback() {
    let (_registry, session, a) = setup(true);
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 8).unwrap();
    session.transaction(&a, |sql| {
        sql.execute("CREATE TABLE items (id INTEGER PRIMARY KEY, label TEXT, data BLOB, absent INTEGER)", &[])?;
        sql.execute("INSERT INTO items VALUES (?, ?, ?, ?)", &[&7i64, &"x'); DROP TABLE items; --", &vec![0u8, 255], &Option::<i64>::None])?;
        Ok(())
    }).unwrap();
    assert!(
        matches!(&sub.drain().unwrap()[..], [Notice::Changed(e)] if e.scope == Scope::Mount("main".into()) && e.key.is_none())
    );
    assert_eq!(
        session
            .capabilities(&a, Operation::Sql)
            .unwrap()
            .observation
            .coverage,
        ObservationCoverage::Mount
    );
    let rows = session
        .transaction(&a, |sql| {
            sql.query("SELECT id, label, data, absent FROM items", &[], |r| {
                Ok((
                    r.get::<i64>(0)?,
                    r.get::<String>(1)?,
                    r.get::<Vec<u8>>(2)?,
                    r.get::<Option<i64>>(3)?,
                ))
            })
        })
        .unwrap();
    assert_eq!(
        rows,
        vec![(7, "x'); DROP TABLE items; --".into(), vec![0, 255], None)]
    );
    assert!(sub.drain().unwrap().is_empty());
    let failed: Result<()> = session.transaction(&a, |sql| {
        sql.execute("INSERT INTO items(id) VALUES (?)", &[&8])?;
        Err(Error::Codec("abort".into()))
    });
    assert!(failed.is_err());
    assert!(sub.drain().unwrap().is_empty());
    assert_eq!(
        session
            .transaction(&a, |sql| sql.query(
                "SELECT count(*) FROM items",
                &[],
                |r| r.get::<i64>(0)
            ))
            .unwrap(),
        vec![1]
    );
    assert!(matches!(
        session.get(&a, "k", &Text),
        Err(Error::UnsupportedCapability {
            capability: Capability::TypedKv
        })
    ));
}

#[test]
fn sqlite_authority_and_transaction_escape_blocked() {
    let (registry, full, a) = setup(true);
    let narrow = registry.session([Grant::new(
        Scope::Dataset(a.clone()),
        [Operation::Sql, Operation::Subscribe],
    )]);
    assert!(matches!(
        narrow.transaction(&a, |_| Ok(())),
        Err(Error::Denied)
    ));
    assert!(matches!(
        narrow.subscribe(Scope::Mount("main".into()), 1),
        Err(Error::Denied)
    ));
    for sql in [
        "COMMIT",
        "ROLLBACK",
        "BEGIN",
        "SAVEPOINT escape",
        "ATTACH DATABASE ':memory:' AS escape",
        "DETACH DATABASE main",
        "PRAGMA writable_schema = ON",
        "PRAGMA temp_store_directory = '/tmp'",
        "PRAGMA journal_mode = OFF",
        "PRAGMA foreign_keys = OFF",
        "SELECT * FROM pragma_table_info('good')",
    ] {
        assert!(
            full.transaction(&a, |tx| tx.execute(sql, &[])).is_err(),
            "{sql}"
        );
    }
    full.transaction(&a, |tx| {
        tx.execute("CREATE TABLE good (x)", &[])?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        full.transaction(&a, |tx| tx.query(
            "INSERT INTO good VALUES (1) RETURNING x",
            &[],
            |r| r.get::<i64>(0)
        )),
        Err(Error::Denied)
    ));
}

#[test]
fn sqlite_vacuum_into_cannot_create_host_file() {
    let (_registry, session, a) = setup(true);
    let path = std::env::temp_dir().join(format!(
        "phenix-storage-escape-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(!path.exists());
    session
        .transaction(&a, |sql| {
            // VACUUM is forbidden inside the API's mandatory transaction. ATTACH is
            // independently denied by the authorizer, including SQLite's native use.
            assert!(
                sql.execute("VACUUM main INTO ?", &[&path.to_str().unwrap()])
                    .is_err()
            );
            assert!(
                sql.execute("ATTACH DATABASE ? AS escape", &[&path.to_str().unwrap()])
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
    assert!(!path.exists());
}

#[test]
fn sqlite_commit_failure_does_not_notify() {
    let (_registry, session, a) = setup(true);
    session.transaction(&a, |sql| {
        sql.execute("CREATE TABLE parent (id INTEGER PRIMARY KEY)", &[])?;
        sql.execute("CREATE TABLE child (id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED)", &[])?;
        Ok(())
    }).unwrap();
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 8).unwrap();
    assert!(
        session
            .transaction(&a, |sql| {
                sql.execute("INSERT INTO child VALUES (42)", &[])?;
                Ok(())
            })
            .is_err()
    );
    assert!(sub.drain().unwrap().is_empty());
    assert_eq!(
        session
            .transaction(&a, |sql| sql.query(
                "SELECT count(*) FROM child",
                &[],
                |r| r.get::<i64>(0)
            ))
            .unwrap(),
        vec![0]
    );
}

#[test]
fn concurrent_writes_have_monotonic_notifications() {
    let (_registry, session, a) = setup(false);
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 100).unwrap();
    std::thread::scope(|scope| {
        for i in 0..4 {
            let session = &session;
            let a = &a;
            scope.spawn(move || {
                for j in 0..20 {
                    session
                        .put(a, &format!("{i}-{j}"), &"v".into(), &Text)
                        .unwrap();
                }
            });
        }
    });
    let sequences: Vec<_> = sub
        .drain()
        .unwrap()
        .into_iter()
        .map(|e| match e {
            Notice::Changed(e) => e.sequence,
            _ => panic!("unexpected overflow"),
        })
        .collect();
    assert_eq!(sequences, (1..=80).collect::<Vec<_>>());
}

#[test]
fn sqlite_implicit_rollback_cannot_escape_into_autocommit() {
    let (_registry, session, a) = setup(true);
    session
        .transaction(&a, |sql| {
            sql.execute("CREATE TABLE t (id INTEGER UNIQUE)", &[])?;
            sql.execute("INSERT INTO t VALUES (1)", &[])?;
            Ok(())
        })
        .unwrap();
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 8).unwrap();
    assert!(
        session
            .transaction(&a, |sql| {
                assert!(
                    sql.execute("INSERT OR ROLLBACK INTO t VALUES (1)", &[])
                        .is_err()
                );
                assert!(matches!(
                    sql.execute("INSERT INTO t VALUES (2)", &[]),
                    Err(Error::TransactionEnded)
                ));
                assert!(matches!(
                    sql.query("SELECT * FROM t", &[], |r| r.get::<i64>(0)),
                    Err(Error::TransactionEnded)
                ));
                Ok(())
            })
            .is_err()
    );
    assert!(sub.drain().unwrap().is_empty());
    assert_eq!(
        session
            .transaction(&a, |sql| sql
                .query("SELECT id FROM t", &[], |r| r.get::<i64>(0)))
            .unwrap(),
        vec![1]
    );
}

#[test]
fn sqlite_cross_dataset_effects_notify_only_same_adapter() {
    let (registry, session, a) = setup(true);
    let b = Address::parse("store://main/other").unwrap();
    registry.register(&b, Text.schema()).unwrap();
    registry.mount_sqlite("isolated", ":memory:").unwrap();
    let c = Address::parse("store://isolated/items").unwrap();
    registry.register(&c, Text.schema()).unwrap();
    let isolated = registry.session([Grant::new(
        Scope::Dataset(c.clone()),
        [Operation::Subscribe],
    )]);
    let mut same = session.subscribe(Scope::Dataset(b), 2).unwrap();
    let mut other = isolated.subscribe(Scope::Dataset(c), 2).unwrap();
    session
        .transaction(&a, |sql| {
            sql.execute("CREATE TABLE unrelated (x)", &[])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(same.drain().unwrap().len(), 1);
    assert!(other.drain().unwrap().is_empty());
}

#[test]
fn sqlite_panic_rolls_back_without_poisoning_any_mount() {
    let (registry, session, a) = setup(true);
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 1).unwrap();
    registry.mount_memory("other").unwrap();
    let other = Address::parse("store://other/items").unwrap();
    registry.register(&other, Text.schema()).unwrap();
    let memory = registry.session([Grant::new(
        Scope::Mount("other".into()),
        [Operation::Read, Operation::Write],
    )]);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<()> = session.transaction(&a, |sql| {
            sql.execute("CREATE TABLE aborted (x)", &[])?;
            panic!("callback panic");
        });
    }));
    assert!(result.is_err());
    assert!(session.capabilities(&a, Operation::Sql).unwrap().sql);
    assert!(sub.drain().unwrap().is_empty());
    session
        .transaction(&a, |sql| {
            assert_eq!(
                sql.query(
                    "SELECT count(*) FROM sqlite_master WHERE name = 'aborted'",
                    &[],
                    |r| r.get::<i64>(0)
                )?,
                vec![0]
            );
            sql.execute("CREATE TABLE recovered (x)", &[])?;
            Ok(())
        })
        .unwrap();
    memory.put(&other, "k", &"v".into(), &Text).unwrap();
    assert_eq!(memory.get(&other, "k", &Text).unwrap(), Some("v".into()));
    drop(sub);
    drop(session);
    drop(registry);
}

#[test]
fn codec_failure_does_not_mutate_or_notify() {
    struct Failing;
    impl Codec for Failing {
        type Value = String;
        fn schema(&self) -> Schema {
            Text.schema()
        }
        fn encode(&self, _: &String) -> Result<Vec<u8>> {
            Err(Error::Codec("encode failed".into()))
        }
        fn decode(&self, _: &[u8]) -> Result<String> {
            Err(Error::Codec("decode failed".into()))
        }
    }
    let (_registry, session, a) = setup(false);
    let mut sub = session.subscribe(Scope::Dataset(a.clone()), 4).unwrap();
    assert!(matches!(
        session.put(&a, "x", &"value".into(), &Failing),
        Err(Error::Codec(_))
    ));
    assert!(sub.drain().unwrap().is_empty());
    assert_eq!(session.get(&a, "x", &Text).unwrap(), None);
    session.put(&a, "x", &"value".into(), &Text).unwrap();
    assert!(matches!(
        session.get(&a, "x", &Failing),
        Err(Error::Codec(_))
    ));
    assert_eq!(session.get(&a, "x", &Text).unwrap(), Some("value".into()));
}

#[test]
fn sqlite_type_and_parameter_errors_remain_structured() {
    let (_registry, session, a) = setup(true);
    assert!(matches!(
        session.transaction(&a, |sql| sql
            .query("SELECT 'not an integer'", &[], |row| row.get::<i64>(0))),
        Err(Error::Sql(_))
    ));
    assert!(matches!(
        session.transaction(&a, |sql| sql
            .query("SELECT ?", &[], |row| row.get::<i64>(0))),
        Err(Error::Sql(_))
    ));
    assert!(matches!(
        session.transaction(&a, |sql| sql
            .query("SELECT 1", &[], |row| row.get::<i64>(4))),
        Err(Error::Sql(_))
    ));
}
