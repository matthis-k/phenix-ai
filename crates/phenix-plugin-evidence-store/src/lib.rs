//! Durable session evidence over the selected Storage and SourceEvidence providers.
//! Basic's journal remains the authoritative source. Every durable fact is
//! checked through the selected source provider before write and retrieval.
use phenix_advanced_contracts::{
    validate_record, EvidenceError, EvidenceFreshness, EvidenceProblem, EvidenceRecord,
    EvidenceScope, EvidenceScopeKind,
};
use std::collections::BTreeMap;

const RECORD_KEY: &str = "evidence-store.v1";
const MAX_HISTORY: usize = 1024;
const MAX_STATEMENT: usize = 8192;

fn issue(problem: EvidenceProblem, message: impl Into<String>) -> EvidenceError {
    EvidenceError {
        problem,
        message: message.into(),
    }
}

/// The ledger retains every accepted revision. A corrected fact never
/// overwrites the evidence that supported an earlier historical answer.
pub fn append_version(
    history: &[EvidenceRecord],
    proposed: &EvidenceRecord,
) -> Result<Vec<EvidenceRecord>, EvidenceError> {
    validate_record(proposed).map_err(|p| issue(p, "invalid evidence record"))?;
    if !matches!(
        proposed.scope.kind,
        EvidenceScopeKind::Session | EvidenceScopeKind::Project
    ) || !phenix_sdk::identifier(&proposed.scope.id)
        || !phenix_sdk::identifier(&proposed.id)
        || proposed.statement.len() > MAX_STATEMENT
    {
        return Err(issue(
            EvidenceProblem::InvalidIdentity,
            "invalid evidence identity or size",
        ));
    }
    if history.len() >= MAX_HISTORY {
        return Err(issue(
            EvidenceProblem::CapacityExceeded,
            "evidence history is full",
        ));
    }
    validate_ledger(history, &proposed.scope)?;
    if let Some(previous) = latest(history, &proposed.id) {
        let next = previous
            .revision
            .checked_add(1)
            .ok_or_else(|| issue(EvidenceProblem::Conflict, "revision overflow"))?;
        if proposed.revision != next {
            return Err(issue(EvidenceProblem::Conflict, "stale evidence revision"));
        }
        if proposed.valid_from < previous.valid_from
            || previous
                .valid_until
                .is_some_and(|end| proposed.valid_from < end)
        {
            return Err(issue(
                EvidenceProblem::InvalidValidity,
                "overlapping evidence validity",
            ));
        }
    } else if proposed.revision != 1 {
        return Err(issue(
            EvidenceProblem::Conflict,
            "new evidence must begin at revision one",
        ));
    }
    for source in &proposed.sources {
        if source.scope != proposed.scope {
            return Err(issue(
                EvidenceProblem::ScopeDenied,
                "source scope differs from evidence scope",
            ));
        }
    }
    let mut next = history.to_vec();
    next.push(proposed.clone());
    Ok(next)
}

fn latest<'a>(history: &'a [EvidenceRecord], id: &str) -> Option<&'a EvidenceRecord> {
    history
        .iter()
        .filter(|record| record.id == id)
        .max_by_key(|record| record.revision)
}

pub fn validate_ledger(
    history: &[EvidenceRecord],
    scope: &EvidenceScope,
) -> Result<(), EvidenceError> {
    if history.len() > MAX_HISTORY {
        return Err(issue(
            EvidenceProblem::StorageFailed,
            "ledger record limit exceeded",
        ));
    }
    let mut revisions: BTreeMap<&str, u64> = BTreeMap::new();
    for entry in history {
        validate_record(entry)
            .map_err(|_| issue(EvidenceProblem::StorageFailed, "invalid persisted evidence"))?;
        if entry.scope != *scope {
            return Err(issue(
                EvidenceProblem::StorageFailed,
                "persisted scope mismatch",
            ));
        }
        let last = revisions.get(entry.id.as_str()).copied().unwrap_or(0);
        if last.checked_add(1) != Some(entry.revision) {
            return Err(issue(
                EvidenceProblem::StorageFailed,
                "missing or repeated evidence revision",
            ));
        }
        revisions.insert(&entry.id, entry.revision);
    }
    Ok(())
}

pub fn select_records(
    history: &[EvidenceRecord],
    as_of: Option<u64>,
    text: &str,
    limit: u64,
) -> Result<Vec<EvidenceRecord>, EvidenceError> {
    if !(1..=128).contains(&limit) {
        return Err(issue(
            EvidenceProblem::InvalidRange,
            "search limit out of range",
        ));
    }
    let mut selected = BTreeMap::<&str, &EvidenceRecord>::new();
    for entry in history {
        if as_of.is_some_and(|when| !entry.valid_at(when)) {
            continue;
        }
        let should_replace = selected
            .get(entry.id.as_str())
            .is_none_or(|prev| prev.revision < entry.revision);
        if should_replace {
            selected.insert(entry.id.as_str(), entry);
        }
    }
    let query = text.to_lowercase();
    let matching = selected
        .values()
        .filter(|entry| {
            entry.freshness != EvidenceFreshness::Withdrawn
                && entry.statement.to_lowercase().contains(&query)
        })
        .copied()
        .collect::<Vec<_>>();
    // The v1 request has no incoming cursor. Never silently truncate results.
    if matching.len() > limit as usize {
        return Err(issue(
            EvidenceProblem::CapacityExceeded,
            "result exceeds bounded search limit",
        ));
    }
    Ok(matching.into_iter().cloned().collect())
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.evidence-store",
    version = "1.0.0",
    // A SourceEvidence import calls Journal, which in turn calls Storage.
    // These definitions allow transitive delegation without direct bindings.
    define(phenix_basic_contracts::EventJournal),
    provide(phenix_advanced_contracts::EvidenceStore, slot = "Store"),
    require(phenix_advanced_contracts::SourceEvidence, slot = "Source"),
    require(phenix_basic_contracts::Storage, slot = "Storage"),
    require(phenix_advanced_contracts::ProjectStorage, slot = "ProjectStorage"),
    require(phenix_advanced_contracts::ProjectMembership, slot = "Membership"),
    require(phenix_advanced_contracts::SourceEvidence, slot = "ProjectSource")
)]
mod plugin {
    use super::{append_version, issue, latest, select_records, validate_ledger, RECORD_KEY};
    use phenix_advanced_contracts::{
        compare_source, decode, encode, EvidenceError, EvidenceFreshness, EvidenceKey,
        EvidencePage, EvidenceProblem, EvidenceRecord, EvidenceScope, EvidenceScopeKind,
        EvidenceSearch, EvidenceStore, ProjectEdit, ProjectMemberKey, ProjectMembership,
        ProjectRecord, ProjectRecordKey, ProjectStorage, ProjectStorageError,
        ProjectStorageProblem, ProjectTransaction, SourceEvidence, SourceMatch, SourceRef,
    };
    use phenix_basic_contracts::{
        BasicError, ErrorCode, RecordKey, Storage, StorageEdit, StorageTransaction, StoredRecord,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};

    fn authorize(host: &Host<'_>, scope: &EvidenceScope) -> CallResult<(), EvidenceError> {
        if !phenix_sdk::identifier(&scope.id) {
            return Err(CallError::Domain(issue(
                EvidenceProblem::InvalidIdentity,
                "invalid evidence scope",
            )));
        }
        match scope.kind {
            EvidenceScopeKind::Session => phenix_basic_contracts::check_session(host, &scope.id)
                .map_err(|err| match err {
                    CallError::Kernel(failure) => CallError::Kernel(failure),
                    CallError::Domain(_) => CallError::Domain(issue(
                        EvidenceProblem::ScopeDenied,
                        "session evidence is not authorized",
                    )),
                }),
            EvidenceScopeKind::Project => {
                let resource = host.resource()?.unwrap_or_default();
                let Some(session) = resource.strip_prefix("phenix.session.") else {
                    return Err(CallError::Domain(issue(
                        EvidenceProblem::ScopeDenied,
                        "project evidence requires session authority",
                    )));
                };
                if !phenix_sdk::identifier(session) {
                    return Err(CallError::Domain(issue(
                        EvidenceProblem::ScopeDenied,
                        "invalid session authority",
                    )));
                }
                let member = host
                    .invoke::<ProjectMembership::get>(
                        "Membership",
                        &ProjectMemberKey {
                            project: scope.id.clone(),
                            session: session.into(),
                        },
                    )
                    .map_err(|err| match err {
                        CallError::Kernel(failure) => CallError::Kernel(failure),
                        CallError::Domain(_) => CallError::Domain(issue(
                            EvidenceProblem::ScopeDenied,
                            "project membership denied",
                        )),
                    })?;
                if member.active {
                    Ok(())
                } else {
                    Err(CallError::Domain(issue(
                        EvidenceProblem::ScopeDenied,
                        "project membership revoked",
                    )))
                }
            }
            _ => Err(CallError::Domain(issue(
                EvidenceProblem::ScopeDenied,
                "unsupported evidence scope",
            ))),
        }
    }

    fn storage_failure(error: CallError<BasicError>) -> CallError<EvidenceError> {
        match error {
            CallError::Kernel(failure) => CallError::Kernel(failure),
            CallError::Domain(error) => {
                let code = match error.code {
                    ErrorCode::NotFound => EvidenceProblem::NotFound,
                    ErrorCode::Conflict => EvidenceProblem::Conflict,
                    ErrorCode::Denied => EvidenceProblem::ScopeDenied,
                    _ => EvidenceProblem::StorageFailed,
                };
                CallError::Domain(issue(code, "durable evidence storage failed"))
            }
        }
    }

    fn project_failure(error: CallError<ProjectStorageError>) -> CallError<EvidenceError> {
        match error {
            CallError::Kernel(failure) => CallError::Kernel(failure),
            CallError::Domain(error) => CallError::Domain(issue(
                match error.problem {
                    ProjectStorageProblem::NotFound => EvidenceProblem::NotFound,
                    ProjectStorageProblem::Conflict => EvidenceProblem::Conflict,
                    ProjectStorageProblem::Denied => EvidenceProblem::ScopeDenied,
                    _ => EvidenceProblem::StorageFailed,
                },
                "project evidence storage failed",
            )),
        }
    }

    fn load(
        host: &Host<'_>,
        scope: &EvidenceScope,
    ) -> CallResult<(u64, Vec<EvidenceRecord>), EvidenceError> {
        authorize(host, scope)?;
        let stored = match scope.kind {
            EvidenceScopeKind::Session => {
                let key = RecordKey {
                    session: scope.id.clone(),
                    key: RECORD_KEY.into(),
                };
                match host.invoke::<Storage::read>("Storage", &key) {
                    Ok(StoredRecord { revision, data }) => Some((revision, data)),
                    Err(CallError::Domain(err)) if err.code == ErrorCode::NotFound => None,
                    Err(error) => return Err(storage_failure(error)),
                }
            }
            EvidenceScopeKind::Project => {
                let key = ProjectRecordKey {
                    project: scope.id.clone(),
                    key: RECORD_KEY.into(),
                };
                match host.invoke::<ProjectStorage::read>("ProjectStorage", &key) {
                    Ok(ProjectRecord { revision, data }) => Some((revision, data)),
                    Err(CallError::Domain(err))
                        if err.problem == ProjectStorageProblem::NotFound =>
                    {
                        None
                    }
                    Err(error) => return Err(project_failure(error)),
                }
            }
            _ => {
                return Err(CallError::Domain(issue(
                    EvidenceProblem::ScopeDenied,
                    "unsupported scope",
                )))
            }
        };
        let (revision, history) = match stored {
            Some((revision, bytes)) => (
                revision,
                decode::<Vec<EvidenceRecord>>(&bytes).map_err(|_| {
                    CallError::Domain(issue(
                        EvidenceProblem::StorageFailed,
                        "invalid persisted ledger bytes",
                    ))
                })?,
            ),
            None => (0, vec![]),
        };
        validate_ledger(&history, scope).map_err(CallError::Domain)?;
        Ok((revision, history))
    }

    fn verify_sources(
        host: &Host<'_>,
        scope: &EvidenceScope,
        sources: &[SourceRef],
    ) -> CallResult<(), EvidenceError> {
        for source in sources {
            if source.scope != *scope {
                return Err(CallError::Domain(issue(
                    EvidenceProblem::ScopeDenied,
                    "evidence source is out of scope",
                )));
            }
            let slot = if scope.kind == EvidenceScopeKind::Project {
                "ProjectSource"
            } else {
                "Source"
            };
            let actual = host.invoke::<SourceEvidence::observe>(slot, source)?;
            if compare_source(source, &actual) != SourceMatch::Current {
                return Err(CallError::Domain(issue(
                    EvidenceProblem::SourceChanged,
                    "evidence source was changed",
                )));
            }
        }
        Ok(())
    }

    #[provide(EvidenceStore::put)]
    fn put(host: &Host<'_>, record: EvidenceRecord) -> CallResult<EvidenceRecord, EvidenceError> {
        authorize(host, &record.scope)?;
        // Reject invalid records before any imported effect.
        phenix_advanced_contracts::validate_record(&record)
            .map_err(|p| CallError::Domain(issue(p, "invalid evidence record")))?;
        let (revision, history) = load(host, &record.scope)?;
        let updated = append_version(&history, &record).map_err(CallError::Domain)?;
        verify_sources(host, &record.scope, &record.sources)?;
        let data = encode(&updated).map_err(CallError::Kernel)?;
        let (actual_revision, actual_data) = match record.scope.kind {
            EvidenceScopeKind::Session => {
                let records = host
                    .invoke::<Storage::transact>(
                        "Storage",
                        &StorageTransaction {
                            session: record.scope.id.clone(),
                            edits: vec![StorageEdit {
                                key: RECORD_KEY.into(),
                                expected: revision,
                                data: data.clone(),
                            }],
                        },
                    )
                    .map_err(storage_failure)?;
                if records.len() != 1 {
                    return Err(CallError::Kernel(Failure::new(
                        Code::PluginFailed,
                        "invalid session evidence CAS reply",
                    )));
                }
                (records[0].revision, records[0].data.clone())
            }
            EvidenceScopeKind::Project => {
                let records = host
                    .invoke::<ProjectStorage::transact>(
                        "ProjectStorage",
                        &ProjectTransaction {
                            project: record.scope.id.clone(),
                            edits: vec![ProjectEdit {
                                key: RECORD_KEY.into(),
                                expected: revision,
                                data: data.clone(),
                            }],
                        },
                    )
                    .map_err(project_failure)?;
                if records.len() != 1 {
                    return Err(CallError::Kernel(Failure::new(
                        Code::PluginFailed,
                        "invalid project evidence CAS reply",
                    )));
                }
                (records[0].revision, records[0].data.clone())
            }
            _ => {
                return Err(CallError::Domain(issue(
                    EvidenceProblem::ScopeDenied,
                    "unsupported scope",
                )))
            }
        };
        if actual_revision != revision + 1 || actual_data != data {
            return Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "storage returned invalid evidence CAS acknowledgment",
            )));
        }
        Ok(record)
    }

    #[provide(EvidenceStore::get)]
    fn get(host: &Host<'_>, key: EvidenceKey) -> CallResult<EvidenceRecord, EvidenceError> {
        authorize(host, &key.scope)?;
        if !phenix_sdk::identifier(&key.id) {
            return Err(CallError::Domain(issue(
                EvidenceProblem::InvalidIdentity,
                "invalid record ID",
            )));
        }
        let (_, history) = load(host, &key.scope)?;
        let selected = latest(&history, &key.id).ok_or_else(|| {
            CallError::Domain(issue(EvidenceProblem::NotFound, "evidence record missing"))
        })?;
        if selected.freshness == EvidenceFreshness::Withdrawn {
            return Err(CallError::Domain(issue(
                EvidenceProblem::NotFound,
                "evidence was withdrawn",
            )));
        }
        verify_sources(host, &key.scope, &selected.sources)?;
        Ok(selected.clone())
    }

    #[provide(EvidenceStore::search)]
    fn search(host: &Host<'_>, request: EvidenceSearch) -> CallResult<EvidencePage, EvidenceError> {
        if request.scopes.len() != 1 {
            return Err(CallError::Domain(issue(
                EvidenceProblem::ScopeDenied,
                "search requires one authorized session scope",
            )));
        }
        let scope = &request.scopes[0];
        let (_, history) = load(host, scope)?;
        let records = select_records(&history, request.as_of, &request.query, request.limit)
            .map_err(CallError::Domain)?;
        for record in &records {
            verify_sources(host, scope, &record.sources)?;
        }
        Ok(EvidencePage {
            records,
            next: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_advanced_contracts::{EvidenceKind, SourceRef};

    fn scope() -> EvidenceScope {
        EvidenceScope {
            kind: EvidenceScopeKind::Session,
            id: "s1".into(),
        }
    }
    fn record(revision: u64) -> EvidenceRecord {
        EvidenceRecord {
            id: "fact-color".into(),
            scope: scope(),
            kind: EvidenceKind::Fact,
            statement: if revision == 1 { "violet" } else { "blue" }.into(),
            sources: vec![SourceRef {
                owner: "phenix.basic.event-journal".into(),
                resource: "phenix.session.s1".into(),
                revision: "3".into(),
                digest_sha256: "a".repeat(64),
                range_start: 0,
                range_end: 0,
                scope: scope(),
            }],
            revision,
            observed_at: revision * 10,
            valid_from: if revision == 1 { 1 } else { 100 },
            valid_until: if revision == 1 { Some(100) } else { None },
            confidence_basis_points: 10000,
            freshness: EvidenceFreshness::Current,
            supersedes: vec![],
        }
    }
    #[test]
    fn cas_append_preserves_history_and_detects_conflict() {
        let first = append_version(&[], &record(1)).unwrap();
        assert_eq!(first.len(), 1);
        let updated = append_version(&first, &record(2)).unwrap();
        assert_eq!(updated.len(), 2);
        assert_eq!(latest(&updated, "fact-color").unwrap().revision, 2);
        assert_eq!(
            append_version(&updated, &record(2)).unwrap_err().problem,
            EvidenceProblem::Conflict
        );
        assert_eq!(
            select_records(&updated, Some(50), "", 10).unwrap()[0].statement,
            "violet"
        );
        assert_eq!(
            select_records(&updated, None, "", 10).unwrap()[0].statement,
            "blue"
        );
    }
    #[test]
    fn a_withdrawn_revision_cannot_resurface_in_current_search() {
        let first = append_version(&[], &record(1)).unwrap();
        let second = append_version(&first, &record(2)).unwrap();
        let mut withdrawn = record(3);
        withdrawn.freshness = EvidenceFreshness::Withdrawn;
        withdrawn.valid_from = 200;
        let history = append_version(&second, &withdrawn).unwrap();
        assert!(select_records(&history, None, "", 10).unwrap().is_empty());
        assert_eq!(
            select_records(&history, Some(50), "", 10).unwrap()[0].statement,
            "violet"
        );
    }

    #[test]
    fn invalid_history_and_sources_do_not_become_records() {
        let mut bad = record(1);
        bad.sources[0].scope.id = "s2".into();
        assert_eq!(
            append_version(&[], &bad).unwrap_err().problem,
            EvidenceProblem::ScopeDenied
        );
        bad = record(1);
        bad.sources.clear();
        assert_eq!(
            append_version(&[], &bad).unwrap_err().problem,
            EvidenceProblem::MissingSource
        );
        let mut corrupted = vec![record(1)];
        corrupted.push(record(1));
        assert_eq!(
            validate_ledger(&corrupted, &scope()).unwrap_err().problem,
            EvidenceProblem::StorageFailed
        );
    }
}
