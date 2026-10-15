//! P2-3c session-scoped checkpoint CAS using selected Basic Storage and Journal.
//! The Basic EventJournal is authoritative. Compaction stores only derived views.
use phenix_advanced_contracts::{
    validate_checkpoint, CompactionCheckpoint, CompactionProblem,
};
use std::collections::BTreeSet;

const RECORD_KEY: &str = "compaction-ledger.v1";
const MAX_CHECKPOINTS: usize = 256;
const MAX_EVENTS_PER_CHECKPOINT: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerProblem {
    InvalidCheckpoint(CompactionProblem),
    InvalidLineage,
    Conflict,
    ScopeDenied,
    CapacityExceeded,
}

pub fn validate_ledger(history: &[CompactionCheckpoint]) -> Result<(), LedgerProblem> {
    if history.len() > MAX_CHECKPOINTS {
        return Err(LedgerProblem::CapacityExceeded);
    }
    let mut ids = BTreeSet::new();
    let mut previous: Option<&CompactionCheckpoint> = None;
    for (i, current) in history.iter().enumerate() {
        validate_checkpoint(current).map_err(LedgerProblem::InvalidCheckpoint)?;
        if !ids.insert(current.id.as_str())
            || current.revision != u64::try_from(i + 1).map_err(|_| LedgerProblem::CapacityExceeded)?
            || current.parent_checkpoint.as_deref() != previous.map(|item| item.id.as_str())
        {
            return Err(LedgerProblem::InvalidLineage);
        }
        if previous.is_some_and(|item| item.session != current.session) {
            return Err(LedgerProblem::ScopeDenied);
        }
        previous = Some(current);
    }
    Ok(())
}

pub fn append_checkpoint(
    history: &[CompactionCheckpoint],
    expected: u64,
    checkpoint: &CompactionCheckpoint,
) -> Result<Vec<CompactionCheckpoint>, LedgerProblem> {
    validate_ledger(history)?;
    if history.len() == MAX_CHECKPOINTS {
        return Err(LedgerProblem::CapacityExceeded);
    }
    if expected != history.len() as u64 {
        return Err(LedgerProblem::Conflict);
    }
    if history.first().is_some_and(|previous| previous.session != checkpoint.session) {
        return Err(LedgerProblem::ScopeDenied);
    }
    validate_checkpoint(checkpoint).map_err(LedgerProblem::InvalidCheckpoint)?;
    if checkpoint.revision != expected + 1
        || checkpoint.parent_checkpoint.as_deref()
            != history.last().map(|previous| previous.id.as_str())
        || history.iter().any(|prior| prior.id == checkpoint.id)
    {
        return Err(LedgerProblem::InvalidLineage);
    }
    let mut next = history.to_vec();
    next.push(checkpoint.clone());
    Ok(next)
}

#[phenix_sdk::plugin(
    id = "phenix.advanced.compaction-store",
    version = "1.0.0",
    provide(phenix_advanced_contracts::CompactionStore, slot = "Checkpoint"),
    require(phenix_basic_contracts::Storage, slot = "Storage"),
    require(phenix_basic_contracts::EventJournal, slot = "Journal")
)]
mod plugin {
    use super::{
        append_checkpoint, validate_ledger, LedgerProblem, MAX_EVENTS_PER_CHECKPOINT, RECORD_KEY,
    };
    use phenix_advanced_contracts::{
        decode, encode, CompactionCheckpoint, CompactionCheckpointCommit, CompactionCheckpointKey,
        CompactionStore, CompactionStoreError, CompactionStoreProblem, StoredCompactionCheckpoint,
    };
    use phenix_basic_contracts::{
        BasicError, ErrorCode, Event, EventJournal, JournalQuery, RecordKey, Storage, StorageEdit,
        StorageTransaction, StoredRecord,
    };
    use phenix_p2_compaction_replay::restore_original_events;
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};

    #[derive(Default)]
    pub struct State;

    fn error(problem: CompactionStoreProblem, message: &'static str) -> CompactionStoreError {
        CompactionStoreError {
            problem,
            message: message.into(),
        }
    }

    fn failure(err: CallError<BasicError>) -> CallError<CompactionStoreError> {
        match err {
            CallError::Kernel(value) => CallError::Kernel(value),
            CallError::Domain(value) => CallError::Domain(error(
                match value.code {
                    ErrorCode::NotFound => CompactionStoreProblem::NotFound,
                    ErrorCode::Conflict => CompactionStoreProblem::Conflict,
                    ErrorCode::Denied => CompactionStoreProblem::ScopeDenied,
                    _ => CompactionStoreProblem::StorageFailed,
                },
                "basic storage rejected checkpoint operation",
            )),
        }
    }

    fn ledger_error(err: LedgerProblem) -> CallError<CompactionStoreError> {
        let problem = match err {
            LedgerProblem::InvalidCheckpoint(_) => CompactionStoreProblem::InvalidCheckpoint,
            LedgerProblem::InvalidLineage => CompactionStoreProblem::InvalidLineage,
            LedgerProblem::Conflict => CompactionStoreProblem::Conflict,
            LedgerProblem::ScopeDenied => CompactionStoreProblem::ScopeDenied,
            LedgerProblem::CapacityExceeded => CompactionStoreProblem::CapacityExceeded,
        };
        CallError::Domain(error(problem, "checkpoint ledger validation failed"))
    }

    fn session(host: &Host<'_>, name: &str) -> CallResult<(), CompactionStoreError> {
        phenix_basic_contracts::check_session(host, name).map_err(|err| match err {
            CallError::Kernel(value) => CallError::Kernel(value),
            CallError::Domain(_) => CallError::Domain(error(
                CompactionStoreProblem::ScopeDenied,
                "checkpoint session is not authorized",
            )),
        })
    }

    fn load(
        host: &Host<'_>,
        name: &str,
    ) -> CallResult<(u64, Vec<CompactionCheckpoint>), CompactionStoreError> {
        session(host, name)?;
        let key = RecordKey {
            session: name.into(),
            key: RECORD_KEY.into(),
        };
        let stored = match host.invoke::<Storage::read>("Storage", &key) {
            Ok(record) => Some(record),
            Err(CallError::Domain(err)) if err.code == ErrorCode::NotFound => None,
            Err(err) => return Err(failure(err)),
        };
        let (revision, history) = match stored {
            Some(StoredRecord { revision, data }) => {
                let history = decode::<Vec<CompactionCheckpoint>>(&data).map_err(|_| {
                    CallError::Domain(error(
                        CompactionStoreProblem::StorageFailed,
                        "persisted checkpoint bytes are invalid",
                    ))
                })?;
                (revision, history)
            }
            None => (0, vec![]),
        };
        validate_ledger(&history).map_err(|_| {
            CallError::Domain(error(
                CompactionStoreProblem::StorageFailed,
                "persisted checkpoint lineage is invalid",
            ))
        })?;
        if revision != history.len() as u64
            || history.iter().any(|checkpoint| checkpoint.session != name)
        {
            return Err(CallError::Domain(error(
                CompactionStoreProblem::StorageFailed,
                "checkpoint revision or session mismatch",
            )));
        }
        Ok((revision, history))
    }

    fn verify_journal(
        host: &Host<'_>,
        checkpoint: &CompactionCheckpoint,
    ) -> CallResult<(), CompactionStoreError> {
        let input = &checkpoint.input;
        let count = input
            .last_sequence
            .checked_sub(input.first_sequence)
            .and_then(|delta| delta.checked_add(1))
            .ok_or_else(|| CallError::Domain(error(
                CompactionStoreProblem::InvalidCheckpoint,
                "invalid checkpoint input range",
            )))?;
        if count > MAX_EVENTS_PER_CHECKPOINT {
            return Err(CallError::Domain(error(
                CompactionStoreProblem::CapacityExceeded,
                "checkpoint input exceeds bounded journal verification",
            )));
        }
        let mut events: Vec<Event> = Vec::new();
        let mut after = input.first_sequence - 1;
        while (events.len() as u64) < count {
            let limit = (count - events.len() as u64).min(64);
            let page = host
                .invoke::<EventJournal::read>(
                    "Journal",
                    &JournalQuery {
                        session: checkpoint.session.clone(),
                        after,
                        limit,
                    },
                )
                .map_err(|err| match err {
                    CallError::Kernel(value) => CallError::Kernel(value),
                    CallError::Domain(value) => CallError::Domain(error(
                        if value.code == ErrorCode::Denied {
                            CompactionStoreProblem::ScopeDenied
                        } else {
                            CompactionStoreProblem::SourceChanged
                        },
                        "checkpoint source journal is unavailable",
                    )),
                })?;
            if page.events.is_empty() || page.next <= after {
                return Err(CallError::Domain(error(
                    CompactionStoreProblem::SourceChanged,
                    "checkpoint source event is missing",
                )));
            }
            after = page.next;
            events.extend(page.events);
        }
        restore_original_events(checkpoint, &events).map_err(|_| {
            CallError::Domain(error(
                CompactionStoreProblem::SourceChanged,
                "checkpoint source journal changed",
            ))
        })?;
        Ok(())
    }

    #[provide(CompactionStore::commit)]
    fn commit(
        _: &mut State,
        host: &Host<'_>,
        request: CompactionCheckpointCommit,
    ) -> CallResult<StoredCompactionCheckpoint, CompactionStoreError> {
        let checkpoint = request.checkpoint;
        session(host, &checkpoint.session)?;
        // Run schema validation before invoking any imported capability.
        phenix_advanced_contracts::validate_checkpoint(&checkpoint)
            .map_err(|_| CallError::Domain(error(
                CompactionStoreProblem::InvalidCheckpoint,
                "invalid proposed checkpoint",
            )))?;
        let (revision, previous) = load(host, &checkpoint.session)?;
        let updated = append_checkpoint(&previous, request.expected, &checkpoint)
            .map_err(ledger_error)?;
        if revision != request.expected {
            return Err(CallError::Domain(error(
                CompactionStoreProblem::Conflict,
                "stale checkpoint storage revision",
            )));
        }
        verify_journal(host, &checkpoint)?;
        let data = encode(&updated).map_err(CallError::Kernel)?;
        let records = host.invoke::<Storage::transact>(
            "Storage",
            &StorageTransaction {
                session: checkpoint.session.clone(),
                edits: vec![StorageEdit {
                    key: RECORD_KEY.into(),
                    expected: revision,
                    data: data.clone(),
                }],
            },
        ).map_err(failure)?;
        if records.len() != 1 || records[0].revision != revision + 1
            || records[0].data != data
        {
            return Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "invalid checkpoint storage CAS acknowledgement",
            )));
        }
        Ok(StoredCompactionCheckpoint {
            checkpoint,
            storage_revision: revision + 1,
        })
    }

    #[provide(CompactionStore::get)]
    fn get(
        _: &mut State,
        host: &Host<'_>,
        key: CompactionCheckpointKey,
    ) -> CallResult<StoredCompactionCheckpoint, CompactionStoreError> {
        session(host, &key.session)?;
        if !phenix_sdk::identifier(&key.id) {
            return Err(CallError::Domain(error(
                CompactionStoreProblem::InvalidCheckpoint,
                "invalid checkpoint ID",
            )));
        }
        let (revision, history) = load(host, &key.session)?;
        let checkpoint = history.iter().find(|entry| entry.id == key.id).ok_or_else(|| {
            CallError::Domain(error(CompactionStoreProblem::NotFound, "checkpoint not found"))
        })?;
        verify_journal(host, checkpoint)?;
        Ok(StoredCompactionCheckpoint {
            checkpoint: checkpoint.clone(),
            storage_revision: revision,
        })
    }
}
