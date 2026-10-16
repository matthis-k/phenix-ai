//! P2-3c checkpoint metadata, anchored in the authoritative Basic journal.
//! No copied journal bytes, alternate database, model calls, or tool replay.
use phenix_advanced_contracts::{
    validate_checkpoint, CheckpointStoreError, CheckpointStoreProblem, CompactionCheckpoint,
};

const RECORD_KEY: &str = "compaction-checkpoints.v1";
const MAX_CHECKPOINTS: usize = 256;
const MAX_JOURNAL_EVENTS: u64 = 1024;

fn problem(code: CheckpointStoreProblem, message: &str) -> CheckpointStoreError {
    CheckpointStoreError {
        problem: code,
        message: message.into(),
    }
}

/// Fail closed on malformed persisted history. A snapshot cannot repair
/// missing checkpoint revisions, identity collisions or branched lineage.
pub fn validate_ledger(
    history: &[CompactionCheckpoint],
    session: &str,
) -> Result<(), CheckpointStoreProblem> {
    if history.len() > MAX_CHECKPOINTS {
        return Err(CheckpointStoreProblem::StorageFailed);
    }
    for (index, checkpoint) in history.iter().enumerate() {
        let expected = u64::try_from(index)
            .ok()
            .and_then(|number| number.checked_add(1))
            .ok_or(CheckpointStoreProblem::StorageFailed)?;
        let parent = if index == 0 {
            None
        } else {
            Some(history[index - 1].id.as_str())
        };
        if validate_checkpoint(checkpoint).is_err()
            || checkpoint.session != session
            || checkpoint.revision != expected
            || checkpoint.parent_checkpoint.as_deref() != parent
            || history[..index]
                .iter()
                .any(|older| older.id == checkpoint.id)
        {
            return Err(CheckpointStoreProblem::StorageFailed);
        }
    }
    Ok(())
}

/// Testable CAS proposal validation. The provider performs the actual atomic
/// compare-and-swap using Basic Storage::transact after journal revalidation.
pub fn append_checkpoint(
    history: &[CompactionCheckpoint],
    checkpoint: &CompactionCheckpoint,
    session: &str,
) -> Result<Vec<CompactionCheckpoint>, CheckpointStoreProblem> {
    validate_ledger(history, session)?;
    if checkpoint.session != session {
        return Err(CheckpointStoreProblem::ScopeDenied);
    }
    // A persisted ID names exactly one checkpoint. Report attempts to reuse
    // that ID as a revision conflict even if the candidate's own parent
    // reference is structurally invalid (self-parenting).
    if history.iter().any(|prior| prior.id == checkpoint.id) {
        return Err(CheckpointStoreProblem::Conflict);
    }
    validate_checkpoint(checkpoint).map_err(|_| CheckpointStoreProblem::InvalidCheckpoint)?;
    if history.len() >= MAX_CHECKPOINTS {
        return Err(CheckpointStoreProblem::Conflict);
    }
    let next = u64::try_from(history.len())
        .ok()
        .and_then(|number| number.checked_add(1))
        .ok_or(CheckpointStoreProblem::Conflict)?;
    if checkpoint.revision != next
        || checkpoint.parent_checkpoint.as_deref() != history.last().map(|item| item.id.as_str())
        || history.iter().any(|prior| prior.id == checkpoint.id)
    {
        return Err(CheckpointStoreProblem::Conflict);
    }
    let mut updated = history.to_vec();
    updated.push(checkpoint.clone());
    Ok(updated)
}

pub fn find_checkpoint<'a>(
    history: &'a [CompactionCheckpoint],
    id: &str,
) -> Result<&'a CompactionCheckpoint, CheckpointStoreProblem> {
    history
        .iter()
        .find(|checkpoint| checkpoint.id == id)
        .ok_or(CheckpointStoreProblem::NotFound)
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.compaction-checkpoints",
    version = "1.0.0",
    define(phenix_basic_contracts::Storage),
    provide(phenix_advanced_contracts::CompactionStore, slot = "Checkpoints"),
    require(phenix_basic_contracts::Storage, slot = "Storage"),
    require(phenix_basic_contracts::EventJournal, slot = "Journal")
)]
mod plugin {
    use super::{
        append_checkpoint, find_checkpoint, problem, validate_ledger, MAX_JOURNAL_EVENTS,
        RECORD_KEY,
    };
    use phenix_advanced_contracts::{
        validate_checkpoint, CheckpointKey, CheckpointStoreError, CheckpointStoreProblem,
        CompactionCheckpoint, CompactionStore,
    };
    use phenix_basic_contracts::{
        decode, encode, BasicError, ErrorCode, EventJournal, JournalQuery, RecordKey, Storage,
        StorageEdit, StorageTransaction,
    };
    use phenix_p2_compaction_replay::{restore_original_events, ReplayProblem};
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};

    #[derive(Default)]
    pub struct State;

    fn map_storage(error: CallError<BasicError>) -> CallError<CheckpointStoreError> {
        match error {
            CallError::Kernel(failure) => CallError::Kernel(failure),
            CallError::Domain(basic) => CallError::Domain(problem(
                match basic.code {
                    ErrorCode::NotFound => CheckpointStoreProblem::NotFound,
                    ErrorCode::Denied => CheckpointStoreProblem::ScopeDenied,
                    ErrorCode::Conflict => CheckpointStoreProblem::Conflict,
                    _ => CheckpointStoreProblem::StorageFailed,
                },
                "checkpoint dependency denied or failed",
            )),
        }
    }

    fn load(
        host: &Host<'_>,
        session: &str,
    ) -> CallResult<(u64, Vec<CompactionCheckpoint>), CheckpointStoreError> {
        phenix_basic_contracts::check_session(host, session).map_err(map_storage)?;
        let record = host.invoke::<Storage::read>(
            "Storage",
            &RecordKey {
                session: session.into(),
                key: RECORD_KEY.into(),
            },
        );
        let (revision, history) = match record {
            Ok(saved) => {
                let history: Vec<CompactionCheckpoint> = decode(&saved.data).map_err(|_| {
                    CallError::Domain(problem(
                        CheckpointStoreProblem::StorageFailed,
                        "invalid checkpoint ledger encoding",
                    ))
                })?;
                (saved.revision, history)
            }
            Err(CallError::Domain(err)) if err.code == ErrorCode::NotFound => (0, Vec::new()),
            Err(other) => return Err(map_storage(other)),
        };
        validate_ledger(&history, session).map_err(|_| {
            CallError::Domain(problem(
                CheckpointStoreProblem::StorageFailed,
                "invalid durable checkpoint lineage",
            ))
        })?;
        if usize::try_from(revision).ok() != Some(history.len()) {
            return Err(CallError::Domain(problem(
                CheckpointStoreProblem::StorageFailed,
                "checkpoint ledger storage revision mismatch",
            )));
        }
        Ok((revision, history))
    }

    fn verify_journal(
        host: &Host<'_>,
        checkpoint: &CompactionCheckpoint,
    ) -> CallResult<(), CheckpointStoreError> {
        validate_checkpoint(checkpoint).map_err(|_| {
            CallError::Domain(problem(
                CheckpointStoreProblem::InvalidCheckpoint,
                "invalid checkpoint structure",
            ))
        })?;
        let span = &checkpoint.input;
        let count = span
            .last_sequence
            .checked_sub(span.first_sequence)
            .and_then(|delta| delta.checked_add(1))
            .ok_or_else(|| {
                CallError::Domain(problem(
                    CheckpointStoreProblem::InvalidCheckpoint,
                    "invalid journal range",
                ))
            })?;
        if count > MAX_JOURNAL_EVENTS {
            return Err(CallError::Domain(problem(
                CheckpointStoreProblem::InvalidCheckpoint,
                "checkpoint source range exceeds bounded journal read",
            )));
        }
        let page = host
            .invoke::<EventJournal::read>(
                "Journal",
                &JournalQuery {
                    session: checkpoint.session.clone(),
                    after: span.first_sequence - 1,
                    limit: count,
                },
            )
            .map_err(map_storage)?;
        restore_original_events(checkpoint, &page.events).map_err(|error| {
            CallError::Domain(problem(
                match error {
                    ReplayProblem::InvalidCheckpoint | ReplayProblem::CapacityExceeded => {
                        CheckpointStoreProblem::InvalidCheckpoint
                    }
                    ReplayProblem::WrongSession => CheckpointStoreProblem::ScopeDenied,
                    _ => CheckpointStoreProblem::SourceChanged,
                },
                "original source journal is missing, altered or inaccessible",
            ))
        })?;
        Ok(())
    }

    #[provide(CompactionStore::put)]
    fn put(
        _: &mut State,
        host: &Host<'_>,
        checkpoint: CompactionCheckpoint,
    ) -> CallResult<CompactionCheckpoint, CheckpointStoreError> {
        let (revision, history) = load(host, &checkpoint.session)?;
        let updated = append_checkpoint(&history, &checkpoint, &checkpoint.session)
            .map_err(|code| CallError::Domain(problem(code, "invalid checkpoint revision")))?;
        verify_journal(host, &checkpoint)?;
        let data = encode(&updated).map_err(CallError::Kernel)?;
        let result = host
            .invoke::<Storage::transact>(
                "Storage",
                &StorageTransaction {
                    session: checkpoint.session.clone(),
                    edits: vec![StorageEdit {
                        key: RECORD_KEY.into(),
                        expected: revision,
                        data: data.clone(),
                    }],
                },
            )
            .map_err(map_storage)?;
        if result.len() != 1 || result[0].revision != revision + 1 || result[0].data != data {
            return Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "invalid checkpoint storage CAS acknowledgment",
            )));
        }
        Ok(checkpoint)
    }

    #[provide(CompactionStore::get)]
    fn get(
        _: &mut State,
        host: &Host<'_>,
        key: CheckpointKey,
    ) -> CallResult<CompactionCheckpoint, CheckpointStoreError> {
        let (_, history) = load(host, &key.session)?;
        let checkpoint = find_checkpoint(&history, &key.id)
            .map_err(|code| CallError::Domain(problem(code, "checkpoint not found")))?;
        // Get is not permission to bypass source erasure or source revision
        // checks. Consumers can use exact replay only after this gate.
        verify_journal(host, checkpoint)?;
        Ok(checkpoint.clone())
    }
}
