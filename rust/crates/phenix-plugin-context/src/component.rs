use crate::context_manifest;
use phenix_core::{
    ComponentExport, ComponentId, ComponentImport, ComponentInterface, ComponentManifest, PluginId,
};
use phenix_sdk::{
    ContextCompactionInterface, ContextExpansionInterface, ContextInterface, ContextReducerInterface,
    ExecutionInterface,
};

const CONTEXT_COMPONENT: &str = "phenix.context";
const CONTEXT_PLUGIN: &str = "phenix.context";

#[must_use]
pub fn context_component_id() -> ComponentId {
    ComponentId::parse(CONTEXT_COMPONENT).expect("static component id is valid")
}

fn optional_import<I: ComponentInterface>(authority: &phenix_core::Authority) -> ComponentImport {
    ComponentImport {
        interface: I::interface_id(),
        schema: I::schema(),
        required: false,
        authority: authority.clone(),
    }
}

#[must_use]
pub fn context_component_manifest() -> ComponentManifest {
    let authority = context_manifest().maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: context_component_id(),
        owner: PluginId::parse(CONTEXT_PLUGIN).expect("static plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: ExecutionInterface::interface_id(),
                schema: ExecutionInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            optional_import::<ContextCompactionInterface>(&authority),
            optional_import::<ContextExpansionInterface>(&authority),
            ComponentImport {
                interface: ContextReducerInterface::interface_id(),
                schema: ContextReducerInterface::schema(),
                required: false,
                authority: phenix_core::Authority::default(),
            },
        ],
        exports: vec![ComponentExport {
            interface: ContextInterface::interface_id(),
            schema: ContextInterface::schema(),
            priority: 100,
            required_authority: authority.clone(),
        }],
        maximum_authority: authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Authority, ComponentGraphError, ResolvedComponentGraph};
    use phenix_plugin_execution::{execution_component_manifest, execution_manifest};

    fn authority() -> Authority {
        context_manifest().maximum_authority
    }

    #[test]
    fn context_requires_execution_binding_before_activation() {
        let error = ResolvedComponentGraph::compile(
            [execution_manifest(authority()), context_manifest()],
            [context_component_manifest()],
            &authority(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ComponentGraphError::MissingRequiredImport { component, interface }
                if component == context_component_id()
                    && interface == ExecutionInterface::interface_id()
        ));
    }

    #[test]
    fn context_execution_import_binds_to_execution_component() {
        let graph = ResolvedComponentGraph::compile(
            [execution_manifest(authority()), context_manifest()],
            [
                execution_component_manifest(authority()),
                context_component_manifest(),
            ],
            &authority(),
        )
        .unwrap();
        let handle = graph
            .import_handle(&context_component_id(), &ExecutionInterface::interface_id())
            .unwrap()
            .unwrap();
        assert_eq!(
            handle.exporter(),
            &execution_component_manifest(authority()).id
        );
        assert_eq!(handle.effective_authority(), &authority());
        assert!(graph
            .import_handle(
                &context_component_id(),
                &ContextCompactionInterface::interface_id()
            )
            .unwrap()
            .is_none());
        assert!(graph
            .import_handle(
                &context_component_id(),
                &ContextExpansionInterface::interface_id()
            )
            .unwrap()
            .is_none());
        assert!(graph
            .import_handle(
                &context_component_id(),
                &ContextReducerInterface::interface_id()
            )
            .unwrap()
            .is_none());
        let reducer_import = context_component_manifest()
            .imports
            .into_iter()
            .find(|import| import.interface == ContextReducerInterface::interface_id())
            .expect("context declares the optional reducer backend");
        assert_eq!(reducer_import.authority, Authority::default());
    }
}
