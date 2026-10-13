//! SQLite-backed Basic Storage. Session and journal semantics stay in their
//! own plugins. SQLite serializes CAS transactions across provider processes.
//!
//! PHENIX_BASIC_SQLITE_PATH is set by the product launcher. No default database
//! is created in the current directory, and a missing setting fails closed.
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

struct Db(Connection);
impl Db {
    fn open() -> Result<Self, BasicError> {
        let name = env::var("PHENIX_BASIC_SQLITE_PATH").map_err(|_| failed())?;
        if name.is_empty() || name == ":memory:" || name.starts_with("file:") {
            return Err(failed());
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = Connection::open_with_flags(name, flags).map_err(|_| failed())?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|_| failed())?;
        conn.execute_batch("PRAGMA synchronous = FULL")
            .map_err(|_| failed())?;
        conn.execute_batch(CREATE).map_err(|_| failed())?;
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
