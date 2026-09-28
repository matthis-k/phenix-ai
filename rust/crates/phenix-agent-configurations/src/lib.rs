#![forbid(unsafe_code)]

use phenix_core::{Authority, PluginExecution, PluginId, PluginManifest};

pub const BASIC_AGENT_CONFIGURATION: &str = "phenix.agent.basic";
pub const ADVANCED_AGENT_CONFIGURATION: &str = "phenix.agent.advanced";

const BASIC_AGENT_DEPENDENCIES: &[&str] = &[
    "phenix.agent-loop",
    "phenix.context",
    "phenix.execution",
    "phenix.harness.invocation-defaults",
    "phenix.models",
    "phenix.step-runner",
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
pub fn advanced_agent_configuration_manifest() -> PluginManifest {
    let dependencies = std::iter::once(BASIC_AGENT_CONFIGURATION)
        .chain(ADVANCED_AGENT_EXTENSIONS.iter().copied())
        .collect::<Vec<_>>();
    assembly_manifest(ADVANCED_AGENT_CONFIGURATION, &dependencies)
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
