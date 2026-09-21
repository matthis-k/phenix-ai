//! Packaged profiles retain their IDs for durable sessions when retired.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MANIFEST: &str = "configuration/packaged-v1";

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    owned: BTreeMap<RoutingProfileId, RoutingProfile>,
    active: BTreeSet<RoutingProfileId>,
}

fn manifest(context: &ModelContext<'_, '_, '_>) -> Result<(Option<Vec<u8>>, Manifest), String> {
    let bytes = read_raw(context, MANIFEST)?;
    let value = bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    Ok((bytes, value))
}

pub(super) fn retired(
    context: &ModelContext<'_, '_, '_>,
) -> Result<BTreeSet<RoutingProfileId>, String> {
    let (_, manifest) = manifest(context)?;
    Ok(manifest
        .owned
        .keys()
        .filter(|id| !manifest.active.contains(*id))
        .cloned()
        .collect())
}

fn normalize(mut profile: RoutingProfile) -> RoutingProfile {
    for target in std::iter::once(&mut profile.default_target)
        .chain(profile.fallback_targets.iter_mut())
        .chain(profile.callable_targets.values_mut())
    {
        if matches!(target.options.get("backend"), Some(phenix_core::PhenixValue::String(value)) if value == "phenix")
        {
            target.options.remove("backend");
        }
        if matches!(
            target.options.get("inference"),
            Some(phenix_core::PhenixValue::Unit)
        ) {
            target.options.remove("inference");
        }
    }
    profile
}

fn generated(profile: &RoutingProfile) -> Result<bool, String> {
    if !profile.fallback_targets.is_empty() || !profile.callable_targets.is_empty() {
        return Ok(false);
    }
    // #579 may already have normalized values while preserving their old hashed IDs.
    for backend in [false, true] {
        for inference in [false, true] {
            let mut target = profile.default_target.clone();
            if backend {
                target.options.insert(
                    "backend".into(),
                    phenix_core::PhenixValue::String("phenix".into()),
                );
            }
            if inference && !target.options.contains_key("inference") {
                target
                    .options
                    .insert("inference".into(), phenix_core::PhenixValue::Unit);
            }
            let hash =
                Sha256::digest(serde_json::to_vec(&target).map_err(|error| error.to_string())?);
            let suffix = hash[..8]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if profile.id.as_str()
                == format!("model.{}.{}.{suffix}", target.provider_plugin, target.model)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn prepare(
    context: &ModelContext<'_, '_, '_>,
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
    // Recognize old content-addressed records by recomputing the full ID, never by prefix.
    for profile in current.values() {
        if generated(profile)? {
            ownership
                .owned
                .entry(profile.id.clone())
                .or_insert_with(|| profile.clone());
        }
    }
    for (id, expected) in &ownership.owned {
        if current.get(id) != Some(expected) {
            return Err(format!(
                "packaged routing profile changed outside configuration: {id}"
            ));
        }
    }
    for (id, profile) in &desired {
        if let Some(existing) = current.get(id) {
            if !ownership.owned.contains_key(id) && normalize(existing.clone()) != *profile {
                return Err(format!("routing profile identity is immutable: {id}"));
            }
        }
    }
    // Normalize legacy metadata in the same transaction as desired-state application.
    for (id, existing) in &mut current {
        let normalized = normalize(existing.clone());
        if normalized != *existing {
            *existing = normalized;
            if ownership.owned.contains_key(id) {
                ownership.owned.insert(id.clone(), existing.clone());
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
