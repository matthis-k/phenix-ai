use crate::{cli_manifest, CLI_AUTH_STATE_SERVICE, CLI_DISCOVER_SERVICE, CLI_VERSION_SERVICE};
use phenix_core::{
    Authority, CapabilityId, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, PluginId,
};
use phenix_sdk::WorkspaceInterface;

const CLI_COMPONENT: &str = "phenix.command-toolbelt";
const CLI_PLUGIN: &str = "phenix.command-toolbelt";
const WORKSPACE_SHELL: &str = "workspace.shell";

pub struct CliDiscoverInterface;
pub struct CliVersionInterface;
pub struct CliAuthStateInterface;

macro_rules! cli_interface {
    ($type:ty, $service:expr) => {
        impl ComponentInterface for $type {
            fn interface_id() -> InterfaceId {
                InterfaceId::parse($service).expect("static CLI interface id is valid")
            }

            fn schema() -> phenix_core::InterfaceSchema {
                phenix_core::InterfaceSchema::of::<crate::CliProbeRequest, crate::CliDescriptor>()
            }
        }
    };
}

cli_interface!(CliDiscoverInterface, CLI_DISCOVER_SERVICE);
cli_interface!(CliVersionInterface, CLI_VERSION_SERVICE);
cli_interface!(CliAuthStateInterface, CLI_AUTH_STATE_SERVICE);

#[must_use]
pub fn cli_component_id() -> ComponentId {
    ComponentId::parse(CLI_COMPONENT).expect("static CLI component id is valid")
}

#[must_use]
pub fn cli_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let authority = cli_manifest(maximum_authority).maximum_authority;
    let shell =
        Authority::new([CapabilityId::parse(WORKSPACE_SHELL)
            .expect("static workspace shell capability is valid")]);
    ComponentManifest {
        listeners: Vec::new(),
        id: cli_component_id(),
        owner: PluginId::parse(CLI_PLUGIN).expect("static CLI plugin id is valid"),
        imports: vec![ComponentImport {
            interface: WorkspaceInterface::interface_id(),
            schema: WorkspaceInterface::schema(),
            required: true,
            authority: shell,
        }],
        exports: vec![
            ComponentExport {
                interface: CliDiscoverInterface::interface_id(),
                schema: CliDiscoverInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: CliVersionInterface::interface_id(),
                schema: CliVersionInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: CliAuthStateInterface::interface_id(),
                schema: CliAuthStateInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority: authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ComponentGraphError, ResolvedComponentGraph};
    use phenix_plugin_environment_local::{local_environment_component_manifest, local_environment_manifest};
    use phenix_plugin_workspace::{workspace_component_manifest, workspace_manifest};

    #[test]
    fn command_toolbelt_uses_canonical_runtime_identity() {
        assert_eq!(
            cli_manifest(Authority::default()).id.as_str(),
            "phenix.command-toolbelt"
        );
        assert_eq!(cli_component_id().as_str(), "phenix.command-toolbelt");
        assert_eq!(
            crate::cli_discover_service().as_str(),
            "phenix.command-toolbelt.discover@1"
        );
        assert_eq!(
            crate::cli_version_service().as_str(),
            "phenix.command-toolbelt.version@1"
        );
        assert_eq!(
            crate::cli_auth_state_service().as_str(),
            "phenix.command-toolbelt.auth-state@1"
        );
    }

    #[test]
    fn cli_requires_workspace_binding_before_activation() {
        let shell = Authority::new([CapabilityId::parse(WORKSPACE_SHELL).unwrap()]);
        let error = ResolvedComponentGraph::compile(
            [workspace_manifest(), cli_manifest(shell.clone())],
            [cli_component_manifest(shell.clone())],
            &shell,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ComponentGraphError::MissingRequiredImport { component, interface }
                if component == cli_component_id()
                    && interface == WorkspaceInterface::interface_id()
        ));
    }

    #[test]
    fn cli_workspace_import_is_attenuated_by_cli_authority() {
        let shell_capability = CapabilityId::parse(WORKSPACE_SHELL).unwrap();
        let shell = Authority::new([shell_capability.clone()]);
        let graph = ResolvedComponentGraph::compile(
            [
                local_environment_manifest(),
                workspace_manifest(),
                cli_manifest(shell.clone()),
            ],
            [
                local_environment_component_manifest(),
                workspace_component_manifest(),
                cli_component_manifest(shell.clone()),
            ],
            &shell,
        )
        .unwrap();
        let handle = graph
            .import_handle(&cli_component_id(), &WorkspaceInterface::interface_id())
            .unwrap()
            .unwrap();

        assert!(handle.effective_authority().permits(&shell_capability));
        assert_eq!(handle.exporter(), &workspace_component_manifest().id);
    }
}
