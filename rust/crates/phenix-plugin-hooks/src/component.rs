use crate::{HOOK_SERVICE, hook_manifest};
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, PluginId,
};
use phenix_sdk::{ContextInterface, ExecutionInterface};

const HOOK_COMPONENT: &str = "phenix.hooks";
const HOOK_PLUGIN: &str = "phenix.hooks";

pub struct HookInterface;

impl ComponentInterface for HookInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(HOOK_SERVICE).expect("static hook interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<crate::HookCommand, crate::HookResponse>()
    }
}

#[must_use]
pub fn hook_component_id() -> ComponentId {
    ComponentId::parse(HOOK_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn hook_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let authority = hook_manifest(maximum_authority).maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: hook_component_id(),
        owner: PluginId::parse(HOOK_PLUGIN).expect("static plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: ContextInterface::interface_id(),
                schema: ContextInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: ExecutionInterface::interface_id(),
                schema: ExecutionInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
        ],
        exports: vec![ComponentExport {
            interface: HookInterface::interface_id(),
            schema: HookInterface::schema(),
            priority: 100,
            required_authority: authority.clone(),
        }],
        maximum_authority: authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ComponentGraphError, PermissionId, ResolvedComponentGraph};
    use phenix_plugin_context::{context_component_manifest, context_manifest};
    use phenix_plugin_execution::{execution_component_manifest, execution_manifest};

    fn authority() -> Authority {
        hook_manifest(Authority::default()).maximum_authority
    }

    #[test]
    fn hooks_fail_composition_when_context_or_execution_import_is_missing() {
        let execution = execution_manifest(authority());
        let context = context_manifest();
        let hooks = hook_manifest(authority());

        let missing_context = ResolvedComponentGraph::compile(
            [execution.clone(), context.clone(), hooks.clone()],
            [
                execution_component_manifest(authority()),
                hook_component_manifest(authority()),
            ],
            &authority(),
        )
        .unwrap_err();
        assert!(matches!(
            missing_context,
            ComponentGraphError::MissingRequiredImport { component, interface }
                if component == hook_component_id()
                    && interface == ContextInterface::interface_id()
        ));

        let missing_execution = ResolvedComponentGraph::compile(
            [execution, context, hooks],
            [
                context_component_manifest(),
                hook_component_manifest(authority()),
            ],
            &authority(),
        )
        .unwrap_err();
        assert!(matches!(
            missing_execution,
            ComponentGraphError::MissingRequiredImport { component, interface }
                if component == context_component_manifest().id
                    && interface == ExecutionInterface::interface_id()
        ));
    }

    #[test]
    fn hook_imports_bind_to_context_and_execution_components() {
        let graph = ResolvedComponentGraph::compile(
            [
                execution_manifest(authority()),
                context_manifest(),
                hook_manifest(authority()),
            ],
            [
                execution_component_manifest(authority()),
                context_component_manifest(),
                hook_component_manifest(authority()),
            ],
            &authority(),
        )
        .unwrap();

        let context = graph
            .import_handle(&hook_component_id(), &ContextInterface::interface_id())
            .unwrap()
            .unwrap();
        assert_eq!(context.exporter(), &context_component_manifest().id);
        let execution = graph
            .import_handle(&hook_component_id(), &ExecutionInterface::interface_id())
            .unwrap()
            .unwrap();
        assert_eq!(
            execution.exporter(),
            &execution_component_manifest(authority()).id
        );
        let persistence_read = PermissionId::parse("kernel.persistence.read").unwrap();
        assert!(context.effective_authority().permits(&persistence_read));
        assert!(execution.effective_authority().permits(&persistence_read));
    }
}
