#![forbid(unsafe_code)]

//! Declarative reference product profiles. These are defaults, not concrete
//! package dependencies: selecting a profile must not make its implementations
//! impossible to replace through the canonical Phenix resolver.

use phenix_core::{Authority, PluginExecution, PluginId, PluginManifest};
use std::collections::BTreeSet;

pub const BASIC_AGENT_CONFIGURATION: &str = "phenix.agent.basic";
pub const ADVANCED_AGENT_CONFIGURATION: &str = "phenix.agent.advanced";
pub const BASIC_PRODUCT_CONFIGURATION: &str = "phenix.product.basic";
pub const FULL_PRODUCT_CONFIGURATION: &str = "phenix.product.full";

const BASIC_AGENT_DEFAULTS: &[&str] = &[
    // Basic and Advanced use the selected topology and naive node providers.
    // Legacy agent-loop implementations remain explicit, replaceable choices.
    "phenix.agent-topology",
    "phenix.basic-agent-nodes",
    "phenix.application-agent-tools",
    "phenix.basic-skills",
    "phenix.context",
    "phenix.execution",
    "phenix.harness.invocation-defaults",
    "phenix.models",
    "phenix.step-runner",
];

const BASIC_PRODUCT_DEFAULTS: &[&str] = &[
    BASIC_AGENT_CONFIGURATION,
    "phenix.api",
    "phenix.environment.local",
    "phenix.options",
    "phenix.providers",
    "phenix.workspace",
    "openai-codex",
];

const ADVANCED_AGENT_DEFAULTS: &[&str] = &[
    BASIC_AGENT_CONFIGURATION,
    "phenix.api",
    "phenix.artifacts",
    "phenix.command-toolbelt",
    "phenix.debug",
    "phenix.efficiency-evaluation",
    "phenix.environment.local",
    "phenix.frontend-services",
    "phenix.hooks",
    "phenix.jobs",
    "phenix.language",
    "phenix.memory",
    "phenix.options",
    "phenix.planning",
    "phenix.repository-workers",
    "phenix.session-tree",
    "phenix.sessions",
    "phenix.workspace",
];

const FULL_PRODUCT_DEFAULTS: &[&str] = &[
    BASIC_PRODUCT_CONFIGURATION,
    ADVANCED_AGENT_CONFIGURATION,
    "phenix.providers",
    "openai-codex",
    "phenix.interactive-ui",
];

/// Named first-party profile inheritance and default selections.
///
/// Unlike PluginManifest.dependencies, these declarations can be overridden
/// before the actual hard implementation dependency closure is evaluated.
#[must_use]
pub fn profile_defaults(id: &str) -> Option<&'static [&'static str]> {
    match id {
        BASIC_AGENT_CONFIGURATION => Some(BASIC_AGENT_DEFAULTS),
        ADVANCED_AGENT_CONFIGURATION => Some(ADVANCED_AGENT_DEFAULTS),
        BASIC_PRODUCT_CONFIGURATION => Some(BASIC_PRODUCT_DEFAULTS),
        FULL_PRODUCT_CONFIGURATION => Some(FULL_PRODUCT_DEFAULTS),
        _ => None,
    }
}

/// Expand named profile defaults, excluding providers explicitly disabled by
/// the caller. Does not resolve plugin manifests or contract bindings.
/// The latter are always validated by the canonical Phenix resolver.
#[must_use]
pub fn expand_profile_defaults(
    selected: &BTreeSet<String>,
    excluded: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut expanded = BTreeSet::new();
    let mut pending = selected.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if excluded.contains(&id) || !expanded.insert(id.clone()) {
            continue;
        }
        if let Some(defaults) = profile_defaults(&id) {
            pending.extend(defaults.iter().map(|id| (*id).to_owned()));
        }
    }
    expanded
}

#[must_use]
pub fn basic_agent_configuration_manifest() -> PluginManifest {
    assembly_manifest(BASIC_AGENT_CONFIGURATION)
}

#[must_use]
pub fn basic_product_configuration_manifest() -> PluginManifest {
    assembly_manifest(BASIC_PRODUCT_CONFIGURATION)
}

#[must_use]
pub fn advanced_agent_configuration_manifest() -> PluginManifest {
    assembly_manifest(ADVANCED_AGENT_CONFIGURATION)
}

#[must_use]
pub fn full_product_configuration_manifest() -> PluginManifest {
    assembly_manifest(FULL_PRODUCT_CONFIGURATION)
}

fn assembly_manifest(id: &str) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::ResourceOnly,
        // Product defaults must never masquerade as implementation dependencies.
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn plugin(id: &str) -> PluginId {
    PluginId::parse(id).expect("static agent configuration plugin id is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_contribute_defaults_not_hard_manifest_dependencies() {
        for profile in [
            BASIC_AGENT_CONFIGURATION,
            ADVANCED_AGENT_CONFIGURATION,
            BASIC_PRODUCT_CONFIGURATION,
            FULL_PRODUCT_CONFIGURATION,
        ] {
            let manifest = assembly_manifest(profile);
            assert!(
                manifest.dependencies.is_empty(),
                "{profile} must remain replaceable"
            );
            assert!(profile_defaults(profile).is_some());
        }
        assert!(profile_defaults("unrelated.third-party-plugin").is_none());
    }

    #[test]
    fn basic_agent_is_option_free() {
        let dependencies = expand_profile_defaults(
            &BTreeSet::from([BASIC_AGENT_CONFIGURATION.to_owned()]),
            &BTreeSet::new(),
        );
        assert!(!dependencies.contains("phenix.agent-loop"));
        assert!(dependencies.contains("phenix.agent-topology"));
        assert!(dependencies.contains("phenix.basic-agent-nodes"));
        assert!(dependencies.contains("phenix.application-agent-tools"));
        assert!(dependencies.contains("phenix.basic-skills"));
        for optional in ["phenix.options", "phenix.memory", "phenix.planning"] {
            assert!(!dependencies.contains(optional));
        }
    }

    #[test]
    fn excluding_legacy_loop_keeps_independent_declarative_defaults() {
        let selected = BTreeSet::from([BASIC_AGENT_CONFIGURATION.to_owned()]);
        let excluded = BTreeSet::from(["phenix.agent-loop".to_owned()]);
        let defaults = expand_profile_defaults(&selected, &excluded);
        assert!(!defaults.contains("phenix.agent-loop"));
        assert!(defaults.contains("phenix.agent-topology"));
        assert!(defaults.contains("phenix.basic-agent-nodes"));
        assert!(defaults.contains("phenix.application-agent-tools"));

        // Exclusions act independently: the declarative topology and Basic
        // node implementation are replaceable without reinstating the loop.
        let excluded = BTreeSet::from([
            "phenix.agent-loop".to_owned(),
            "phenix.basic-agent-nodes".to_owned(),
        ]);
        let defaults = expand_profile_defaults(&selected, &excluded);
        assert!(defaults.contains("phenix.agent-topology"));
        assert!(!defaults.contains("phenix.basic-agent-nodes"));
    }

    #[test]
    fn advanced_agent_inherits_basic_without_repeating_its_implementations() {
        let advanced = profile_defaults(ADVANCED_AGENT_CONFIGURATION).unwrap();
        assert!(advanced.contains(&BASIC_AGENT_CONFIGURATION));
        assert!(!advanced.contains(&"phenix.agent-loop"));
        let effective = expand_profile_defaults(
            &BTreeSet::from([ADVANCED_AGENT_CONFIGURATION.to_owned()]),
            &BTreeSet::new(),
        );
        assert!(effective.contains(BASIC_AGENT_CONFIGURATION));
        assert!(!effective.contains("phenix.agent-loop"));
        assert!(effective.contains("phenix.agent-topology"));
        assert!(effective.contains("phenix.basic-agent-nodes"));
        assert!(effective.contains("phenix.memory"));
    }

    #[test]
    fn products_inherit_agent_profiles_and_provider_defaults() {
        let basic = profile_defaults(BASIC_PRODUCT_CONFIGURATION).unwrap();
        for required in [
            BASIC_AGENT_CONFIGURATION,
            "phenix.api",
            "phenix.options",
            "phenix.providers",
            "phenix.workspace",
            "openai-codex",
        ] {
            assert!(basic.contains(&required), "basic product missed {required}");
        }
        let full = profile_defaults(FULL_PRODUCT_CONFIGURATION).unwrap();
        assert!(full.contains(&BASIC_PRODUCT_CONFIGURATION));
        assert!(full.contains(&ADVANCED_AGENT_CONFIGURATION));
        assert!(full.contains(&"phenix.providers"));
        assert!(full.contains(&"openai-codex"));
        assert!(full.contains(&"phenix.interactive-ui"));
        assert!(!basic.contains(&"phenix.interactive-ui"));

        let expanded = expand_profile_defaults(
            &BTreeSet::from([FULL_PRODUCT_CONFIGURATION.to_owned()]),
            &BTreeSet::new(),
        );
        assert!(expanded.contains(BASIC_PRODUCT_CONFIGURATION));
        assert!(expanded.contains(BASIC_AGENT_CONFIGURATION));
        assert!(expanded.contains(ADVANCED_AGENT_CONFIGURATION));
    }

    #[test]
    fn disabling_an_inherited_default_does_not_remove_the_profile() {
        let selected = BTreeSet::from([FULL_PRODUCT_CONFIGURATION.to_owned()]);
        let excluded = BTreeSet::from(["phenix.memory".to_owned()]);
        let expanded = expand_profile_defaults(&selected, &excluded);
        assert!(expanded.contains(FULL_PRODUCT_CONFIGURATION));
        assert!(expanded.contains(ADVANCED_AGENT_CONFIGURATION));
        assert!(expanded.contains(BASIC_AGENT_CONFIGURATION));
        assert!(!expanded.contains("phenix.memory"));
        assert!(expanded.contains("phenix.planning"));
    }

    #[test]
    fn arbitrary_third_party_plugins_are_not_expanded_as_profiles() {
        let selected = BTreeSet::from(["acme.agent".to_owned()]);
        assert_eq!(
            expand_profile_defaults(&selected, &BTreeSet::new()),
            selected
        );
    }
}
