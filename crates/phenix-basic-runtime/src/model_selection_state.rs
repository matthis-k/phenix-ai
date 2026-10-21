//! Session-owned, non-secret selected-model intent in the existing Basic
//! Storage plugin. This is a checked persistence boundary, NOT an executable
//! provider grant or an independent controller database.
use phenix_basic_contracts::{
    self as basic, BasicError, ErrorCode, RecordKey, StorageEdit, StorageTransaction, StoredRecord,
};
use phenix_contract::Portable;
use phenix_kernel::{
    resolve::{Composition, Selection},
    Kernel, Outcome,
};
use phenix_provider_contracts::{HttpModelBinding, ModelSelection};

const KEY: &str = "phenix.provider.selected-model.v1";

fn storage_provider(composition: &Composition) -> Result<&str, String> {
    let mut slots = composition.slots.iter().filter(|slot| {
        slot.caller == "phenix.basic.session" && slot.slot == "Storage" && slot.index == 0
    });
    let provider = slots.next().ok_or("session storage binding is missing")?;
    if slots.next().is_some() {
        return Err("session storage binding is ambiguous".into());
    }
    Ok(&provider.provider)
}

fn invoke<T: Portable, R: Portable>(
    kernel: &Kernel,
    provider: &str,
    operation: &str,
    request: &T,
) -> Result<Result<R, BasicError>, String> {
    // A root operation requires an exact launcher-installed grant on this
    // session's kernel. The provider name cannot come from a client request.
    let root = kernel
        .admit_root(Selection {
            interface: "phenix.basic.storage@1".into(),
            operation: operation.into(),
            provider: provider.into(),
        })
        .map_err(|_| "model selection storage operation is not authorized".to_owned())?;
    match root
        .invoke(&request.to_value())
        .map_err(|_| "model selection storage call failed".to_owned())?
    {
        Outcome::Success(value) => Ok(Ok(R::from_value(value)
            .map_err(|_| "invalid typed model selection storage reply".to_owned())?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)
            .map_err(|_| "invalid model selection storage error".to_owned())?)),
    }
}

fn validate_intent(selection: &ModelSelection) -> Result<(), String> {
    if selection.catalog_revision == 0
        || selection.provider_id.trim().is_empty()
        || selection.connection_id.trim().is_empty()
        || selection.model_id.trim().is_empty()
        || selection.protocol_id.trim().is_empty()
        || selection.endpoint_id.trim().is_empty()
        || selection
            .profile_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
    {
        return Err("stored selection has invalid model identity".into());
    }
    Ok(())
}

fn unpack(record: StoredRecord) -> Result<(ModelSelection, u64), String> {
    if record.revision == 0 {
        return Err("stored model revision is invalid".into());
    }
    let selected: ModelSelection = basic::decode(&record.data)
        .map_err(|_| "selected model record failed canonical Portable decoding".to_owned())?;
    validate_intent(&selected)?;
    Ok((selected, record.revision))
}

/// Read an intent; NOT authentication or effective current-model evidence.
/// The controller must refresh current provider/catalog state after each
/// reload and before any actual HTTP Model installation.
#[allow(dead_code)]
pub(crate) fn read(
    kernel: &Kernel,
    composition: &Composition,
    session: &str,
) -> Result<Option<(ModelSelection, u64)>, String> {
    basic::session_resource(session).map_err(|_| "invalid selected model session".to_owned())?;
    let provider = storage_provider(composition)?;
    let request = RecordKey {
        session: session.into(),
        key: KEY.into(),
    };
    match invoke::<_, StoredRecord>(kernel, provider, "read", &request)? {
        Ok(record) => unpack(record).map(Some),
        Err(problem) if problem.code == ErrorCode::NotFound => Ok(None),
        Err(problem) => Err(format!("selected model read denied: {:?}", problem.code)),
    }
}

/// Commit only a previously checked non-secret binding to the session's
/// ordinary revision-CAS Storage. Never write a client-supplied model name.
/// The caller must install the current authenticated Model through the same
/// session kernel and prove execution admission BEFORE acknowledging success.
#[allow(dead_code)]
pub(crate) fn compare_and_swap(
    kernel: &Kernel,
    composition: &Composition,
    session: &str,
    checked: &HttpModelBinding,
    expected: u64,
) -> Result<u64, String> {
    basic::session_resource(session).map_err(|_| "invalid selected model session".to_owned())?;
    let next = expected
        .checked_add(1)
        .ok_or("model selection revision overflow")?;
    validate_intent(checked.selection())?;
    let data = basic::encode(checked.selection())
        .map_err(|_| "model selection canonical encoding failed".to_owned())?;
    let transaction = StorageTransaction {
        session: session.into(),
        edits: vec![StorageEdit {
            key: KEY.into(),
            expected,
            data: data.clone(),
        }],
    };
    let provider = storage_provider(composition)?;
    let result: Vec<StoredRecord> = invoke(kernel, provider, "transact", &transaction)?
        .map_err(|problem| format!("selected model CAS denied: {:?}", problem.code))?;
    if result.len() != 1 || result[0].revision != next || result[0].data != data {
        return Err("model selection Storage returned an invalid CAS receipt".into());
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected() -> ModelSelection {
        ModelSelection {
            provider_id: "fixture".into(),
            connection_id: "http".into(),
            profile_id: Some("primary".into()),
            model_id: "real-selected".into(),
            protocol_id: "openai-chat".into(),
            endpoint_id: "chat".into(),
            catalog_revision: 7,
        }
    }

    #[test]
    fn stored_selection_is_canonical_nonsecret_identity() {
        let choice = selected();
        let data = basic::encode(&choice).unwrap();
        assert_eq!(
            unpack(StoredRecord {
                revision: 3,
                data: data.clone(),
            })
            .unwrap(),
            (choice, 3)
        );
        assert!(unpack(StoredRecord { revision: 0, data }).is_err());
    }

    #[test]
    fn refuse_corrupted_or_unvalidated_persisted_choices() {
        let mut choice = selected();
        choice.catalog_revision = 0;
        assert!(unpack(StoredRecord {
            revision: 1,
            data: basic::encode(&choice).unwrap(),
        })
        .is_err());
        choice = selected();
        choice.profile_id = Some("".into());
        assert!(validate_intent(&choice).is_err());
        assert!(unpack(StoredRecord {
            revision: 1,
            data: phenix_contract::Blob(vec![255, 0, 120]),
        })
        .is_err());
    }
}
