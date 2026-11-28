//! Select model offers without conflating metadata with runtime authority.
//! Discovery is read-only; the host still approves the chosen revision,
//! exact import and component writes before publishing a generation.

use super::ModelProfile;
use crate::{CONTRACTS_PLUGIN, MODEL_CONTRACT};
use phenix_kernel::{Key, OfferedComponent, component_offers};
use phenix_plugin::{Code, Failure, PluginSnapshot};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOffer {
    pub component: OfferedComponent,
    pub profile: Option<ModelProfile>,
}

/// An explicit selector. Missing fields do not imply a default provider.
/// Ambiguous matches fail, even when their advertised metadata is identical.
#[derive(Clone, Debug, Default)]
pub struct ModelSelection {
    pub offer: Option<Key>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub required_capabilities: BTreeSet<String>,
}

impl ModelSelection {
    pub fn select(
        &self,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
    ) -> Result<ModelOffer, Failure> {
        let contract = Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT);
        let mut candidates = Vec::new();
        for snapshot in snapshots {
            for component in component_offers(&snapshot) {
                if component.contract != contract
                    || self
                        .offer
                        .as_ref()
                        .is_some_and(|key| key != &component.offer)
                {
                    continue;
                }
                let profile = snapshot
                    .get::<ModelProfile>(&component.offer.id)
                    .map(|profile| (*profile).clone());
                let needs_profile = self.provider.is_some()
                    || self.model.is_some()
                    || !self.required_capabilities.is_empty();
                if needs_profile
                    && !profile.as_ref().is_some_and(|profile| {
                        self.provider
                            .as_ref()
                            .is_none_or(|name| name == &profile.provider)
                            && self.model.as_ref().is_none_or(|name| name == &profile.model)
                            && self.required_capabilities.is_subset(&profile.capabilities)
                    })
                {
                    continue;
                }
                candidates.push(ModelOffer { component, profile });
            }
        }
        match candidates.len() {
            1 => Ok(candidates.pop().expect("one matching offer")),
            0 => Err(Failure::new(Code::MissingBinding, "no matching model offer")),
            _ => Err(Failure::new(
                Code::DescriptorInvalid,
                "ambiguous model selection",
            )),
        }
    }
}
