use crate::configuration::ExecutionConfigurationInterface;
use crate::{
    execution_manifest, AgentLoopCommand, AgentLoopResponse, ExecutionReviewInterface,
    AGENT_LOOP_SERVICE,
};
use phenix_core::{
    Authority, CapabilityId, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, PluginId,
};
use phenix_sdk::{
    DefaultInvocationInterface, ExecutionInterface, ExecutionResourceInterface,
    StepAttemptInterface, StepTransactionInterface, WorkspaceInterface,
};

const EXECUTION_COMPONENT: &str = "phenix.execution";
const AGENT_LOOP_COMPONENT: &str = "phenix.agent-loop";
const EXECUTION_PLUGIN: &str = "phenix.execution";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const WORKSPACE_WRITE: &str = "workspace.write";

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
pub fn agent_loop_component_id() -> ComponentId {
    ComponentId::parse(AGENT_LOOP_COMPONENT).expect("static agent loop component id is valid")
}

#[must_use]
pub fn execution_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let workspace_authority = maximum_authority.attenuate(&workspace_write_authority());
    let authority = execution_manifest(maximum_authority).maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: execution_component_id(),
        owner: PluginId::parse(EXECUTION_PLUGIN).expect("static plugin id is valid"),
        imports: vec![ComponentImport {
            interface: WorkspaceInterface::interface_id(),
            schema: WorkspaceInterface::schema(),
            required: false,
            authority: workspace_authority,
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
                interface: StepTransactionInterface::interface_id(),
                schema: StepTransactionInterface::schema(),
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
                interface: ExecutionReviewInterface::interface_id(),
                schema: ExecutionReviewInterface::schema(),
                priority: 100,
                required_authority: persistence_authority(),
            },
        ],
        maximum_authority: authority,
    }
}

#[must_use]
pub fn agent_loop_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: agent_loop_component_id(),
        owner: PluginId::parse(EXECUTION_PLUGIN).expect("static plugin id is valid"),
        imports: vec![ComponentImport {
            interface: DefaultInvocationInterface::interface_id(),
            schema: DefaultInvocationInterface::schema(),
            required: false,
            authority: maximum_authority.clone(),
        }],
        exports: vec![ComponentExport {
            interface: AgentLoopInterface::interface_id(),
            schema: AgentLoopInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority,
    }
}

fn persistence_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_SCHEMA).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_READ).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_WRITE).expect("static capability is valid"),
    ])
}

fn workspace_write_authority() -> Authority {
    Authority::new([CapabilityId::parse(WORKSPACE_WRITE).expect("static capability is valid")])
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
        assert_eq!(component.exports.len(), 6);
        assert_eq!(
            component.exports[0].interface,
            ExecutionInterface::interface_id()
        );
        assert_eq!(
            component.exports[1].interface,
            ExecutionResourceInterface::interface_id()
        );
        assert_eq!(
            component.exports[2].interface,
            StepAttemptInterface::interface_id()
        );
        assert_eq!(
            component.exports[3].interface,
            StepTransactionInterface::interface_id()
        );
        for export in &component.exports[..4] {
            assert_eq!(export.required_authority, persistence_authority());
        }
        assert_eq!(
            component.exports[4].interface,
            ExecutionConfigurationInterface::interface_id()
        );
        assert_eq!(
            component.exports[4].required_authority,
            Authority::default()
        );
        assert_eq!(
            component.exports[5].interface,
            ExecutionReviewInterface::interface_id()
        );
        assert_eq!(
            component.exports[5].required_authority,
            persistence_authority()
        );
        assert_eq!(component.imports.len(), 1);
        assert_eq!(
            component.imports[0].interface,
            WorkspaceInterface::interface_id()
        );
        assert!(!component.imports[0].required);
        assert!(graph.component(&execution_component_id()).is_some());
    }

    #[test]
    fn review_workspace_import_is_limited_to_package_ceiling() {
        let write = CapabilityId::parse(WORKSPACE_WRITE).unwrap();
        let component = execution_component_manifest(Authority::new([write.clone()]));
        assert!(component.imports[0].authority.permits(&write));
    }

    #[test]
    fn agent_loop_component_owns_invocation_dependency_separately() {
        let network = CapabilityId::parse("network.model").unwrap();
        let component = agent_loop_component_manifest(Authority::new([network.clone()]));

        assert_eq!(component.id, agent_loop_component_id());
        assert_eq!(component.imports.len(), 1);
        assert!(!component.imports[0].required);
        assert_eq!(
            component.imports[0].interface,
            DefaultInvocationInterface::interface_id()
        );
        assert!(component.imports[0].authority.permits(&network));
        assert_eq!(component.exports.len(), 1);
        assert_eq!(
            component.exports[0].interface,
            AgentLoopInterface::interface_id()
        );
        assert_eq!(
            component.exports[0].required_authority,
            Authority::default()
        );
    }

    #[test]
    fn invocation_import_does_not_inherit_execution_persistence_authority() {
        let network = CapabilityId::parse("network.model").unwrap();
        let component = agent_loop_component_manifest(Authority::new([network.clone()]));
        assert!(component.imports[0].authority.permits(&network));
        for capability in persistence_authority().capabilities() {
            assert!(!component.imports[0].authority.permits(capability));
        }
    }
}
