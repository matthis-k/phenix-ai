use crate::{session_tree_manifest, SessionTreeInterface};
use phenix_core::{
    ComponentExport, ComponentId, ComponentImport, ComponentInterface, ComponentManifest, PluginId,
};
use phenix_sdk::{SessionInterface, SessionMutationInterface};

const SESSION_TREE_COMPONENT: &str = "phenix.session-tree";
const SESSION_TREE_PLUGIN: &str = "phenix.session-tree";

#[must_use]
pub fn session_tree_component_id() -> ComponentId {
    ComponentId::parse(SESSION_TREE_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn session_tree_component_manifest() -> ComponentManifest {
    let authority = session_tree_manifest().maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: session_tree_component_id(),
        owner: PluginId::parse(SESSION_TREE_PLUGIN).expect("static plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: SessionInterface::interface_id(),
                schema: SessionInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: SessionMutationInterface::interface_id(),
                schema: SessionMutationInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
        ],
        exports: vec![ComponentExport {
            interface: SessionTreeInterface::interface_id(),
            schema: SessionTreeInterface::schema(),
            priority: 100,
            required_authority: authority.clone(),
        }],
        maximum_authority: authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_tree_manifest;
    use phenix_core::{ComponentGraphError, ResolvedComponentGraph};
    use phenix_plugin_sessions::{session_component_manifest, session_manifest};

    fn authority() -> phenix_core::Authority {
        phenix_core::Authority::new(
            session_manifest()
                .maximum_authority
                .permissions()
                .cloned()
                .chain(
                    session_tree_manifest()
                        .maximum_authority
                        .permissions()
                        .cloned(),
                ),
        )
    }

    #[test]
    fn required_flat_session_import_fails_before_session_tree_activation() {
        let error = ResolvedComponentGraph::compile(
            [session_manifest(), session_tree_manifest()],
            [session_tree_component_manifest()],
            &authority(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ComponentGraphError::MissingRequiredImport { component, interface }
                if component == session_tree_component_id()
                    && interface == SessionInterface::interface_id()
        ));
    }

    #[test]
    fn session_tree_import_binds_to_the_flat_session_component() {
        let graph = ResolvedComponentGraph::compile(
            [session_manifest(), session_tree_manifest()],
            [
                session_component_manifest(),
                session_tree_component_manifest(),
            ],
            &authority(),
        )
        .unwrap();
        let handle = graph
            .import_handle(
                &session_tree_component_id(),
                &SessionInterface::interface_id(),
            )
            .unwrap()
            .unwrap();

        assert_eq!(handle.exporter(), &session_component_manifest().id);
        assert_eq!(handle.owning_plugin(), &session_manifest().id);
        let tree = graph.component(&session_tree_component_id()).unwrap();
        assert!(tree
            .imports
            .iter()
            .any(|import| import.interface == SessionInterface::interface_id()));
    }
}
