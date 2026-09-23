use crate::{debug_manifest, DEBUG_SERVICE};
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface, ComponentListener,
    ComponentManifest, EventFailurePolicy, HasPhenixSchema, InterfaceId, InterfaceSchema,
    ListenerProjection, PhenixSchema, PhenixValue, SubscriptionId,
};
use phenix_sdk::{
    model_diagnostic_event_type, ContextInterface, FrontendInterface, JobInterface,
    ModelRoutingInterface, PlanningInterface, SessionInterface, MODEL_DIAGNOSTIC_EVENT_VERSION,
};

pub struct DebugInterface;

impl ComponentInterface for DebugInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(DEBUG_SERVICE).expect("static debug interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<crate::DebugCommand, crate::DebugResponse>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub(crate) enum SessionProbeCommand {
    List,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub(crate) enum ContextProbeCommand {
    List,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub(crate) enum PlanningProbeCommand {
    SearchHistory {
        objective_id: Option<String>,
        query: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub(crate) enum JobProbeCommand {
    List,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub(crate) enum ModelProbeCommand {
    ListProfiles,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
pub(crate) enum FrontendProbeCommand {
    Catalog,
}

#[must_use]
pub fn debug_component_id() -> ComponentId {
    crate::Plugin::component_id()
}

fn optional_import<Request: HasPhenixSchema>(
    interface: InterfaceId,
    authority: &Authority,
) -> ComponentImport {
    ComponentImport {
        interface,
        schema: InterfaceSchema::fallible_of::<Request, PhenixValue, PhenixValue>(),
        required: false,
        authority: authority.clone(),
    }
}

#[must_use]
pub fn debug_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let authority = debug_manifest(maximum_authority).maximum_authority;
    ComponentManifest {
        listeners: vec![
            ComponentListener {
                id: SubscriptionId::parse("phenix.debug/listener/model-diagnostic")
                    .expect("static model diagnostic listener id is valid"),
                event: model_diagnostic_event_type(),
                event_version: MODEL_DIAGNOSTIC_EVENT_VERSION,
                method: "model_diagnostic".into(),
                payload_schema: PhenixSchema::Any,
                projection: ListenerProjection::Exact,
                dependencies: Vec::new(),
                failure_policy: EventFailurePolicy::Warn,
                required_authority: Authority::default(),
            },
        ],
        id: debug_component_id(),
        owner: crate::Plugin::plugin_id(),
        imports: vec![
            optional_import::<SessionProbeCommand>(SessionInterface::interface_id(), &authority),
            optional_import::<ContextProbeCommand>(ContextInterface::interface_id(), &authority),
            optional_import::<PlanningProbeCommand>(PlanningInterface::interface_id(), &authority),
            optional_import::<JobProbeCommand>(JobInterface::interface_id(), &authority),
            optional_import::<ModelProbeCommand>(ModelRoutingInterface::interface_id(), &authority),
            optional_import::<FrontendProbeCommand>(FrontendInterface::interface_id(), &authority),
        ],
        exports: vec![ComponentExport {
            interface: DebugInterface::interface_id(),
            schema: DebugInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::ResolvedComponentGraph;
    use phenix_plugin_sessions::{session_component_manifest, session_manifest};

    #[test]
    fn debug_optional_imports_bind_only_available_components() {
        let authority = Authority::default();
        let graph = ResolvedComponentGraph::compile(
            [session_manifest(), debug_manifest(authority.clone())],
            [
                session_component_manifest(),
                debug_component_manifest(authority.clone()),
            ],
            &authority,
        )
        .unwrap();

        assert!(graph
            .import_handle(&debug_component_id(), &SessionInterface::interface_id())
            .unwrap()
            .is_some());
        assert!(graph
            .import_handle(&debug_component_id(), &ContextInterface::interface_id())
            .unwrap()
            .is_none());
        let manifest = debug_component_manifest(authority);
        assert_eq!(manifest.listeners.len(), 1);
        let model_listener = manifest
            .listeners
            .iter()
            .find(|listener| listener.event == model_diagnostic_event_type())
            .expect("model diagnostic listener");
        assert_eq!(model_listener.failure_policy, EventFailurePolicy::Warn);
    }
}
