//! SQLite-backed Basic Storage. Session and journal semantics stay in their
//! own plugins. SQLite serializes CAS transactions across provider processes.
//!
//! PHENIX_BASIC_SQLITE_PATH is supplied by the runtime, which resolves the
//! XDG state default when the caller does not set it. No database is created in
//! the current directory; a missing file-backed path fails closed.
use phenix_basic_contracts::{
    error, BasicError, ErrorCode, RecordKey, StorageEdit, StorageTransaction, StoredRecord,
};
use phenix_sdk::Blob;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::{collections::BTreeSet, env, time::Duration};

const CREATE: &str = "CREATE TABLE IF NOT EXISTS basic_records (session TEXT NOT NULL, name TEXT NOT NULL, revision INTEGER NOT NULL CHECK (revision > 0), data BLOB NOT NULL, PRIMARY KEY (session, name)) WITHOUT ROWID";
const SELECT: &str = "SELECT revision, data FROM basic_records WHERE session = ?1 AND name = ?2";
const UPSERT: &str = "INSERT INTO basic_records (session, name, revision, data) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(session, name) DO UPDATE SET revision = excluded.revision, data = excluded.data";

fn failed() -> BasicError {
    error(ErrorCode::StorageFailed, "SQLite storage operation failed")
}
fn bad_key() -> BasicError {
    error(ErrorCode::InvalidInput, "invalid storage transaction key")
}
fn checked_i64(value: u64) -> Result<i64, BasicError> {
    i64::try_from(value).map_err(|_| error(ErrorCode::Conflict, "storage revision overflow"))
}

/// Database files made before schema versioning use user_version = 0.
/// The only supported migration retains the original record layout and marks
/// it as version 1. Reject future or malformed schemas before any Storage I/O.
fn schema_version(conn: &Connection) -> Result<i64, BasicError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|_| failed())
}

fn check_schema(conn: &Connection) -> Result<(), BasicError> {
    let mut statement = conn
        .prepare("PRAGMA table_info(basic_records)")
        .map_err(|_| failed())?;
    let columns = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|_| failed())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| failed())?;
    let expected = [
        ("session".into(), "TEXT".into(), 1_i64, 1_i64),
        ("name".into(), "TEXT".into(), 1_i64, 2_i64),
        ("revision".into(), "INTEGER".into(), 1_i64, 0_i64),
        ("data".into(), "BLOB".into(), 1_i64, 0_i64),
    ];
    if columns.as_slice() != expected.as_slice() {
        return Err(failed());
    }
    Ok(())
}

fn ensure_schema(conn: &mut Connection) -> Result<(), BasicError> {
    match schema_version(conn)? {
        1 => check_schema(conn),
        0 => {
            // Serialize competing first opens across processes. Recheck the
            // version after BEGIN IMMEDIATE so only one process migrates.
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| failed())?;
            match schema_version(&tx)? {
                0 => {
                    tx.execute_batch(CREATE).map_err(|_| failed())?;
                    check_schema(&tx)?;
                    tx.execute_batch("PRAGMA user_version = 1")
                        .map_err(|_| failed())?;
                }
                1 => check_schema(&tx)?,
                _ => return Err(failed()),
            }
            tx.commit().map_err(|_| failed())
        }
        _ => Err(failed()),
    }
}

struct Db(Connection);
impl Db {
    fn open() -> Result<Self, BasicError> {
        let name = env::var("PHENIX_BASIC_SQLITE_PATH").map_err(|_| {
            error(
                ErrorCode::StorageFailed,
                "SQLite path is not configured: PHENIX_BASIC_SQLITE_PATH",
            )
        })?;
        if name.is_empty()
            || name == ":memory:"
            || name.starts_with("file:")
            || !std::path::Path::new(&name).is_absolute()
        {
            return Err(error(
                ErrorCode::StorageFailed,
                "SQLite path must be an absolute file path",
            ));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let mut conn = Connection::open_with_flags(&name, flags).map_err(|_| {
            error(
                ErrorCode::StorageFailed,
                "SQLite database cannot be opened; check its parent directory and permissions",
            )
        })?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|_| failed())?;
        conn.execute_batch("PRAGMA synchronous = FULL")
            .map_err(|_| failed())?;
        ensure_schema(&mut conn).map_err(|_| {
            error(
                ErrorCode::StorageFailed,
                "SQLite schema is incompatible, corrupt or could not be migrated",
            )
        })?;
        Ok(Self(conn))
    }

    fn get(&self, key: &RecordKey) -> Result<Option<StoredRecord>, BasicError> {
        read_record(&self.0, key)
    }
}

fn read_record(conn: &Connection, key: &RecordKey) -> Result<Option<StoredRecord>, BasicError> {
    let row: Option<(i64, Vec<u8>)> = conn
        .query_row(SELECT, params![&key.session, &key.key], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .map_err(|_| failed())?;
    row.map(|(revision, bytes)| {
        let revision = u64::try_from(revision).map_err(|_| failed())?;
        if revision == 0 {
            return Err(failed());
        }
        Ok(StoredRecord {
            revision,
            data: Blob(bytes),
        })
    })
    .transpose()
}

fn write_record(
    conn: &Connection,
    key: &RecordKey,
    record: &StoredRecord,
) -> Result<(), BasicError> {
    conn.execute(
        UPSERT,
        params![
            &key.session,
            &key.key,
            checked_i64(record.revision)?,
            &record.data.0
        ],
    )
    .map_err(|_| failed())?;
    Ok(())
}

/// Check the whole edit set before writing. An immediate SQLite transaction
/// serializes inter-process CAS; errors roll back the entire edit set.
fn transact(request: StorageTransaction) -> Result<Vec<StoredRecord>, BasicError> {
    let mut keys = BTreeSet::new();
    for edit in &request.edits {
        if edit.key.is_empty() || !keys.insert(edit.key.as_str()) {
            return Err(bad_key());
        }
        checked_i64(
            edit.expected
                .checked_add(1)
                .ok_or_else(|| error(ErrorCode::Conflict, "storage revision overflow"))?,
        )?;
    }
    if request.session.is_empty() {
        return Err(bad_key());
    }

    let mut db = Db::open()?;
    let tx =
        db.0.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| failed())?;
    let result = (|| {
        let mut records = Vec::with_capacity(request.edits.len());
        for StorageEdit {
            key,
            expected,
            data,
        } in request.edits
        {
            let record_key = RecordKey {
                session: request.session.clone(),
                key,
            };
            let current = read_record(&tx, &record_key)?;
            let revision = current.as_ref().map_or(0, |record| record.revision);
            if revision != expected {
                return Err(error(ErrorCode::Conflict, "storage revision conflict"));
            }
            let record = StoredRecord {
                revision: revision + 1,
                data,
            };
            write_record(&tx, &record_key, &record)?;
            records.push(record);
        }
        Ok(records)
    })();
    match result {
        Ok(records) => {
            tx.commit().map_err(|_| failed())?;
            Ok(records)
        }
        Err(err) => {
            let _ = tx.rollback();
            Err(err)
        }
    }
}

#[phenix_sdk::plugin(
    id = "phenix.basic.storage-sqlite",
    version = "1.0.0",
    provide(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use super::{transact, Db};
    use phenix_basic_contracts::{
        error, BasicError, ErrorCode, RecordKey, Storage, StorageTransaction, StoredRecord,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(Storage::read)]
    fn read(
        _: &mut State,
        host: &Host<'_>,
        request: RecordKey,
    ) -> CallResult<StoredRecord, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        if request.key.is_empty() {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "empty storage key",
            )));
        }
        Db::open()
            .and_then(|db| db.get(&request))
            .and_then(|row| row.ok_or_else(|| error(ErrorCode::NotFound, "missing record")))
            .map_err(CallError::Domain)
    }

    #[provide(Storage::transact)]
    fn write(
        _: &mut State,
        host: &Host<'_>,
        request: StorageTransaction,
    ) -> CallResult<Vec<StoredRecord>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        transact(request).map_err(CallError::Domain)
    }
}
