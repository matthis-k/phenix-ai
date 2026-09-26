use crate::configuration::ExecutionConfigurationInterface;
use crate::{execution_manifest, ExecutionReviewInterface};
use phenix_core::{
    Authority, CapabilityId, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, PluginId,
};
use phenix_sdk::{
    ExecutionInterface, ExecutionResourceInterface, StepAttemptInterface,
    StepTransactionInterface, WorkspaceInterface,
};

const EXECUTION_COMPONENT: &str = "phenix.execution";
const EXECUTION_PLUGIN: &str = "phenix.execution";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const WORKSPACE_WRITE: &str = "workspace.write";


#[must_use]
pub fn execution_component_id() -> ComponentId {
    ComponentId::parse(EXECUTION_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn execution_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let workspace_authority = maximum_authority.attenuate(&workspace_write_authority());
    let authority = execution_manifest(maximum_authority).maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: execution_component_id(),
        owner: PluginId::parse(EXECUTION_PLUGIN).expect("static plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: WorkspaceInterface::interface_id(),
                schema: WorkspaceInterface::schema(),
                required: false,
                authority: workspace_authority,
            },
            ComponentImport {
                interface: phenix_sdk::ModelRoutingInterface::interface_id(),
                schema: phenix_sdk::ModelRoutingInterface::schema(),
                required: false,
                authority: persistence_authority(),
            },
        ],
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
                required_authority: attempt_read_authority(),
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

fn persistence_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_SCHEMA).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_READ).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_WRITE).expect("static capability is valid"),
    ])
}

fn attempt_read_authority() -> Authority {
    Authority::new([CapabilityId::parse(PERSISTENCE_READ).expect("static capability is valid")])
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
        assert_eq!(
            component.exports[0].required_authority,
            persistence_authority()
        );
        assert_eq!(
            component.exports[1].required_authority,
            persistence_authority()
        );
        assert_eq!(
            component.exports[2].required_authority,
            attempt_read_authority()
        );
        assert_eq!(
            component.exports[3].required_authority,
            persistence_authority()
        );
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
        assert_eq!(component.imports.len(), 2);
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

}
