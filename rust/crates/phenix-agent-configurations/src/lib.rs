#![forbid(unsafe_code)]

use phenix_core::{Authority, PluginExecution, PluginId, PluginManifest};

pub const BASIC_AGENT_CONFIGURATION: &str = "phenix.agent.basic";
pub const ADVANCED_AGENT_CONFIGURATION: &str = "phenix.agent.advanced";
pub const BASIC_PRODUCT_CONFIGURATION: &str = "phenix.product.basic";
pub const FULL_PRODUCT_CONFIGURATION: &str = "phenix.product.full";

const BASIC_AGENT_DEPENDENCIES: &[&str] = &[
    "phenix.agent-loop",
    "phenix.context",
    "phenix.execution",
    "phenix.harness.invocation-defaults",
    "phenix.models",
    "phenix.step-runner",
];

const BASIC_PRODUCT_DEPENDENCIES: &[&str] = &[
    BASIC_AGENT_CONFIGURATION,
    "phenix.api",
    "phenix.options",
    "phenix.providers",
    "openai-codex",
];

const ADVANCED_AGENT_EXTENSIONS: &[&str] = &[
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

#[must_use]
pub fn basic_agent_configuration_manifest() -> PluginManifest {
    assembly_manifest(BASIC_AGENT_CONFIGURATION, BASIC_AGENT_DEPENDENCIES)
}

#[must_use]
pub fn basic_product_configuration_manifest() -> PluginManifest {
    assembly_manifest(BASIC_PRODUCT_CONFIGURATION, BASIC_PRODUCT_DEPENDENCIES)
}

#[must_use]
pub fn advanced_agent_configuration_manifest() -> PluginManifest {
    let dependencies = std::iter::once(BASIC_AGENT_CONFIGURATION)
        .chain(ADVANCED_AGENT_EXTENSIONS.iter().copied())
        .collect::<Vec<_>>();
    assembly_manifest(ADVANCED_AGENT_CONFIGURATION, &dependencies)
}

#[must_use]
pub fn full_product_configuration_manifest() -> PluginManifest {
    assembly_manifest(
        FULL_PRODUCT_CONFIGURATION,
        &[ADVANCED_AGENT_CONFIGURATION, "phenix.providers", "openai-codex"],
    )
}

fn assembly_manifest(id: &str, dependencies: &[&str]) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::ResourceOnly,
        dependencies: dependencies.iter().copied().map(plugin).collect(),
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
    use std::collections::BTreeSet;

    fn dependency_ids(manifest: PluginManifest) -> BTreeSet<String> {
        manifest
            .dependencies
            .into_iter()
            .map(|dependency| dependency.as_str().to_owned())
            .collect()
    }

    #[test]
    fn basic_agent_is_option_free() {
        let dependencies = dependency_ids(basic_agent_configuration_manifest());
        assert!(dependencies.contains("phenix.agent-loop"));
        for optional in ["phenix.options", "phenix.memory", "phenix.planning"] {
            assert!(!dependencies.contains(optional));
        }
    }

    #[test]
    fn advanced_agent_extends_basic_instead_of_copying_it() {
        let basic = dependency_ids(basic_agent_configuration_manifest());
        let advanced = dependency_ids(advanced_agent_configuration_manifest());

        assert!(advanced.contains(BASIC_AGENT_CONFIGURATION));
        for dependency in basic {
            assert!(
                !advanced.contains(&dependency),
                "advanced configuration repeated basic dependency {dependency}"
            );
        }
    }

    #[test]
    fn product_configurations_add_frontend_and_provider_capabilities() {
        let basic = dependency_ids(basic_product_configuration_manifest());
        for required in [
            BASIC_AGENT_CONFIGURATION,
            "phenix.api",
            "phenix.options",
            "phenix.providers",
            "openai-codex",
        ] {
            assert!(basic.contains(required), "basic product missed {required}");
        }

        let full = dependency_ids(full_product_configuration_manifest());
        assert!(full.contains(ADVANCED_AGENT_CONFIGURATION));
        assert!(full.contains("phenix.providers"));
        assert!(full.contains("openai-codex"));
    }

    #[test]
    fn advanced_agent_adds_optional_agent_services() {
        let dependencies = dependency_ids(advanced_agent_configuration_manifest());
        for optional in [
            "phenix.options",
            "phenix.memory",
            "phenix.planning",
            "phenix.repository-workers",
            "phenix.session-tree",
            "phenix.language",
            "phenix.efficiency-evaluation",
            "phenix.jobs",
            "phenix.hooks",
            "phenix.debug",
        ] {
            assert!(dependencies.contains(optional), "missing {optional}");
        }
    }
}
