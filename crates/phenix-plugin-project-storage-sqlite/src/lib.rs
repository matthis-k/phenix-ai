//! Project-scoped, revision-checked SQLite records.
//! Reuses rusqlite and the Basic database file, but does not expose Basic
//! session records or modify its storage contract or schema version.
use phenix_advanced_contracts::{
    ProjectEdit, ProjectRecord, ProjectStorageError, ProjectStorageProblem, ProjectTransaction,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
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
    transact_scoped(request, None)
}

/// Membership and data edits use one IMMEDIATE transaction. Revocation
/// cannot interleave between authorization and the committed change.
fn transact_scoped(
    request: ProjectTransaction,
    session: Option<&str>,
) -> Result<Vec<ProjectRecord>, ProjectStorageError> {
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
        if let Some(session) = session {
            let membership =
                membership_read(&transaction, &request.project, session).map_err(|_| {
                    error(
                        ProjectStorageProblem::StorageFailed,
                        "membership lookup failed",
                    )
                })?;
            if !membership.is_some_and(|member| member.active) {
                return Err(error(
                    ProjectStorageProblem::Denied,
                    "project write requires active membership",
                ));
            }
        }
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

const PROJECT_OWNER: &str = "phenix.advanced.project-storage-sqlite";

fn source_error(
    problem: phenix_advanced_contracts::EvidenceProblem,
    message: &str,
) -> phenix_advanced_contracts::EvidenceError {
    phenix_advanced_contracts::EvidenceError {
        problem,
        message: message.into(),
    }
}

/// Create a source reference to one exact project-record revision. The
/// source provider returns the live revision and digest on each observation.
pub fn project_record_reference(
    project: &str,
    key: &str,
    record: &phenix_advanced_contracts::ProjectRecord,
) -> Result<phenix_advanced_contracts::SourceRef, phenix_advanced_contracts::EvidenceError> {
    use phenix_advanced_contracts::{EvidenceProblem, EvidenceScope, EvidenceScopeKind, SourceRef};
    if !valid(project, key) || key.contains('@') || record.revision == 0 {
        return Err(source_error(
            EvidenceProblem::InvalidIdentity,
            "invalid project source identity",
        ));
    }
    let digest = Sha256::digest(&record.data.0);
    Ok(SourceRef {
        owner: PROJECT_OWNER.into(),
        resource: format!("phenix.project.{project}"),
        revision: format!("{key}@{}", record.revision),
        digest_sha256: format!("{digest:x}"),
        range_start: 0,
        range_end: 0,
        scope: EvidenceScope {
            kind: EvidenceScopeKind::Project,
            id: project.into(),
        },
    })
}

fn parse_project_source(
    reference: &phenix_advanced_contracts::SourceRef,
) -> Result<&str, phenix_advanced_contracts::EvidenceError> {
    use phenix_advanced_contracts::{validate_reference, EvidenceProblem, EvidenceScopeKind};
    validate_reference(reference).map_err(|p| source_error(p, "invalid source reference"))?;
    if reference.owner != PROJECT_OWNER
        || reference.scope.kind != EvidenceScopeKind::Project
        || !phenix_sdk::identifier(&reference.scope.id)
        || reference.resource != format!("phenix.project.{}", reference.scope.id)
        || reference.range_start != 0
        || reference.range_end != 0
    {
        return Err(source_error(
            EvidenceProblem::InvalidIdentity,
            "source is not a project record",
        ));
    }
    let (key, revision) = reference.revision.rsplit_once('@').ok_or_else(|| {
        source_error(EvidenceProblem::InvalidIdentity, "invalid project revision")
    })?;
    let parsed = revision
        .parse::<u64>()
        .map_err(|_| source_error(EvidenceProblem::InvalidIdentity, "invalid source revision"))?;
    if !phenix_sdk::identifier(key)
        || key.contains('@')
        || parsed == 0
        || parsed.to_string() != revision
    {
        return Err(source_error(
            EvidenceProblem::InvalidIdentity,
            "invalid project record revision",
        ));
    }
    Ok(key)
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.project-storage-sqlite",
    version = "1.0.0",
    provide(phenix_advanced_contracts::ProjectStorage, slot = "ProjectStorage"),
    provide(phenix_advanced_contracts::ProjectMembership, slot = "Membership"),
    provide(phenix_advanced_contracts::SourceEvidence, slot = "ProjectSource")
)]
mod plugin {
    use super::{
        error, invalid, membership_denied, membership_failed, membership_invalid, membership_read,
        open, parse_project_source, project_record_reference, select, set_membership, source_error,
        transact, transact_scoped, valid,
    };
    use phenix_advanced_contracts::{
        EvidenceError, EvidenceProblem, ProjectMember, ProjectMemberChange, ProjectMemberKey,
        ProjectMembership, ProjectMembershipError, ProjectRecord, ProjectRecordKey, ProjectStorage,
        ProjectStorageError, ProjectStorageProblem, ProjectTransaction, SourceEvidence,
        SourceObservation, SourceRef,
    };
    use phenix_sdk::{CallError, CallResult, Host};
    use rusqlite::TransactionBehavior;

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

    fn authorized_project_read(
        conn: &rusqlite::Connection,
        host: &Host<'_>,
        project: &str,
    ) -> CallResult<(), ProjectStorageError> {
        let resource = host.resource()?.unwrap_or_default();
        if resource == format!("phenix.project.{project}") {
            return Ok(());
        }
        let membership = match resource.strip_prefix("phenix.session.") {
            Some(session) if phenix_sdk::identifier(session) => {
                membership_read(conn, project, session).map_err(|_| {
                    CallError::Domain(error(
                        ProjectStorageProblem::StorageFailed,
                        "project membership lookup failed",
                    ))
                })?
            }
            _ => None,
        };
        if membership.is_some_and(|record| record.active) {
            Ok(())
        } else {
            Err(CallError::Domain(error(
                ProjectStorageProblem::Denied,
                "project read requires active membership",
            )))
        }
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

    #[provide(SourceEvidence::observe)]
    fn observe(
        _: &mut State,
        host: &Host<'_>,
        reference: SourceRef,
    ) -> CallResult<SourceObservation, EvidenceError> {
        let key = parse_project_source(&reference).map_err(CallError::Domain)?;
        let mut conn = open().map_err(|_| {
            CallError::Domain(source_error(
                EvidenceProblem::StorageFailed,
                "project source storage failed",
            ))
        })?;
        // The membership check and source read share one SQLite snapshot.
        // A concurrent revocation cannot commit between those two reads.
        let txn = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|_| {
                CallError::Domain(source_error(
                    EvidenceProblem::StorageFailed,
                    "project source read transaction failed",
                ))
            })?;
        authorized_project_read(&txn, host, &reference.scope.id).map_err(|error| match error {
            CallError::Kernel(failure) => CallError::Kernel(failure),
            CallError::Domain(inner) => CallError::Domain(source_error(
                if inner.problem == ProjectStorageProblem::Denied {
                    EvidenceProblem::ScopeDenied
                } else {
                    EvidenceProblem::StorageFailed
                },
                "project source access denied or unavailable",
            )),
        })?;
        let actual = select(&txn, &reference.scope.id, key)
            .map_err(|_| {
                CallError::Domain(source_error(
                    EvidenceProblem::StorageFailed,
                    "invalid project source storage",
                ))
            })?
            .ok_or_else(|| {
                CallError::Domain(source_error(
                    EvidenceProblem::NotFound,
                    "project source not found",
                ))
            })?;
        let current = project_record_reference(&reference.scope.id, key, &actual)
            .map_err(CallError::Domain)?;
        Ok(SourceObservation {
            owner: current.owner,
            resource: current.resource,
            revision: current.revision,
            digest_sha256: current.digest_sha256,
        })
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
        let mut conn = open().map_err(CallError::Domain)?;
        // Freeze authorization and record data in the same snapshot.
        let txn = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|_| {
                CallError::Domain(error(
                    ProjectStorageProblem::StorageFailed,
                    "project read transaction failed",
                ))
            })?;
        authorized_project_read(&txn, host, &key.project)?;
        select(&txn, &key.project, &key.key)
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
        if !phenix_sdk::identifier(&edit.project) {
            return Err(CallError::Domain(invalid()));
        }
        let current = host.resource()?.unwrap_or_default();
        if current == format!("phenix.project.{}", edit.project) {
            transact(edit).map_err(CallError::Domain)
        } else if let Some(session) = current.strip_prefix("phenix.session.") {
            if !phenix_sdk::identifier(session) {
                return Err(CallError::Domain(error(
                    ProjectStorageProblem::Denied,
                    "invalid session resource",
                )));
            }
            transact_scoped(edit, Some(session)).map_err(CallError::Domain)
        } else {
            Err(CallError::Domain(error(
                ProjectStorageProblem::Denied,
                "project write is not authorized",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_source_read_keeps_membership_and_data_in_one_snapshot() {
        // WAL allows a revocation to commit while a read is in flight.
        // The reader must observe one consistent membership and record revision.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "phenix-project-snapshot-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let seeded = Connection::open(&path).unwrap();
        seeded.execute_batch("PRAGMA journal_mode=WAL").unwrap();
        seeded.execute_batch(TABLE).unwrap();
        seeded.execute_batch(MEMBERSHIP_TABLE).unwrap();
        seeded.execute(MEMBER_UPSERT, params!["alpha", "s1", 1_i64, 1_i64]).unwrap();
        seeded.execute(UPSERT, params!["alpha", "theme", 1_i64, b"blue"]).unwrap();

        let mut reader = Connection::open(&path).unwrap();
        let snapshot = reader
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .unwrap();
        assert!(membership_read(&snapshot, "alpha", "s1").unwrap().unwrap().active);
        let writer = Connection::open(&path).unwrap();
        writer.execute(MEMBER_UPSERT, params!["alpha", "s1", 2_i64, 0_i64]).unwrap();

        assert!(membership_read(&snapshot, "alpha", "s1").unwrap().unwrap().active);
        let record = select(&snapshot, "alpha", "theme").unwrap().unwrap();
        assert_eq!(record.revision, 1);
        assert_eq!(record.data.0, b"blue");
        drop(snapshot);
        let revoked = membership_read(&reader, "alpha", "s1").unwrap().unwrap();
        assert_eq!(revoked.revision, 2);
        assert!(!revoked.active);
        drop(reader);
        drop(writer);
        drop(seeded);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

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
