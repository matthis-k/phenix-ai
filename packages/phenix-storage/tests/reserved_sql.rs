use phenix_storage::*;

struct Bytes;
impl Codec for Bytes {
    type Value = Vec<u8>;
    fn schema(&self) -> Schema {
        Schema {
            name: "bytes".into(),
            revision: 1,
            structure: "bytes".into(),
            codec: "bytes".into(),
        }
    }
    fn encode(&self, value: &Vec<u8>) -> Result<Vec<u8>> {
        Ok(value.clone())
    }
    fn decode(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        Ok(bytes.to_vec())
    }
}
fn address() -> Address {
    Address::parse("store://main/items").unwrap()
}

#[test]
fn reserved_objects_are_protected_at_every_sql_authority_level() {
    let registry = Registry::new();
    registry.mount_sqlite("main", ":memory:").unwrap();
    registry.register(&address(), Bytes.schema()).unwrap();
    let kv = registry.session([Grant::new(
        Scope::Dataset(address()),
        [Operation::Read, Operation::Write, Operation::Subscribe],
    )]);
    let revision = kv
        .compare_exchange(&address(), "secret", None, &vec![42], &Bytes)
        .unwrap();
    let admin = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlAdmin],
    )]);
    admin.transaction_mount("main", |sql| {
        sql.execute("CREATE TABLE ordinary (x)", &[])?;
        // SQLite stores view/trigger bodies without authorizing their accesses.
        // Their eventual execution must still be checked, including nested views.
        sql.execute("CREATE VIEW indirect AS SELECT value FROM __phenix_kv", &[])?;
        sql.execute("CREATE VIEW nested AS SELECT * FROM indirect", &[])?;
        sql.execute("CREATE TRIGGER indirect_write AFTER INSERT ON ordinary BEGIN DELETE FROM __phenix_kv; END", &[])?;
        Ok(())
    }).unwrap();
    let mut notices = kv.subscribe(Scope::Dataset(address()), 64).unwrap();
    for operation in [
        Operation::SqlRead,
        Operation::SqlWrite,
        Operation::SqlAdmin,
        Operation::Sql,
    ] {
        let session = registry.session([Grant::new(Scope::Mount("main".into()), [operation])]);
        for attack in [
            "SELECT * FROM __phenix_kv",
            "SELECT count(*) FROM main.__PHENIX_KV",
            "SELECT * FROM __phenix_schema",
            "SELECT * FROM __phenix_revision_clock",
            "SELECT * FROM indirect",
            "SELECT * FROM nested",
            "SELECT sql FROM sqlite_master",
            "SELECT sql FROM sqlite_schema",
        ] {
            assert!(
                session
                    .transaction_mount("main", |sql| sql.query_each(attack, &[], |_| Ok(())))
                    .is_err(),
                "{operation:?}: {attack}"
            );
        }
        for attack in [
            "DELETE FROM __phenix_kv",
            "UPDATE __phenix_kv SET value = X'00'",
            "INSERT INTO __phenix_kv(dataset,key,value,revision) VALUES ('items','evil',X'00',1)",
            "DELETE FROM __phenix_schema",
            "UPDATE __phenix_revision_clock SET sequence = 0",
            "DROP TABLE __phenix_kv",
            "ALTER TABLE __phenix_schema RENAME TO stolen",
            "ALTER TABLE ordinary RENAME TO __phenix_stolen",
            "CREATE TABLE __PHENIX_custom (x)",
            "CREATE TEMP TABLE __phenix_kv (x)",
            "CREATE INDEX ordinary_index ON __phenix_kv(value)",
            "CREATE INDEX __phenix_index ON ordinary(x)",
            "CREATE VIEW __phenix_view AS SELECT * FROM ordinary",
            "CREATE TEMP VIEW __phenix_view AS SELECT * FROM ordinary",
            "CREATE TRIGGER reserved AFTER INSERT ON __phenix_kv BEGIN SELECT 1; END",
            "CREATE TEMP TRIGGER reserved AFTER INSERT ON main.__phenix_kv BEGIN SELECT 1; END",
            "CREATE TRIGGER __phenix_trigger AFTER INSERT ON ordinary BEGIN SELECT 1; END",
            "INSERT INTO ordinary VALUES (1)",
            "ANALYZE __phenix_kv",
            "PRAGMA writable_schema = ON",
            "SELECT * FROM pragma_table_info('__phenix_kv')",
            "ATTACH DATABASE ':memory:' AS other",
            "COMMIT",
            "SAVEPOINT escape",
        ] {
            assert!(
                session
                    .transaction_mount("main", |sql| sql.execute(attack, &[]))
                    .is_err(),
                "{operation:?}: {attack}"
            );
        }
        assert!(notices.drain().unwrap().is_empty());
        assert_eq!(
            kv.get_revisioned(&address(), "secret", &Bytes)
                .unwrap()
                .unwrap(),
            Revisioned {
                value: vec![42],
                revision: revision.clone()
            }
        );
    }
}

#[test]
fn mount_only_sql_and_authorizer_transitions_cover_all_callback_paths() {
    let registry = Registry::new();
    registry.mount_sqlite("main", ":memory:").unwrap();
    let admin = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlAdmin],
    )]);
    assert!(
        admin
            .capabilities_mount("main", Operation::SqlAdmin)
            .unwrap()
            .sql
    );
    assert!(matches!(
        admin.transaction(&address(), |_| Ok(())),
        Err(Error::UnknownDataset)
    ));
    admin
        .transaction_mount("main", |sql| sql.execute("CREATE TABLE app (x)", &[]))
        .unwrap();
    registry.register(&address(), Bytes.schema()).unwrap();
    let kv = registry.session([Grant::new(
        Scope::Dataset(address()),
        [
            Operation::Read,
            Operation::Write,
            Operation::Delete,
            Operation::Subscribe,
        ],
    )]);
    let read = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlRead],
    )]);
    let mut notices = kv.subscribe(Scope::Dataset(address()), 16).unwrap();
    for mode in 0..4 {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read.transaction_mount("main", |sql| {
                assert_eq!(sql.query("SELECT 1", &[], |r| r.get::<i64>(0))?, vec![1]);
                match mode {
                    0 => Ok(()),
                    1 => Err(Error::Codec("callback failure".into())),
                    2 => {
                        sql.query_each("SELECT * FROM __phenix_kv", &[], |_| Ok(()))?;
                        Ok(())
                    }
                    _ => panic!("callback panic"),
                }
            })
        }));
        match mode {
            0 => assert!(result.unwrap().is_ok()),
            3 => assert!(result.is_err()),
            _ => assert!(result.unwrap().is_err()),
        }
        assert!(notices.drain().unwrap().is_empty());
        let revision = kv
            .compare_exchange(&address(), "k", None, &vec![mode], &Bytes)
            .unwrap();
        let updated = kv
            .compare_exchange(&address(), "k", Some(&revision), &vec![42], &Bytes)
            .unwrap();
        assert_ne!(revision, updated);
        assert_eq!(kv.get(&address(), "k", &Bytes).unwrap(), Some(vec![42]));
        assert!(kv.delete(&address(), "k").unwrap());
        assert_eq!(notices.drain().unwrap().len(), 3);
        admin
            .transaction_mount("main", |sql| {
                assert!(sql.execute("DELETE FROM __phenix_kv", &[]).is_err());
                sql.execute("INSERT INTO app VALUES (1)", &[])?;
                Ok(())
            })
            .unwrap();
        assert_eq!(notices.drain().unwrap().len(), 1);
    }
    let narrow = registry.session([Grant::new(Scope::Dataset(address()), [Operation::SqlAdmin])]);
    assert!(matches!(
        narrow.transaction_mount("main", |_| Ok(())),
        Err(Error::Denied)
    ));
    assert!(matches!(
        narrow.capabilities_mount("main", Operation::SqlAdmin),
        Err(Error::Denied)
    ));
}

#[test]
fn external_backend_recovers_host_authority_after_every_sql_exit() {
    struct External(rusqlite::Connection);
    impl Backend for External {
        fn sql(&mut self) -> Option<&mut dyn SqlBackend> {
            Some(self)
        }
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            Some(self)
        }
    }
    impl SqlBackend for External {
        fn connection(&mut self) -> &mut rusqlite::Connection {
            &mut self.0
        }
    }
    impl TypedKv for External {
        fn get(&mut self, _: &str, _: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }
        fn put(&mut self, _: &str, _: &str, _: Vec<u8>) -> Result<()> {
            self.0.execute("INSERT INTO __phenix_host VALUES (1)", [])?;
            Ok(())
        }
        fn delete(&mut self, _: &str, _: &str) -> Result<bool> {
            Ok(false)
        }
    }
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE __phenix_host(x); CREATE TABLE app(x UNIQUE); INSERT INTO app VALUES(1); CREATE TABLE parent(x PRIMARY KEY); CREATE TABLE child(x REFERENCES parent(x) DEFERRABLE INITIALLY DEFERRED)").unwrap();
    let registry = Registry::new();
    registry.mount("main", External(connection)).unwrap();
    registry.register(&address(), Bytes.schema()).unwrap();
    let kv = registry.session([Grant::new(
        Scope::Dataset(address()),
        [Operation::Write, Operation::Subscribe],
    )]);
    let read = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlRead],
    )]);
    let admin = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::SqlAdmin],
    )]);
    let mut notices = kv.subscribe(Scope::Dataset(address()), 8).unwrap();
    for mode in 0..7 {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let sql_session = if mode >= 4 { &admin } else { &read };
            sql_session.transaction_mount("main", |sql| {
                assert!(
                    sql.query_each("SELECT * FROM __phenix_host", &[], |_| Ok(()))
                        .is_err()
                );
                match mode {
                    0 => Ok(()),
                    1 => Err(Error::Codec("callback".into())),
                    2 => sql.query_each("SELECT * FROM missing", &[], |_| Ok(())),
                    3 => panic!("callback"),
                    4 => {
                        assert!(
                            sql.execute("INSERT OR ROLLBACK INTO app VALUES(1)", &[])
                                .is_err()
                        );
                        Ok(())
                    }
                    5 => {
                        sql.execute("INSERT INTO child VALUES(1)", &[])?;
                        Ok(())
                    }
                    _ => sql.query_each("SELECT 1", &[], |_| panic!("row callback")),
                }
            })
        }));
        match mode {
            0 => assert!(result.unwrap().is_ok()),
            3 | 6 => assert!(result.is_err()),
            _ => assert!(result.unwrap().is_err()),
        }
        assert!(notices.drain().unwrap().is_empty());
        kv.put(&address(), "k", &vec![], &Bytes)
            .expect("SQL policy must not remain on the host connection");
        assert_eq!(notices.drain().unwrap().len(), 1);
    }
}

#[test]
fn transaction_and_mount_capabilities_authorize_before_lookup() {
    let registry = Registry::new();
    registry.mount_sqlite("main", ":memory:").unwrap();
    registry.mount_memory("memory").unwrap();
    let missing = Address::parse("store://missing/items").unwrap();
    for denied in [
        registry.session([]),
        registry.session([Grant::new(Scope::Dataset(address()), [Operation::SqlAdmin])]),
    ] {
        for address in [&address(), &missing] {
            assert!(matches!(
                denied.transaction(address, |_| -> Result<()> { panic!("denied callback") }),
                Err(Error::Denied)
            ));
            assert!(matches!(
                denied.capabilities(address, Operation::SqlAdmin),
                Err(Error::Denied)
            ));
            assert!(matches!(
                denied.transaction_mount(address.mount(), |_| -> Result<()> {
                    panic!("denied callback")
                }),
                Err(Error::Denied)
            ));
            assert!(matches!(
                denied.capabilities_mount(address.mount(), Operation::SqlAdmin),
                Err(Error::Denied)
            ));
        }
    }
    let admin = registry.session([
        Grant::new(Scope::Mount("main".into()), [Operation::SqlAdmin]),
        Grant::new(Scope::Mount("missing".into()), [Operation::SqlAdmin]),
    ]);
    assert!(matches!(
        admin.transaction(&address(), |_| Ok(())),
        Err(Error::UnknownDataset)
    ));
    assert!(matches!(
        admin.transaction(&missing, |_| Ok(())),
        Err(Error::UnknownMount)
    ));
    assert!(matches!(
        admin.transaction_mount("missing", |_| Ok(())),
        Err(Error::UnknownMount)
    ));
    assert!(matches!(
        admin.capabilities_mount("missing", Operation::SqlAdmin),
        Err(Error::UnknownMount)
    ));
    // Capability inspection requires the exact requested grant, not SQL's execution hierarchy.
    assert!(matches!(
        admin.capabilities_mount("main", Operation::SqlRead),
        Err(Error::Denied)
    ));
    registry.register(&address(), Bytes.schema()).unwrap();
    assert!(
        admin
            .capabilities(&address(), Operation::SqlAdmin)
            .unwrap()
            .sql
    );
    let memory = registry.session([Grant::new(
        Scope::Mount("memory".into()),
        [Operation::SqlAdmin],
    )]);
    let capabilities = memory
        .capabilities_mount("memory", Operation::SqlAdmin)
        .unwrap();
    assert!(!capabilities.sql && capabilities.typed_kv && capabilities.compare_exchange);
    assert!(matches!(
        memory.transaction_mount("memory", |_| Ok(())),
        Err(Error::UnsupportedCapability {
            capability: Capability::Sql
        })
    ));
    let invalid = registry.session([Grant::new(
        Scope::Mount("../escape".into()),
        [Operation::SqlRead],
    )]);
    assert!(matches!(
        invalid.transaction_mount("../escape", |_| Ok(())),
        Err(Error::InvalidAddress)
    ));
    assert!(matches!(
        invalid.capabilities_mount("../escape", Operation::SqlRead),
        Err(Error::InvalidAddress)
    ));
    assert!(matches!(
        admin.transaction_mount("../escape", |_| Ok(())),
        Err(Error::Denied)
    ));
    admin
        .transaction_mount("main", |_| {
            assert!(matches!(
                admin.capabilities_mount("main", Operation::SqlAdmin),
                Err(Error::Reentrant)
            ));
            assert!(matches!(
                admin.transaction(&address(), |_| Ok(())),
                Err(Error::Reentrant)
            ));
            Ok(())
        })
        .unwrap();
}

#[test]
fn capabilities_probe_each_backend_interface_once() {
    struct Counted {
        probes: usize,
        kv: MemoryBackend,
    }
    impl Backend for Counted {
        fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
            self.probes += 1;
            assert_eq!(
                self.probes, 1,
                "one inspection must not probe a mutable interface twice"
            );
            Some(&mut self.kv)
        }
    }
    let registry = Registry::new();
    registry
        .mount(
            "main",
            Counted {
                probes: 0,
                kv: MemoryBackend::default(),
            },
        )
        .unwrap();
    let session = registry.session([Grant::new(Scope::Mount("main".into()), [Operation::Read])]);
    let caps = session.capabilities_mount("main", Operation::Read).unwrap();
    assert!(caps.typed_kv && caps.compare_exchange && !caps.sql);
}

#[test]
fn metadata_and_reserved_reads_are_denied_through_nested_and_temp_triggers() {
    let registry = Registry::new();
    registry.mount_sqlite("main", ":memory:").unwrap();
    registry.register(&address(), Bytes.schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [
            Operation::SqlAdmin,
            Operation::Read,
            Operation::Write,
            Operation::Subscribe,
        ],
    )]);
    session
        .put(&address(), "secret", &vec![42], &Bytes)
        .unwrap();
    session.transaction_mount("main", |sql| {
        sql.execute("CREATE TABLE gate(x)", &[])?;
        sql.execute("CREATE TABLE stage(x)", &[])?;
        sql.execute("CREATE TABLE stolen(x)", &[])?;
        sql.execute("CREATE VIEW metadata AS SELECT sql FROM sqlite_schema", &[])?;
        sql.execute("CREATE TEMP VIEW temp_metadata AS SELECT sql FROM main.sqlite_master", &[])?;
        sql.execute("CREATE TRIGGER leak AFTER INSERT ON stage BEGIN INSERT INTO stolen SELECT value FROM __phenix_kv; END", &[])?;
        sql.execute("CREATE TRIGGER nested AFTER INSERT ON gate BEGIN INSERT INTO stage VALUES(new.x); END", &[])?;
        sql.execute("CREATE TEMP TRIGGER temp_leak AFTER UPDATE ON main.gate BEGIN INSERT INTO stolen SELECT * FROM temp_metadata; END", &[])?;
        Ok(())
    }).unwrap();
    let mut notices = session.subscribe(Scope::Dataset(address()), 8).unwrap();
    for query in [
        "SELECT * FROM metadata",
        "SELECT * FROM temp_metadata",
        "SELECT EXISTS(SELECT 1 FROM __phenix_kv)",
        "SELECT 1 FROM __phenix_kv LIMIT 1",
    ] {
        assert!(
            session
                .transaction_mount("main", |sql| sql.query_each(query, &[], |_| Ok(())))
                .is_err(),
            "{query}"
        );
    }
    for mutation in ["INSERT INTO gate VALUES(1)", "UPDATE gate SET x = 1"] {
        assert!(
            session
                .transaction_mount("main", |sql| sql.execute(mutation, &[]))
                .is_err(),
            "{mutation}"
        );
    }
    assert!(notices.drain().unwrap().is_empty());
    session
        .transaction_mount("main", |sql| {
            assert_eq!(
                sql.query("SELECT count(*) FROM stolen", &[], |r| r.get::<i64>(0))?,
                vec![0]
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(
        session.get(&address(), "secret", &Bytes).unwrap(),
        Some(vec![42])
    );
}

#[test]
fn persisted_trigger_cannot_gain_adapter_authority_during_kv_writes() {
    let path = std::env::temp_dir().join(format!(
        "phenix-derived-trigger-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Bytes.schema()).unwrap();
    }
    {
        // Prior versions allowed SQL administrators to attach triggers to KV.
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE stolen(value); CREATE TRIGGER legacy_leak AFTER INSERT ON __phenix_kv BEGIN INSERT INTO stolen SELECT value FROM __phenix_kv; END").unwrap();
    }
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry.register(&address(), Bytes.schema()).unwrap();
        let kv = registry.session([Grant::new(
            Scope::Dataset(address()),
            [Operation::Write, Operation::Read, Operation::Subscribe],
        )]);
        let mut notices = kv.subscribe(Scope::Dataset(address()), 4).unwrap();
        assert!(
            kv.compare_exchange(&address(), "secret", None, &vec![42], &Bytes)
                .is_err()
        );
        assert!(kv.get(&address(), "secret", &Bytes).unwrap().is_none());
        assert!(notices.drain().unwrap().is_empty());
    }
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM stolen", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT sequence FROM __phenix_revision_clock", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    std::fs::remove_file(path).unwrap();
}
