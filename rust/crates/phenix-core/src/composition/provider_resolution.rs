use crate::{ComponentId, InterfaceId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Why the kernel selected the primary provider in a resolved graph generation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSelectionReason {
    ExplicitBinding,
    Priority,
    StableIdentity,
}

/// Why dispatch entered a graph-pinned fallback instead of the primary provider.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFallbackReason {
    PrimaryUnavailable,
}

/// Product-owned inputs that affect provider selection during candidate resolution.
///
/// Plugin-authored export priority remains implementation metadata. Effective
/// provider preference comes from this policy. With no policy entry, eligible
/// providers have equal preference and stable component identity breaks ties.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCompositionPolicy {
    interfaces: BTreeMap<InterfaceId, InterfaceProviderPolicy>,
}

/// Generic policy for one interface. Interface fallback permission and product
/// fallback enablement are separate gates so product policy cannot invent
/// fallback semantics for a contract that does not allow them.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct InterfaceProviderPolicy {
    #[serde(default)]
    explicit: Option<ComponentId>,
    #[serde(default)]
    priorities: BTreeMap<ComponentId, i32>,
    #[serde(default)]
    disabled: BTreeSet<ComponentId>,
    #[serde(default)]
    interface_allows_fallback: bool,
    #[serde(default)]
    fallback_enabled: bool,
}

impl ProviderCompositionPolicy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_priority(
        mut self,
        interface: InterfaceId,
        provider: ComponentId,
        priority: i32,
    ) -> Self {
        self.interfaces
            .entry(interface)
            .or_default()
            .priorities
            .insert(provider, priority);
        self
    }

    #[must_use]
    pub fn with_explicit_binding(mut self, interface: InterfaceId, provider: ComponentId) -> Self {
        self.interfaces.entry(interface).or_default().explicit = Some(provider);
        self
    }

    #[must_use]
    pub fn with_disabled_provider(mut self, interface: InterfaceId, provider: ComponentId) -> Self {
        self.interfaces
            .entry(interface)
            .or_default()
            .disabled
            .insert(provider);
        self
    }

    /// Declare that the interface contract itself permits pre-dispatch fallback.
    #[must_use]
    pub fn with_interface_fallback(mut self, interface: InterfaceId) -> Self {
        self.interfaces
            .entry(interface)
            .or_default()
            .interface_allows_fallback = true;
        self
    }

    /// Enable fallback in product composition policy. This has no effect unless
    /// the interface contract also permits fallback.
    #[must_use]
    pub fn with_fallback_enabled(mut self, interface: InterfaceId) -> Self {
        self.interfaces
            .entry(interface)
            .or_default()
            .fallback_enabled = true;
        self
    }

    pub(crate) fn explicit_binding(&self, interface: &InterfaceId) -> Option<&ComponentId> {
        self.interfaces
            .get(interface)
            .and_then(|policy| policy.explicit.as_ref())
    }

    pub(crate) fn provider_enabled(&self, interface: &InterfaceId, provider: &ComponentId) -> bool {
        !self
            .interfaces
            .get(interface)
            .is_some_and(|policy| policy.disabled.contains(provider))
    }

    pub(crate) fn effective_priority(
        &self,
        interface: &InterfaceId,
        provider: &ComponentId,
    ) -> i32 {
        self.interfaces
            .get(interface)
            .and_then(|policy| policy.priorities.get(provider))
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn has_priority(&self, interface: &InterfaceId, provider: &ComponentId) -> bool {
        self.interfaces
            .get(interface)
            .is_some_and(|policy| policy.priorities.contains_key(provider))
    }

    pub(crate) fn fallback_enabled(&self, interface: &InterfaceId) -> bool {
        self.interfaces
            .get(interface)
            .is_some_and(|policy| policy.interface_allows_fallback && policy.fallback_enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface() -> InterfaceId {
        InterfaceId::parse("fixture.provider@1").unwrap()
    }

    fn provider(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    #[test]
    fn fallback_requires_contract_and_product_policy() {
        let interface = interface();
        assert!(!ProviderCompositionPolicy::new()
            .with_interface_fallback(interface.clone())
            .fallback_enabled(&interface));
        assert!(!ProviderCompositionPolicy::new()
            .with_fallback_enabled(interface.clone())
            .fallback_enabled(&interface));
        assert!(ProviderCompositionPolicy::new()
            .with_interface_fallback(interface.clone())
            .with_fallback_enabled(interface.clone())
            .fallback_enabled(&interface));
    }

    #[test]
    fn product_priority_and_disable_are_provider_specific() {
        let interface = interface();
        let preferred = provider("preferred");
        let disabled = provider("disabled");
        let policy = ProviderCompositionPolicy::new()
            .with_priority(interface.clone(), preferred.clone(), 7)
            .with_disabled_provider(interface.clone(), disabled.clone());

        assert_eq!(policy.effective_priority(&interface, &preferred), 7);
        assert!(policy.has_priority(&interface, &preferred));
        assert!(!policy.provider_enabled(&interface, &disabled));
    }
}
