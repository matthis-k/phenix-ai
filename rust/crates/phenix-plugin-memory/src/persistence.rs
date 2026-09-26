use crate::{
    error::{MemoryError, MemoryResult},
    implementation::{memory_namespace, MemoryContext},
};
use phenix_core::TransactionOp;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;

pub(crate) fn insert_record<T: Serialize>(
    context: &MemoryContext<'_, '_>,
    index_key: &str,
    record_key: &str,
    id: &str,
    record: &T,
) -> MemoryResult<()> {
    insert_record_with_sidecar_secondary_and_updates::<T, (), ()>(
        context,
        index_key,
        record_key,
        id,
        record,
        None,
        None,
        &[],
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "atomic durable insert keeps record, sidecar, secondary index, and updates explicit"
)]
pub(crate) fn insert_record_with_sidecar_secondary_and_updates<
    T: Serialize,
    U: Serialize,
    V: Serialize,
>(
    context: &MemoryContext<'_, '_>,
    index_key: &str,
    record_key: &str,
    id: &str,
    record: &T,
    sidecar: Option<(&str, &U)>,
    secondary: Option<(&str, &[String])>,
    updates: &[(&str, &V)],
) -> MemoryResult<()> {
    let old_index = read_raw(context, index_key)?;
    let mut ids = decode_index(old_index.as_deref())?;
    if ids.iter().any(|existing| existing == id) || read_raw(context, record_key)?.is_some() {
        return Err(MemoryError::Conflict(format!(
            "immutable record already exists: {id}"
        )));
    }
    if let Some((sidecar_key, _)) = sidecar {
        if read_raw(context, sidecar_key)?.is_some() {
            return Err(MemoryError::Conflict(format!(
                "derived record state already exists: {id}"
            )));
        }
    }

    ids.push(id.to_owned());
    ids.sort();
    let mut operations = vec![
        TransactionOp::AssertValue {
            key: record_key.into(),
            expected: None,
        },
        TransactionOp::AssertValue {
            key: index_key.into(),
            expected: old_index,
        },
        TransactionOp::Put {
            key: record_key.into(),
            value: serde_json::to_vec(record)
                .map_err(|error| MemoryError::Persistence(error.to_string()))?,
        },
        TransactionOp::Put {
            key: index_key.into(),
            value: serde_json::to_vec(&ids)
                .map_err(|error| MemoryError::Persistence(error.to_string()))?,
        },
    ];
    if let Some((sidecar_key, sidecar)) = sidecar {
        operations.push(TransactionOp::AssertValue {
            key: sidecar_key.into(),
            expected: None,
        });
        operations.push(TransactionOp::Put {
            key: sidecar_key.into(),
            value: serde_json::to_vec(sidecar)
                .map_err(|error| MemoryError::Persistence(error.to_string()))?,
        });
    }
    if let Some((secondary_key, entries)) = secondary {
        let old_secondary = read_raw(context, secondary_key)?;
        let mut secondary_index = decode_secondary_index(old_secondary.as_deref())?;
        for entry in entries {
            let entry_ids = secondary_index.entry(entry.clone()).or_default();
            if !entry_ids.iter().any(|existing| existing == id) {
                entry_ids.push(id.to_owned());
                entry_ids.sort();
            }
        }
        operations.push(TransactionOp::AssertValue {
            key: secondary_key.into(),
            expected: old_secondary,
        });
        operations.push(TransactionOp::Put {
            key: secondary_key.into(),
            value: serde_json::to_vec(&secondary_index)
                .map_err(|error| MemoryError::Persistence(error.to_string()))?,
        });
    }
    for (key, update) in updates {
        let old = read_raw(context, key)?;
        operations.push(TransactionOp::AssertValue {
            key: (*key).into(),
            expected: old,
        });
        operations.push(TransactionOp::Put {
            key: (*key).into(),
            value: serde_json::to_vec(update)
                .map_err(|error| MemoryError::Persistence(error.to_string()))?,
        });
    }

    context
        .kernel
        .transact_durable(&memory_namespace(), &operations)
        .map_err(|error| MemoryError::Persistence(error.to_string()))
}

pub(crate) fn write_record<T: Serialize>(
    context: &MemoryContext<'_, '_>,
    key: &str,
    record: &T,
) -> MemoryResult<()> {
    let old = read_raw(context, key)?;
    context
        .kernel
        .transact_durable(
            &memory_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: key.into(),
                    value: serde_json::to_vec(record)
                        .map_err(|error| MemoryError::Persistence(error.to_string()))?,
                },
            ],
        )
        .map_err(|error| MemoryError::Persistence(error.to_string()))
}

pub(crate) fn write_record_with_secondary_entry<T: Serialize>(
    context: &MemoryContext<'_, '_>,
    key: &str,
    record: &T,
    secondary_key: &str,
    entry: &str,
    id: &str,
) -> MemoryResult<()> {
    let old = read_raw(context, key)?;
    let old_secondary = read_raw(context, secondary_key)?;
    let mut secondary_index = decode_secondary_index(old_secondary.as_deref())?;
    let entry_ids = secondary_index.entry(entry.to_owned()).or_default();
    if !entry_ids.iter().any(|existing| existing == id) {
        entry_ids.push(id.to_owned());
        entry_ids.sort();
    }
    context
        .kernel
        .transact_durable(
            &memory_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.into(),
                    expected: old,
                },
                TransactionOp::AssertValue {
                    key: secondary_key.into(),
                    expected: old_secondary,
                },
                TransactionOp::Put {
                    key: key.into(),
                    value: serde_json::to_vec(record)
                        .map_err(|error| MemoryError::Persistence(error.to_string()))?,
                },
                TransactionOp::Put {
                    key: secondary_key.into(),
                    value: serde_json::to_vec(&secondary_index)
                        .map_err(|error| MemoryError::Persistence(error.to_string()))?,
                },
            ],
        )
        .map_err(|error| MemoryError::Persistence(error.to_string()))
}

pub(crate) fn load_secondary_ids(
    context: &MemoryContext<'_, '_>,
    secondary_key: &str,
    entry: &str,
    limit: usize,
) -> MemoryResult<Vec<String>> {
    Ok(
        decode_secondary_index(read_raw(context, secondary_key)?.as_deref())?
            .remove(entry)
            .unwrap_or_default()
            .into_iter()
            .take(limit)
            .collect(),
    )
}

pub(crate) fn load_secondary_ids_page(
    context: &MemoryContext<'_, '_>,
    secondary_key: &str,
    entry: &str,
    after: Option<&str>,
    limit: usize,
) -> MemoryResult<(Vec<String>, bool)> {
    let ids = decode_secondary_index(read_raw(context, secondary_key)?.as_deref())?
        .remove(entry)
        .unwrap_or_default();
    let mut page = ids
        .into_iter()
        .filter(|id| after.is_none_or(|after| id.as_str() > after))
        .take(limit.saturating_add(1))
        .collect::<Vec<_>>();
    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }
    Ok((page, has_more))
}

pub(crate) fn load_records<T: DeserializeOwned>(
    context: &MemoryContext<'_, '_>,
    index_key: &str,
    key: fn(&str) -> String,
) -> MemoryResult<Vec<T>> {
    decode_index(read_raw(context, index_key)?.as_deref())?
        .into_iter()
        .map(|id| {
            read_record(context, &key(&id))?.ok_or_else(|| {
                MemoryError::Persistence(format!("missing durable record from {index_key}: {id}"))
            })
        })
        .collect()
}

pub(crate) fn read_record<T: DeserializeOwned>(
    context: &MemoryContext<'_, '_>,
    key: &str,
) -> MemoryResult<Option<T>> {
    read_raw(context, key)?
        .map(|value| {
            serde_json::from_slice(&value)
                .map_err(|error| MemoryError::Persistence(error.to_string()))
        })
        .transpose()
}

fn read_raw(context: &MemoryContext<'_, '_>, key: &str) -> MemoryResult<Option<Vec<u8>>> {
    context
        .kernel
        .read_durable(&memory_namespace(), key)
        .map_err(|error| MemoryError::Persistence(error.to_string()))
}

fn decode_index(value: Option<&[u8]>) -> MemoryResult<Vec<String>> {
    value
        .map(|value| {
            serde_json::from_slice(value)
                .map_err(|error| MemoryError::Persistence(error.to_string()))
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_secondary_index(value: Option<&[u8]>) -> MemoryResult<BTreeMap<String, Vec<String>>> {
    value
        .map(|value| {
            serde_json::from_slice(value)
                .map_err(|error| MemoryError::Persistence(error.to_string()))
        })
        .unwrap_or_else(|| Ok(BTreeMap::new()))
}
