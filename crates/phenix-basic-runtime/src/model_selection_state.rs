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
const ROUTING_WITNESS_KEY: &str = "phenix.provider.native-routing-witness.v1";

/// A durable, non-secret witness that native model routing has been selected.
/// It is committed atomically with the selected identity, so a missing record
/// can never be confused with a new preset session after controller restart.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RoutingWitness {
    NativeSelected,
}

impl Portable for RoutingWitness {
    // A one-variant durable witness needs no new contract/ABI, just a fixed
    // tagged string that is validated when decoding. The Rust type has no
    // invalid success variant and cannot carry secrets or provider metadata.
    const SCHEMA: phenix_contract::StaticSchema = <String as Portable>::SCHEMA;

    fn to_value(&self) -> phenix_contract::Value {
        String::from("native-selected").to_value()
    }

    fn from_value(value: phenix_contract::Value) -> Result<Self, phenix_contract::Failure> {
        let decoded = String::from_value(value)?;
        if decoded == "native-selected" {
            Ok(Self::NativeSelected)
        } else {
            Err(phenix_contract::Failure::new(
                phenix_contract::Code::PluginFailed,
                "unexpected native routing witness",
            ))
        }
    }
}

fn checked_witness(record: &StoredRecord) -> Result<(), String> {
    if record.revision == 0
        || basic::decode::<RoutingWitness>(&record.data)
            .map_err(|_| "invalid native routing witness".to_owned())?
            != RoutingWitness::NativeSelected
    {
        return Err("invalid native routing witness".into());
    }
    Ok(())
}

fn checked_routing_state(
    selected: Option<StoredRecord>,
    witness: Option<StoredRecord>,
) -> Result<Option<(ModelSelection, u64)>, String> {
    match (selected, witness) {
        (None, None) => Ok(None),
        (Some(selected), Some(witness)) => {
            checked_witness(&witness)?;
            if selected.revision != witness.revision {
                return Err("selected model and native routing witness revisions differ".into());
            }
            unpack(selected).map(Some)
        }
        (None, Some(_)) => Err("native routing witness exists without selected model".into()),
        (Some(_), None) => Err("selected model exists without native routing witness".into()),
    }
}
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

fn read_record(
    kernel: &Kernel,
    provider: &str,
    session: &str,
    key: &str,
) -> Result<Option<StoredRecord>, String> {
    let request = RecordKey {
        session: session.into(),
        key: key.into(),
    };
    match invoke::<_, StoredRecord>(kernel, provider, "read", &request)? {
        Ok(record) => Ok(Some(record)),
        Err(problem) if problem.code == ErrorCode::NotFound => Ok(None),
        Err(problem) => Err(format!(
            "selected model Storage read denied: {:?}",
            problem.code
        )),
    }
}

/// Read an intent; NOT authentication or effective current-model evidence.
/// A persisted native witness makes absence/corruption of selected identity
/// an error even after process restart. Neither record grants execution.
/// The controller must refresh current provider/catalog state before install.
pub(crate) fn read(
    kernel: &Kernel,
    composition: &Composition,
    session: &str,
) -> Result<Option<(ModelSelection, u64)>, String> {
    basic::session_resource(session).map_err(|_| "invalid selected model session".to_owned())?;
    let provider = storage_provider(composition)?;
    let selected = read_record(kernel, provider, session, KEY)?;
    let witness = read_record(kernel, provider, session, ROUTING_WITNESS_KEY)?;
    checked_routing_state(selected, witness)
}

/// Commit only a previously checked non-secret binding to the session's
/// ordinary revision-CAS Storage. Never write a client-supplied model name.
/// The caller must install the current authenticated Model through the same
/// session kernel and prove execution admission BEFORE acknowledging success.
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
    let provider = storage_provider(composition)?;
    let current_witness = read_record(kernel, provider, session, ROUTING_WITNESS_KEY)?;
    if expected == 0 && current_witness.is_some() {
        return Err("native routing witness exists without a first selected model".into());
    }
    if expected != 0 && current_witness.is_none() {
        return Err("existing native model has no routing witness".into());
    }
    if let Some(record) = current_witness.as_ref() {
        checked_witness(record)?;
    }
    let witness_expected = current_witness.as_ref().map_or(0, |record| record.revision);
    if witness_expected != expected {
        return Err("native witness revision disagrees with selected model CAS".into());
    }
    let witness_next = witness_expected
        .checked_add(1)
        .ok_or("native routing witness revision overflow")?;
    let data = basic::encode(checked.selection())
        .map_err(|_| "model selection canonical encoding failed".to_owned())?;
    let witness_data = basic::encode(&RoutingWitness::NativeSelected)
        .map_err(|_| "native routing witness canonical encoding failed".to_owned())?;
    // Basic Storage's atomic multi-edit CAS is the only durability source.
    // Incomplete changes roll back together; no separate host database or
    // independent marker write can leave an unwitnessed selected identity.
    let transaction = StorageTransaction {
        session: session.into(),
        edits: vec![
            StorageEdit {
                key: KEY.into(),
                expected,
                data: data.clone(),
            },
            StorageEdit {
                key: ROUTING_WITNESS_KEY.into(),
                expected: witness_expected,
                data: witness_data.clone(),
            },
        ],
    };
    let result: Vec<StoredRecord> = invoke(kernel, provider, "transact", &transaction)?
        .map_err(|problem| format!("selected model CAS denied: {:?}", problem.code))?;
    if result.len() != 2
        || result[0].revision != next
        || result[0].data != data
        || result[1].revision != witness_next
        || result[1].data != witness_data
    {
        return Err("model selection Storage returned an invalid atomic CAS receipt".into());
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
    fn selected_model_requires_atomic_native_witness_on_every_load() {
        let selection = StoredRecord {
            revision: 4,
            data: basic::encode(&selected()).unwrap(),
        };
        let witness = StoredRecord {
            revision: 4,
            data: basic::encode(&RoutingWitness::NativeSelected).unwrap(),
        };
        assert_eq!(checked_routing_state(None, None).unwrap(), None);
        assert!(checked_routing_state(Some(selection.clone()), None).is_err());
        assert!(checked_routing_state(None, Some(witness.clone())).is_err());
        assert_eq!(
            checked_routing_state(Some(selection.clone()), Some(witness.clone()))
                .unwrap()
                .unwrap(),
            (selected(), 4)
        );
        let mut conflicting = witness.clone();
        conflicting.revision = 3;
        assert!(checked_routing_state(Some(selection.clone()), Some(conflicting)).is_err());
        let mut corrupt = witness.clone();
        corrupt.data = phenix_contract::Blob(vec![0, 255, 17]);
        assert!(checked_routing_state(Some(selection.clone()), Some(corrupt)).is_err());
        let mut invalid_revision = witness;
        invalid_revision.revision = 0;
        assert!(checked_routing_state(Some(selection), Some(invalid_revision)).is_err());
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
