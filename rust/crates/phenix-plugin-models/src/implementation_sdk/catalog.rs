//! Provider catalog profiles retain their records for durable sessions when retired.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MANIFEST: &str = "configuration/provider-catalog-v1";

#[derive(Default, Serialize, Deserialize)]
struct ProviderOwnership {
    owned: BTreeMap<RoutingProfileId, RoutingProfile>,
    active: BTreeSet<RoutingProfileId>,
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    providers: BTreeMap<PluginId, ProviderOwnership>,
}

fn manifest(context: &ModelContext<'_, '_>) -> Result<(Option<Vec<u8>>, Manifest), String> {
    let bytes = read_raw(context, MANIFEST)?;
    let value: Manifest = bytes
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
    let mut owned = BTreeSet::new();
    let mut active = BTreeSet::new();
    for provider in manifest.providers.values() {
        owned.extend(provider.owned.keys().cloned());
        active.extend(provider.active.iter().cloned());
    }
    Ok((owned, active))
}

fn validate_profile(provider: &PluginId, profile: &RoutingProfile) -> Result<(), String> {
    if !profile.fallback_targets.is_empty() || !profile.callable_targets.is_empty() {
        return Err(format!(
            "provider catalog profile must contain exactly one target: {}",
            profile.id
        ));
    }
    if &profile.default_target.provider_plugin != provider {
        return Err(format!(
            "provider catalog profile {} belongs to {}, expected {provider}",
            profile.id, profile.default_target.provider_plugin
        ));
    }
    Ok(())
}

pub(super) fn publish(
    context: &ModelContext<'_, '_>,
    provider: PluginId,
    profiles: Vec<RoutingProfile>,
) -> Result<ModelResponse, String> {
    let (old_manifest, mut manifest) = manifest(context)?;
    let old_index = read_raw(context, PROFILE_INDEX)?;
    let mut current = load_profiles(context)?
        .into_iter()
        .map(|profile| (profile.id.clone(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut previous = BTreeMap::new();
    for (id, profile) in &current {
        let raw = read_raw(context, &profile_key(id))?
            .ok_or_else(|| format!("routing profile disappeared: {id}"))?;
        if decode_stored_profile(&raw)? != *profile {
            return Err(format!(
                "routing profile changed during catalog refresh: {id}"
            ));
        }
        previous.insert(id.clone(), raw);
    }

    let mut desired = BTreeMap::new();
    for profile in profiles {
        validate_profile(&provider, &profile)?;
        if desired.insert(profile.id.clone(), profile).is_some() {
            return Err(format!(
                "duplicate provider catalog routing profile for {provider}"
            ));
        }
    }

    {
        let ownership = manifest.providers.entry(provider.clone()).or_default();
        for (id, expected) in &ownership.owned {
            if current.get(id) != Some(expected) {
                return Err(format!(
                    "provider catalog routing profile changed outside catalog ownership: {id}"
                ));
            }
        }
        for id in desired.keys() {
            if current.contains_key(id) && !ownership.owned.contains_key(id) {
                return Err(format!(
                    "routing profile is already owned outside provider catalog: {id}"
                ));
            }
        }

        ownership.active = desired.keys().cloned().collect();
        for (id, profile) in desired {
            ownership.owned.insert(id.clone(), profile.clone());
            current.insert(id, profile);
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
        value: serde_json::to_vec(&manifest).map_err(|error| error.to_string())?,
    });

    context
        .kernel
        .transact_durable(&model_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    Ok(ModelResponse::Profiles {
        profiles: current.values().map(descriptor).collect(),
    })
}
