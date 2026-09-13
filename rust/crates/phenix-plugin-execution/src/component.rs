use crate::configuration::ExecutionConfigurationInterface;
use crate::{execution_manifest, AgentLoopCommand, AgentLoopResponse, AGENT_LOOP_SERVICE};
use phenix_core::{
    Authority, CapabilityId, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, PluginId,
};
use phenix_sdk::{
    ExecutionInterface, ExecutionResourceInterface, ModelRoutingInterface, StepAttemptInterface,
};

const EXECUTION_COMPONENT: &str = "phenix.execution";
const EXECUTION_PLUGIN: &str = "phenix.execution";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

pub struct AgentLoopInterface;

impl ComponentInterface for AgentLoopInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_LOOP_SERVICE).expect("static agent loop interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentLoopCommand, AgentLoopResponse>()
    }
}

#[must_use]
pub fn execution_component_id() -> ComponentId {
    ComponentId::parse(EXECUTION_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn execution_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let model_authority = maximum_authority.clone();
    let authority = execution_manifest(maximum_authority).maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: execution_component_id(),
        owner: PluginId::parse(EXECUTION_PLUGIN).expect("static plugin id is valid"),
        imports: vec![ComponentImport {
            interface: ModelRoutingInterface::interface_id(),
            schema: ModelRoutingInterface::schema(),
            required: false,
            authority: model_authority,
        }],
        exports: vec![
            ComponentExport {
                interface: ExecutionInterface::interface_id(),
                schema: ExecutionInterface::schema(),
                priority: 100,
                required_authority: persistence_authority(),
            },
            ComponentExport {
                interface: ExecutionResourceInterface::interface_id(),
                schema: ExecutionResourceInterface::schema(),
                priority: 100,
                required_authority: persistence_authority(),
            },
            ComponentExport {
                interface: StepAttemptInterface::interface_id(),
                schema: StepAttemptInterface::schema(),
                priority: 100,
                required_authority: persistence_authority(),
            },
            ComponentExport {
                interface: ExecutionConfigurationInterface::interface_id(),
                schema: ExecutionConfigurationInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentLoopInterface::interface_id(),
                schema: AgentLoopInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority: authority,
    }
}

fn persistence_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_SCHEMA).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_READ).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_WRITE).expect("static capability is valid"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::ResolvedComponentGraph;

    #[test]
    fn execution_component_separates_package_ceiling_from_interface_minimum() {
        let capability = CapabilityId::parse("fixture.execution").unwrap();
        let authority = Authority::new([capability.clone()]);
        let plugin = execution_manifest(authority.clone());
        let component = execution_component_manifest(authority);
        let graph = ResolvedComponentGraph::compile(
            [plugin.clone()],
            [component.clone()],
            &plugin.maximum_authority,
        )
        .unwrap();

        assert_eq!(component.owner, plugin.id);
        assert!(component.maximum_authority.permits(&capability));
        assert_eq!(component.exports[0].interface, ExecutionInterface::interface_id());
        assert_eq!(
            component.exports[1].interface,
            ExecutionResourceInterface::interface_id()
        );
        assert_eq!(component.exports[2].interface, StepAttemptInterface::interface_id());
        for export in &component.exports[..3] {
            assert_eq!(export.required_authority, persistence_authority());
        }
        assert_eq!(
            component.exports[3].interface,
            ExecutionConfigurationInterface::interface_id()
        );
        assert_eq!(component.exports[3].required_authority, Authority::default());
        assert_eq!(component.exports[4].interface, AgentLoopInterface::interface_id());
        assert_eq!(component.exports[4].required_authority, Authority::default());
        assert_eq!(component.imports.len(), 1);
        assert!(!component.imports[0].required);
        assert_eq!(component.imports[0].interface, ModelRoutingInterface::interface_id());
        assert!(graph.component(&execution_component_id()).is_some());
    }

    #[test]
    fn model_import_does_not_inherit_execution_persistence_authority() {
        let network = CapabilityId::parse("network.model").unwrap();
        let component = execution_component_manifest(Authority::new([network.clone()]));
        assert!(component.imports[0].authority.permits(&network));
        for capability in persistence_authority().capabilities() {
            assert!(!component.imports[0].authority.permits(capability));
        }
    }
}
