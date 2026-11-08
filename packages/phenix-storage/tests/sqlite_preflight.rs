use phenix_storage::{Address, Error, Registry, Schema};

fn schema() -> Schema {
    Schema {
        name: "text".into(),
        revision: 1,
        structure: "utf8-string".into(),
        codec: "utf8".into(),
    }
}
fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "phenix-preflight-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn missing_path_is_not_provisioned_even_when_parent_is_missing() {
    let parent = path("absent-parent");
    let file = parent.join("store.sqlite");
    Registry::validate_sqlite_datasets(&file, &[("items".into(), schema())]).unwrap();
    assert!(!file.exists());
    assert!(!parent.exists());
}

#[test]
fn persisted_schema_checks_all_fields_without_mutating_database() {
    let path = path("persisted");
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        registry
            .register(&Address::parse("store://main/items").unwrap(), schema())
            .unwrap();
        registry
            .register(&Address::parse("store://main/other").unwrap(), schema())
            .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    Registry::validate_sqlite_datasets(&path, &[]).unwrap();
    Registry::validate_sqlite_datasets(
        &path,
        &[("items".into(), schema()), ("new".into(), schema())],
    )
    .unwrap();
    for field in 0..4 {
        let mut incompatible = schema();
        match field {
            0 => incompatible.name.push('x'),
            1 => incompatible.revision += 1,
            2 => incompatible.structure.push('x'),
            _ => incompatible.codec.push('x'),
        }
        assert!(matches!(
            Registry::validate_sqlite_datasets(
                &path,
                &[
                    ("new".into(), schema()),
                    ("items".into(), schema()),
                    ("other".into(), incompatible)
                ]
            ),
            Err(Error::SchemaMismatch)
        ));
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let connection =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM __phenix_schema", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(connection);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn legacy_kv_without_metadata_is_allowed_without_migration() {
    let path = path("legacy");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE __phenix_kv(dataset TEXT, key TEXT, value BLOB); INSERT INTO __phenix_kv VALUES('items','key',X'61')").unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    Registry::validate_sqlite_datasets(&path, &[("items".into(), schema())]).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let connection =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('__phenix_schema','__phenix_revision_clock')", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    drop(connection);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn metadata_view_is_rejected_without_derived_execution() {
    let path = path("view");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE VIEW __phenix_schema AS SELECT * FROM nonexistent")
            .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        Registry::validate_sqlite_datasets(&path, &[("items".into(), schema())]),
        Err(Error::SchemaMismatch)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_existing_file_returns_structured_sql_error_without_modification() {
    let path = path("invalid");
    let before = b"not a SQLite database";
    std::fs::write(&path, before).unwrap();
    assert!(matches!(
        Registry::validate_sqlite_datasets(&path, &[]),
        Err(Error::Sql(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn registration_rechecks_schema_after_successful_preflight() {
    let path = path("plan-race");
    let planned = schema();
    Registry::validate_sqlite_datasets(&path, &[("items".into(), planned.clone())]).unwrap();
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        let mut changed = schema();
        changed.revision += 1;
        registry
            .register(&Address::parse("store://main/items").unwrap(), changed)
            .unwrap();
    }
    {
        let registry = Registry::new();
        registry.mount_sqlite("main", &path).unwrap();
        assert!(matches!(
            registry.register(&Address::parse("store://main/items").unwrap(), planned),
            Err(Error::SchemaMismatch)
        ));
    }
    std::fs::remove_file(path).unwrap();
}
