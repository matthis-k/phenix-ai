//! In-memory native Storage fixture for independent EventJournal conformance.
//! Intentionally NOT a durable B2 storage implementation.
#[phenix_sdk::plugin(
    id = "phenix.basic.storage-fixture",
    version = "1.0.0",
    provide(phenix_basic_contracts::Storage, slot = "Storage")
)]
mod plugin {
    use phenix_basic_contracts::{
        error, BasicError, ErrorCode, RecordKey, Storage, StorageEdit, StorageTransaction,
        StoredRecord,
    };
    use phenix_sdk::{CallError, CallResult, Host};
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    pub struct State {
        records: BTreeMap<(String, String), StoredRecord>,
    }

    #[provide(Storage::read)]
    fn read(
        state: &mut State,
        host: &Host<'_>,
        request: RecordKey,
    ) -> CallResult<StoredRecord, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        state
            .records
            .get(&(request.session, request.key))
            .cloned()
            .ok_or_else(|| CallError::Domain(error(ErrorCode::NotFound, "missing record")))
    }

    #[provide(Storage::transact)]
    fn transact(
        state: &mut State,
        host: &Host<'_>,
        request: StorageTransaction,
    ) -> CallResult<Vec<StoredRecord>, BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        let mut unique = BTreeSet::new();
        for StorageEdit { key, expected, .. } in &request.edits {
            if key.is_empty() || !unique.insert(key.as_str()) {
                return Err(CallError::Domain(error(
                    ErrorCode::InvalidInput,
                    "invalid transaction key",
                )));
            }
            let prior = state
                .records
                .get(&(request.session.clone(), key.clone()))
                .map_or(0, |record| record.revision);
            if prior != *expected {
                return Err(CallError::Domain(error(
                    ErrorCode::Conflict,
                    "storage revision conflict",
                )));
            }
            if prior == u64::MAX {
                return Err(CallError::Domain(error(
                    ErrorCode::Conflict,
                    "storage revision overflow",
                )));
            }
        }
        let mut results = Vec::with_capacity(request.edits.len());
        for edit in request.edits {
            let record = StoredRecord {
                revision: edit.expected + 1,
                data: edit.data,
            };
            state
                .records
                .insert((request.session.clone(), edit.key), record.clone());
            results.push(record);
        }
        Ok(results)
    }
}
