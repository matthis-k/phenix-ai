//! Packaged profiles retain their IDs for durable sessions when retired.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MANIFEST: &str = "configuration/packaged-v1";

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    owned: BTreeMap<RoutingProfileId, RoutingProfile>,
    active: BTreeSet<RoutingProfileId>,
}

fn manifest(context: &ModelContext<'_, '_>) -> Result<(Option<Vec<u8>>, Manifest), String> {
    let bytes = read_raw(context, MANIFEST)?;
    let value = bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    Ok((bytes, value))
}

pub(super) fn ownership(
    context: &ModelContext<'_, '_>,
) -> Result<(BTreeSet<RoutingProfileId>, BTreeSet<RoutingProfileId>), String> {
    let (_, manifest) = manifest(context)?;
    Ok((manifest.owned.keys().cloned().collect(), manifest.active))
}

pub(super) fn prepare(
    context: &ModelContext<'_, '_>,
    profiles: Vec<RoutingProfile>,
) -> Result<ModelResponse, String> {
    let (old_manifest, mut ownership) = manifest(context)?;
    let old_index = read_raw(context, PROFILE_INDEX)?;
    let mut current = load_profiles(context)?
        .into_iter()
        .map(|profile| (profile.id.clone(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut previous = BTreeMap::new();
    for (id, profile) in &current {
        let raw = read_raw(context, &profile_key(id))?
            .ok_or_else(|| format!("routing profile disappeared: {id}"))?;
        if serde_json::from_slice::<RoutingProfile>(&raw).map_err(|error| error.to_string())?
            != *profile
        {
            return Err(format!(
                "routing profile changed during configuration: {id}"
            ));
        }
        previous.insert(id.clone(), raw);
    }
    let mut desired = BTreeMap::new();
    for profile in profiles {
        if desired.insert(profile.id.clone(), profile).is_some() {
            return Err("duplicate routing profile in packaged configuration".into());
        }
    }
    let mut operations = vec![
        TransactionOp::AssertValue {
            key: MANIFEST.into(),
            expected: old_manifest,
        },
        TransactionOp::AssertValue {
            key: PROFILE_INDEX.into(),
            expected: old_index,
        },
    ];
    for (id, expected) in &ownership.owned {
        if current.get(id) != Some(expected) {
            return Err(format!(
                "packaged routing profile changed outside configuration: {id}"
            ));
        }
    }
    for (id, profile) in &desired {
        if let Some(existing) = current.get(id) {
            if !ownership.owned.contains_key(id) && existing != profile {
                return Err(format!("routing profile identity is immutable: {id}"));
            }
        }
    }
    ownership.active = desired.keys().cloned().collect();
    for (id, profile) in desired {
        ownership.owned.insert(id.clone(), profile.clone());
        current.insert(id, profile);
    }
    for (id, profile) in &current {
        let key = profile_key(id);
        operations.push(TransactionOp::AssertValue {
            key: key.clone(),
            expected: previous.get(id).cloned(),
        });
        operations.push(TransactionOp::Put {
            key,
            value: serde_json::to_vec(profile).map_err(|error| error.to_string())?,
        });
    }
    operations.push(TransactionOp::Put {
        key: PROFILE_INDEX.into(),
        value: serde_json::to_vec(&current.keys().collect::<Vec<_>>())
            .map_err(|error| error.to_string())?,
    });
    operations.push(TransactionOp::Put {
        key: MANIFEST.into(),
        value: serde_json::to_vec(&ownership).map_err(|error| error.to_string())?,
    });
    let mutation = context
        .kernel
        .prepare_durable_transaction(&model_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    Ok(ModelResponse::PreparedProfiles {
        mutation,
        profiles: current.into_values().collect(),
    })
}
