//! Project-scoped, revision-checked SQLite records.
//! Reuses rusqlite and the Basic database file, but does not expose Basic
//! session records or modify its storage contract or schema version.
use phenix_advanced_contracts::{
    ProjectEdit, ProjectRecord, ProjectRecordKey, ProjectStorageError, ProjectStorageProblem,
    ProjectTransaction,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::{collections::BTreeSet, env, time::Duration};

const TABLE: &str = "CREATE TABLE IF NOT EXISTS project_records (project TEXT NOT NULL, name TEXT NOT NULL, revision INTEGER NOT NULL CHECK (revision > 0), data BLOB NOT NULL, PRIMARY KEY (project, name)) WITHOUT ROWID";
const READ: &str = "SELECT revision, data FROM project_records WHERE project = ?1 AND name = ?2";
const UPSERT: &str = "INSERT INTO project_records (project, name, revision, data) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(project, name) DO UPDATE SET revision = excluded.revision, data = excluded.data";
const MAX_BLOB_BYTES: usize = 1024 * 1024;
const MAX_EDITS: usize = 256;

fn error(problem: ProjectStorageProblem, message: &str) -> ProjectStorageError {
    ProjectStorageError {
        problem,
        message: message.into(),
    }
}
fn failed() -> ProjectStorageError {
    error(ProjectStorageProblem::StorageFailed, "project SQLite operation failed")
}
fn invalid() -> ProjectStorageError {
    error(ProjectStorageProblem::InvalidInput, "invalid project record request")
}
fn version_i64(n: u64) -> Result<i64, ProjectStorageError> {
    i64::try_from(n).map_err(|_| error(ProjectStorageProblem::Conflict, "revision overflow"))
}
fn valid(project: &str, key: &str) -> bool {
    phenix_sdk::identifier(project) && phenix_sdk::identifier(key)
}

fn open() -> Result<Connection, ProjectStorageError> {
    let path = env::var("PHENIX_BASIC_SQLITE_PATH").map_err(|_| failed())?;
    if path.is_empty() || path == ":memory:" || path.starts_with("file:") {
        return Err(failed());
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    let conn = Connection::open_with_flags(path, flags).map_err(|_| failed())?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|_| failed())?;
    conn.execute_batch("PRAGMA synchronous = FULL")
        .map_err(|_| failed())?;
    conn.execute_batch(TABLE).map_err(|_| failed())?;
    let mut statement = conn.prepare("PRAGMA table_info(project_records)")
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
        ("project".into(), "TEXT".into(), 1_i64, 1_i64),
        ("name".into(), "TEXT".into(), 1_i64, 2_i64),
        ("revision".into(), "INTEGER".into(), 1_i64, 0_i64),
        ("data".into(), "BLOB".into(), 1_i64, 0_i64),
    ];
    if columns.as_slice() != expected.as_slice() {
        return Err(failed());
    }
    drop(statement);
    Ok(conn)
}

fn select(conn: &Connection, project: &str, key: &str)
    -> Result<Option<ProjectRecord>, ProjectStorageError>
{
    let row: Option<(i64, Vec<u8>)> = conn
        .query_row(READ, params![project, key], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()
        .map_err(|_| failed())?;
    row.map(|(revision, bytes)| {
        let revision = u64::try_from(revision).map_err(|_| failed())?;
        if revision == 0 || bytes.len() > MAX_BLOB_BYTES {
            return Err(failed());
        }
        Ok(ProjectRecord {
            revision,
            data: phenix_sdk::Blob(bytes),
        })
    })
    .transpose()
}

/// SQLite BEGIN IMMEDIATE serializes revisions across independent processes.
/// Reject the whole change set before commit if any precondition fails.
pub fn transact(request: ProjectTransaction)
    -> Result<Vec<ProjectRecord>, ProjectStorageError>
{
    if !phenix_sdk::identifier(&request.project)
        || request.edits.len() > MAX_EDITS
    {
        return Err(invalid());
    }
    let mut keys = BTreeSet::new();
    for edit in &request.edits {
        if !valid(&request.project, &edit.key)
            || !keys.insert(edit.key.as_str())
            || edit.data.0.len() > MAX_BLOB_BYTES
        {
            return Err(invalid());
        }
        version_i64(edit.expected.checked_add(1).ok_or_else(|| {
            error(ProjectStorageProblem::Conflict, "revision overflow")
        })?)?;
    }
    let mut conn = open()?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| failed())?;
    let changes = (|| {
        let mut output = Vec::new();
        for ProjectEdit {
            key,
            expected,
            data,
        } in request.edits
        {
            let actual = select(&transaction, &request.project, &key)?;
            let previous = actual.map_or(0, |record| record.revision);
            if previous != expected {
                return Err(error(ProjectStorageProblem::Conflict, "project revision conflict"));
            }
            let revision = expected + 1;
            transaction.execute(
                UPSERT,
                params![&request.project, &key, version_i64(revision)?, &data.0],
            ).map_err(|_| failed())?;
            output.push(ProjectRecord { revision, data });
        }
        Ok(output)
    })();
    match changes {
        Ok(records) => {
            transaction.commit().map_err(|_| failed())?;
            Ok(records)
        }
        Err(error) => {
            let _ = transaction.rollback();
            Err(error)
        }
    }
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.project-storage-sqlite",
    version = "1.0.0",
    provide(phenix_advanced_contracts::ProjectStorage, slot = "ProjectStorage")
)]
mod plugin {
    use super::{error, failed, invalid, open, select, transact, valid};
    use phenix_advanced_contracts::{
        ProjectRecord, ProjectRecordKey, ProjectStorage, ProjectStorageError,
        ProjectStorageProblem, ProjectTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    fn authorized(host: &Host<'_>, project: &str)
        -> CallResult<(), ProjectStorageError>
    {
        if !phenix_sdk::identifier(project) {
            return Err(CallError::Domain(invalid()));
        }
        let resource = format!("phenix.project.{project}");
        if host.resource()?.as_deref() != Some(resource.as_str()) {
            return Err(CallError::Domain(error(
                ProjectStorageProblem::Denied,
                "project resource is not authorized",
            )));
        }
        Ok(())
    }

    #[provide(ProjectStorage::read)]
    fn read(_: &mut State, host: &Host<'_>, key: ProjectRecordKey)
        -> CallResult<ProjectRecord, ProjectStorageError>
    {
        authorized(host, &key.project)?;
        if !valid(&key.project, &key.key) {
            return Err(CallError::Domain(invalid()));
        }
        let conn = open().map_err(CallError::Domain)?;
        select(&conn, &key.project, &key.key)
            .and_then(|record| {
                record.ok_or_else(|| error(ProjectStorageProblem::NotFound, "project record missing"))
            })
            .map_err(CallError::Domain)
    }

    #[provide(ProjectStorage::transact)]
    fn write(_: &mut State, host: &Host<'_>, edit: ProjectTransaction)
        -> CallResult<Vec<ProjectRecord>, ProjectStorageError>
    {
        authorized(host, &edit.project)?;
        transact(edit).map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_and_invalid_edits_fail_before_opening_a_database() {
        let invalid = ProjectTransaction {
            project: "alpha".into(),
            edits: vec![
                ProjectEdit { key: "same".into(), expected: 0, data: phenix_sdk::Blob(vec![1]) },
                ProjectEdit { key: "same".into(), expected: 0, data: phenix_sdk::Blob(vec![2]) },
            ],
        };
        assert_eq!(transact(invalid).unwrap_err().problem, ProjectStorageProblem::InvalidInput);
        assert!(!valid("alpha", "../escape"));
    }
}
