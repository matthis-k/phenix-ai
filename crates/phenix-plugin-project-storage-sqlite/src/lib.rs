//! Project-scoped, revision-checked SQLite records.
//! Reuses rusqlite and the Basic database file, but does not expose Basic
//! session records or modify its storage contract or schema version.
use phenix_advanced_contracts::{
    ProjectEdit, ProjectRecord, ProjectStorageError, ProjectStorageProblem, ProjectTransaction,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::{collections::BTreeSet, env, time::Duration};

const TABLE: &str = "CREATE TABLE IF NOT EXISTS project_records (project TEXT NOT NULL, name TEXT NOT NULL, revision INTEGER NOT NULL CHECK (revision > 0), data BLOB NOT NULL, PRIMARY KEY (project, name)) WITHOUT ROWID";
const MEMBERSHIP_TABLE: &str = "CREATE TABLE IF NOT EXISTS project_membership (project TEXT NOT NULL, session TEXT NOT NULL, revision INTEGER NOT NULL CHECK (revision > 0), active INTEGER NOT NULL CHECK (active IN (0, 1)), PRIMARY KEY (project, session)) WITHOUT ROWID";
const MEMBER_READ: &str =
    "SELECT revision, active FROM project_membership WHERE project = ?1 AND session = ?2";
const MEMBER_UPSERT: &str = "INSERT INTO project_membership (project, session, revision, active) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(project, session) DO UPDATE SET revision = excluded.revision, active = excluded.active";
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
    error(
        ProjectStorageProblem::StorageFailed,
        "project SQLite operation failed",
    )
}
fn invalid() -> ProjectStorageError {
    error(
        ProjectStorageProblem::InvalidInput,
        "invalid project record request",
    )
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
    conn.execute_batch(MEMBERSHIP_TABLE).map_err(|_| failed())?;
    let mut statement = conn
        .prepare("PRAGMA table_info(project_records)")
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

fn select(
    conn: &Connection,
    project: &str,
    key: &str,
) -> Result<Option<ProjectRecord>, ProjectStorageError> {
    let row: Option<(i64, Vec<u8>)> = conn
        .query_row(READ, params![project, key], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
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
pub fn transact(request: ProjectTransaction) -> Result<Vec<ProjectRecord>, ProjectStorageError> {
    if !phenix_sdk::identifier(&request.project) || request.edits.len() > MAX_EDITS {
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
        version_i64(
            edit.expected
                .checked_add(1)
                .ok_or_else(|| error(ProjectStorageProblem::Conflict, "revision overflow"))?,
        )?;
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
                return Err(error(
                    ProjectStorageProblem::Conflict,
                    "project revision conflict",
                ));
            }
            let revision = expected + 1;
            transaction
                .execute(
                    UPSERT,
                    params![&request.project, &key, version_i64(revision)?, &data.0],
                )
                .map_err(|_| failed())?;
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

fn membership_error(
    problem: phenix_advanced_contracts::ProjectMembershipProblem,
    message: &str,
) -> phenix_advanced_contracts::ProjectMembershipError {
    phenix_advanced_contracts::ProjectMembershipError {
        problem,
        message: message.into(),
    }
}
fn membership_failed() -> phenix_advanced_contracts::ProjectMembershipError {
    membership_error(
        phenix_advanced_contracts::ProjectMembershipProblem::StorageFailed,
        "project membership SQLite operation failed",
    )
}
fn membership_invalid() -> phenix_advanced_contracts::ProjectMembershipError {
    membership_error(
        phenix_advanced_contracts::ProjectMembershipProblem::InvalidInput,
        "invalid membership change",
    )
}
fn membership_denied() -> phenix_advanced_contracts::ProjectMembershipError {
    membership_error(
        phenix_advanced_contracts::ProjectMembershipProblem::Denied,
        "project membership is not authorized",
    )
}
fn membership_read(
    conn: &Connection,
    project: &str,
    session: &str,
) -> Result<
    Option<phenix_advanced_contracts::ProjectMember>,
    phenix_advanced_contracts::ProjectMembershipError,
> {
    let row: Option<(i64, i64)> = conn
        .query_row(MEMBER_READ, params![project, session], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .map_err(|_| membership_failed())?;
    row.map(|(revision, active)| {
        let revision = u64::try_from(revision).map_err(|_| membership_failed())?;
        if revision == 0 || (active != 0 && active != 1) {
            return Err(membership_failed());
        }
        Ok(phenix_advanced_contracts::ProjectMember {
            project: project.into(),
            session: session.into(),
            revision,
            active: active == 1,
        })
    })
    .transpose()
}

/// Serialized compare-and-swap preserves grants and revocations across
/// processes. Only the project-authorized host may call this operation.
pub fn set_membership(
    change: phenix_advanced_contracts::ProjectMemberChange,
) -> Result<
    phenix_advanced_contracts::ProjectMember,
    phenix_advanced_contracts::ProjectMembershipError,
> {
    use phenix_advanced_contracts::{ProjectMember, ProjectMembershipProblem};
    if !valid(&change.project, &change.session) {
        return Err(membership_invalid());
    }
    let revision = change
        .expected
        .checked_add(1)
        .and_then(|next| i64::try_from(next).ok())
        .ok_or_else(|| {
            membership_error(
                ProjectMembershipProblem::Conflict,
                "membership revision overflow",
            )
        })?;
    let mut conn = open().map_err(|_| membership_failed())?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| membership_failed())?;
    let old = membership_read(&transaction, &change.project, &change.session)?;
    let previous = old.as_ref().map_or(0, |record| record.revision);
    if previous != change.expected {
        return Err(membership_error(
            ProjectMembershipProblem::Conflict,
            "membership revision conflict",
        ));
    }
    if previous == 0 && !change.active {
        return Err(membership_denied());
    }
    transaction
        .execute(
            MEMBER_UPSERT,
            params![
                &change.project,
                &change.session,
                revision,
                i64::from(change.active)
            ],
        )
        .map_err(|_| membership_failed())?;
    transaction.commit().map_err(|_| membership_failed())?;
    Ok(ProjectMember {
        project: change.project,
        session: change.session,
        revision: u64::try_from(revision).map_err(|_| membership_failed())?,
        active: change.active,
    })
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.project-storage-sqlite",
    version = "1.0.0",
    provide(phenix_advanced_contracts::ProjectStorage, slot = "ProjectStorage"),
    provide(phenix_advanced_contracts::ProjectMembership, slot = "Membership")
)]
mod plugin {
    use super::{
        error, invalid, membership_denied, membership_failed, membership_invalid, membership_read,
        open, select, set_membership, transact, valid,
    };
    use phenix_advanced_contracts::{
        ProjectMember, ProjectMemberChange, ProjectMemberKey, ProjectMembership,
        ProjectMembershipError, ProjectRecord, ProjectRecordKey, ProjectStorage,
        ProjectStorageError, ProjectStorageProblem, ProjectTransaction,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    fn authorized(host: &Host<'_>, project: &str) -> CallResult<(), ProjectStorageError> {
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

    #[provide(ProjectMembership::set)]
    fn set(
        _: &mut State,
        host: &Host<'_>,
        change: ProjectMemberChange,
    ) -> CallResult<ProjectMember, ProjectMembershipError> {
        if !valid(&change.project, &change.session)
            || host.resource()?.as_deref()
                != Some(format!("phenix.project.{}", change.project).as_str())
        {
            return Err(CallError::Domain(membership_denied()));
        }
        set_membership(change).map_err(CallError::Domain)
    }

    #[provide(ProjectMembership::get)]
    fn get(
        _: &mut State,
        host: &Host<'_>,
        key: ProjectMemberKey,
    ) -> CallResult<ProjectMember, ProjectMembershipError> {
        if !valid(&key.project, &key.session) {
            return Err(CallError::Domain(membership_invalid()));
        }
        if host.resource()?.as_deref() != Some(format!("phenix.session.{}", key.session).as_str()) {
            return Err(CallError::Domain(membership_denied()));
        }
        let conn = open()
            .map_err(|_| membership_failed())
            .map_err(CallError::Domain)?;
        match membership_read(&conn, &key.project, &key.session).map_err(CallError::Domain)? {
            Some(member) if member.active => Ok(member),
            _ => Err(CallError::Domain(membership_denied())),
        }
    }

    #[provide(ProjectStorage::read)]
    fn read(
        _: &mut State,
        host: &Host<'_>,
        key: ProjectRecordKey,
    ) -> CallResult<ProjectRecord, ProjectStorageError> {
        if !valid(&key.project, &key.key) {
            return Err(CallError::Domain(invalid()));
        }
        let conn = open().map_err(CallError::Domain)?;
        let resource = host.resource()?.unwrap_or_default();
        if resource != format!("phenix.project.{}", key.project) {
            let member = if let Some(session) = resource
                .strip_prefix("phenix.session.")
                .filter(|session| phenix_sdk::identifier(session))
            {
                membership_read(&conn, &key.project, session).map_err(|_| {
                    CallError::Domain(error(
                        ProjectStorageProblem::StorageFailed,
                        "project membership lookup failed",
                    ))
                })?
            } else {
                None
            };
            if !member.is_some_and(|record| record.active) {
                return Err(CallError::Domain(error(
                    ProjectStorageProblem::Denied,
                    "project record read requires active project membership",
                )));
            }
        }
        select(&conn, &key.project, &key.key)
            .and_then(|record| {
                record
                    .ok_or_else(|| error(ProjectStorageProblem::NotFound, "project record missing"))
            })
            .map_err(CallError::Domain)
    }

    #[provide(ProjectStorage::transact)]
    fn write(
        _: &mut State,
        host: &Host<'_>,
        edit: ProjectTransaction,
    ) -> CallResult<Vec<ProjectRecord>, ProjectStorageError> {
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
                ProjectEdit {
                    key: "same".into(),
                    expected: 0,
                    data: phenix_sdk::Blob(vec![1]),
                },
                ProjectEdit {
                    key: "same".into(),
                    expected: 0,
                    data: phenix_sdk::Blob(vec![2]),
                },
            ],
        };
        assert_eq!(
            transact(invalid).unwrap_err().problem,
            ProjectStorageProblem::InvalidInput
        );
        assert!(!valid("alpha", "../escape"));
    }
}
