use super::{
    ListenerProjection, StaticComponentEvent, StaticComponentHost, StaticComponentImport,
    StaticComponentImports,
};
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentListener, ComponentManifest,
    EventFailurePolicy, EventTypeId, HasPhenixSchema, InterfaceId, InterfaceSchema, PhenixSchema,
    PluginId, ServiceContribution, ServiceId, ServiceRole, SubscriptionId,
};

pub trait InterfaceMarker {
    fn interface_id() -> InterfaceId;
}

pub trait StaticComponentDefinition {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticComponentExport {
    pub interface: InterfaceId,
    pub schema: InterfaceSchema,
    pub method: &'static str,
    pub public: bool,
    pub terminal: bool,
    pub priority: i32,
    pub required_authority: Authority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticComponentLayer {
    pub interface: InterfaceId,
    pub method: &'static str,
    pub priority: i32,
    pub required_authority: Authority,
}

impl StaticComponentLayer {
    #[must_use]
    pub fn of<I: InterfaceMarker>(method: &'static str, priority: i32) -> Self {
        Self::with_authority::<I>(method, priority, Authority::default())
    }

    #[must_use]
    pub fn with_authority<I: InterfaceMarker>(
        method: &'static str,
        priority: i32,
        required_authority: Authority,
    ) -> Self {
        Self {
            interface: I::interface_id(),
            method,
            priority,
            required_authority,
        }
    }

    #[must_use]
    pub fn service(&self) -> ServiceContribution {
        ServiceContribution {
            service: ServiceId::parse(self.interface.as_str())
                .expect("interface identity is a valid service identity"),
            role: ServiceRole::Layer,
            priority: self.priority,
            required_authority: self.required_authority.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticComponentListener {
    pub event: EventTypeId,
    pub method: &'static str,
    pub payload_type: &'static str,
    pub payload_schema: PhenixSchema,
    pub projection: ListenerProjection,
    pub required_authority: Authority,
}

impl StaticComponentListener {
    #[must_use]
    pub fn of<T: HasPhenixSchema>(event: &str, method: &'static str) -> Self {
        Self::projected::<T>(event, method)
    }

    #[must_use]
    pub fn with_authority<T: HasPhenixSchema>(
        event: &str,
        method: &'static str,
        required_authority: Authority,
    ) -> Self {
        Self::new::<T>(
            event,
            method,
            ListenerProjection::Project,
            required_authority,
        )
    }

    #[must_use]
    pub fn exact_with_authority<T: HasPhenixSchema>(
        event: &str,
        method: &'static str,
        required_authority: Authority,
    ) -> Self {
        Self::new::<T>(event, method, ListenerProjection::Exact, required_authority)
    }

    #[must_use]
    pub fn projected<T: HasPhenixSchema>(event: &str, method: &'static str) -> Self {
        Self::new::<T>(
            event,
            method,
            ListenerProjection::Project,
            Authority::default(),
        )
    }

    #[must_use]
    pub fn exact<T: HasPhenixSchema>(event: &str, method: &'static str) -> Self {
        Self::new::<T>(
            event,
            method,
            ListenerProjection::Exact,
            Authority::default(),
        )
    }

    fn new<T: HasPhenixSchema>(
        event: &str,
        method: &'static str,
        projection: ListenerProjection,
        required_authority: Authority,
    ) -> Self {
        Self {
            event: EventTypeId::parse(event)
                .expect("component attribute validated the static event type"),
            method,
            payload_type: std::any::type_name::<T>(),
            payload_schema: T::phenix_schema(),
            projection,
            required_authority,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticComponentValue {
    pub id: InterfaceId,
    pub method: &'static str,
    pub public: bool,
    pub value_type: &'static str,
    pub schema: PhenixSchema,
}

impl StaticComponentValue {
    #[must_use]
    pub fn of<T: HasPhenixSchema>(id: &str, method: &'static str, public: bool) -> Self {
        Self {
            id: InterfaceId::parse(id).expect("component attribute validated the static value id"),
            method,
            public,
            value_type: std::any::type_name::<T>(),
            schema: T::phenix_schema(),
        }
    }
}

pub trait StaticComponentBehavior {
    fn exports() -> Vec<StaticComponentExport> {
        Vec::new()
    }

    fn layers() -> Vec<StaticComponentLayer> {
        Vec::new()
    }

    fn listeners() -> Vec<StaticComponentListener> {
        Vec::new()
    }

    fn values() -> Vec<StaticComponentValue> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticComponentDescriptor {
    pub id: ComponentId,
    pub field: &'static str,
    pub component_type: &'static str,
    imports: Vec<StaticComponentImport>,
    hosts: Vec<StaticComponentHost>,
    events: Vec<StaticComponentEvent>,
    exports: Vec<StaticComponentExport>,
    layers: Vec<StaticComponentLayer>,
    listeners: Vec<StaticComponentListener>,
    values: Vec<StaticComponentValue>,
}

impl StaticComponentDescriptor {
    #[must_use]
    pub fn derived<T>(owner: &PluginId, field: &'static str) -> Self
    where
        T: StaticComponentDefinition + StaticComponentImports + StaticComponentBehavior,
    {
        let id = ComponentId::parse(format!("{}.{field}", owner.as_str()))
            .expect("plugin id and Rust field name derive a valid component id");
        Self::new::<T>(id, field)
    }

    #[must_use]
    pub fn explicit<T>(id: &str, field: &'static str) -> Self
    where
        T: StaticComponentDefinition + StaticComponentImports + StaticComponentBehavior,
    {
        let id =
            ComponentId::parse(id).expect("component attribute validated the static component id");
        Self::new::<T>(id, field)
    }

    fn new<T>(id: ComponentId, field: &'static str) -> Self
    where
        T: StaticComponentDefinition + StaticComponentImports + StaticComponentBehavior,
    {
        Self {
            id,
            field,
            component_type: std::any::type_name::<T>(),
            imports: T::imports(),
            hosts: T::hosts(),
            events: T::events(),
            exports: T::exports(),
            layers: T::layers(),
            listeners: T::listeners(),
            values: T::values(),
        }
    }

    #[must_use]
    pub fn manifest(&self, owner: &PluginId) -> ComponentManifest {
        self.manifest_with_authority(owner, &Authority::default())
    }

    #[must_use]
    pub fn manifest_with_authority(
        &self,
        owner: &PluginId,
        maximum_authority: &Authority,
    ) -> ComponentManifest {
        ComponentManifest {
            id: self.id.clone(),
            owner: owner.clone(),
            imports: self
                .imports
                .iter()
                .map(|import| ComponentImport {
                    interface: import.interface.clone(),
                    schema: import.schema.clone(),
                    required: import.required,
                    authority: import.authority.clone(),
                })
                .collect(),
            exports: self
                .exports
                .iter()
                .map(|export| ComponentExport {
                    interface: export.interface.clone(),
                    schema: export.schema.clone(),
                    priority: export.priority,
                    required_authority: export.required_authority.clone(),
                })
                .collect(),
            listeners: self
                .listeners
                .iter()
                .map(|listener| ComponentListener {
                    id: SubscriptionId::parse(format!(
                        "{}/listener/{}/{}",
                        owner.as_str(),
                        self.id.as_str(),
                        listener.method
                    ))
                    .expect("generated stateful listener subscription id is valid"),
                    event: listener.event.clone(),
                    event_version: 1,
                    method: listener.method.to_owned(),
                    payload_schema: listener.payload_schema.clone(),
                    projection: listener.projection,
                    dependencies: Vec::new(),
                    failure_policy: EventFailurePolicy::Warn,
                    required_authority: listener.required_authority.clone(),
                })
                .collect(),
            maximum_authority: maximum_authority.clone(),
        }
    }

    #[must_use]
    pub fn services(&self) -> Vec<ServiceContribution> {
        self.layers
            .iter()
            .map(StaticComponentLayer::service)
            .collect()
    }

    #[must_use]
    pub fn imports(&self) -> &[StaticComponentImport] {
        &self.imports
    }

    #[must_use]
    pub fn hosts(&self) -> &[StaticComponentHost] {
        &self.hosts
    }

    #[must_use]
    pub fn events(&self) -> &[StaticComponentEvent] {
        &self.events
    }

    #[must_use]
    pub fn exports(&self) -> &[StaticComponentExport] {
        &self.exports
    }

    #[must_use]
    pub fn layers(&self) -> &[StaticComponentLayer] {
        &self.layers
    }

    #[must_use]
    pub fn listeners(&self) -> &[StaticComponentListener] {
        &self.listeners
    }

    #[must_use]
    pub fn values(&self) -> &[StaticComponentValue] {
        &self.values
    }
}

pub trait StaticPluginComponents {
    fn components() -> Vec<StaticComponentDescriptor>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Api;
    struct Models;

    impl StaticComponentDefinition for Api {}

    impl StaticComponentImports for Api {
        fn imports() -> Vec<StaticComponentImport> {
            vec![StaticComponentImport {
                interface: Models::interface_id(),
                schema: InterfaceSchema::of::<String, String>(),
                field: "models",
                required: true,
                authority: Authority::default(),
            }]
        }

        fn hosts() -> Vec<StaticComponentHost> {
            vec![StaticComponentHost {
                interface: Models::interface_id(),
                field: "model_host",
                authority: Authority::default(),
            }]
        }

        fn events() -> Vec<StaticComponentEvent> {
            vec![StaticComponentEvent {
                event: EventTypeId::parse("fixture.emitted").unwrap(),
                field: "emitted",
                payload_type: std::any::type_name::<String>(),
                payload_schema: String::phenix_schema(),
            }]
        }
    }

    impl StaticComponentBehavior for Api {
        fn exports() -> Vec<StaticComponentExport> {
            vec![StaticComponentExport {
                interface: Models::interface_id(),
                schema: InterfaceSchema::of::<String, String>(),
                method: "run",
                public: true,
                terminal: true,
                priority: 29,
                required_authority: Authority::default(),
            }]
        }

        fn layers() -> Vec<StaticComponentLayer> {
            vec![StaticComponentLayer::of::<Models>("policy", 17)]
        }

        fn listeners() -> Vec<StaticComponentListener> {
            vec![StaticComponentListener::of::<String>(
                "fixture.completed",
                "completed",
            )]
        }

        fn values() -> Vec<StaticComponentValue> {
            vec![StaticComponentValue::of::<u64>(
                "fixture.status@1",
                "status",
                true,
            )]
        }
    }

    impl InterfaceMarker for Models {
        fn interface_id() -> InterfaceId {
            InterfaceId::parse("fixture.models@1").unwrap()
        }
    }

    #[test]
    fn component_id_derives_from_owner_and_field() {
        let owner = PluginId::parse("fixture.component-owner").unwrap();
        let component = StaticComponentDescriptor::derived::<Api>(&owner, "api");

        assert_eq!(component.id.as_str(), "fixture.component-owner.api");
        assert_eq!(component.field, "api");
        assert!(component.component_type.ends_with("::Api"));
    }

    #[test]
    fn explicit_component_id_preserves_stable_identity() {
        let component = StaticComponentDescriptor::explicit::<Api>("legacy.component", "api");

        assert_eq!(component.id.as_str(), "legacy.component");
        assert_eq!(component.field, "api");
    }

    #[test]
    fn erased_descriptor_preserves_runtime_behavior_metadata() {
        let owner = PluginId::parse("fixture.component-owner").unwrap();
        let component = StaticComponentDescriptor::derived::<Api>(&owner, "api");

        let manifest = component.manifest(&owner);
        let services = component.services();

        assert_eq!(manifest.id, component.id);
        assert_eq!(manifest.owner, owner);
        assert_eq!(manifest.imports.len(), 1);
        assert_eq!(manifest.exports.len(), 1);
        assert_eq!(manifest.listeners.len(), 1);
        assert_eq!(
            manifest.listeners[0].id.as_str(),
            "fixture.component-owner/listener/fixture.component-owner.api/completed"
        );
        assert_eq!(manifest.listeners[0].event_version, 1);
        assert_eq!(
            manifest.listeners[0].projection,
            ListenerProjection::Project
        );
        assert_eq!(component.imports().len(), 1);
        assert_eq!(component.imports()[0].field, "models");
        assert_eq!(component.hosts().len(), 1);
        assert_eq!(component.hosts()[0].interface.as_str(), "fixture.models@1");
        assert_eq!(component.events().len(), 1);
        assert_eq!(component.events()[0].event.as_str(), "fixture.emitted");
        assert_eq!(component.exports().len(), 1);
        assert_eq!(component.exports()[0].method, "run");
        assert_eq!(component.layers().len(), 1);
        assert_eq!(component.layers()[0].priority, 17);
        assert_eq!(component.listeners().len(), 1);
        assert_eq!(component.listeners()[0].event.as_str(), "fixture.completed");
        assert_eq!(component.values().len(), 1);
        assert_eq!(component.values()[0].id.as_str(), "fixture.status@1");
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].role, ServiceRole::Layer);
    }

    #[test]
    fn behavior_descriptors_preserve_semantic_identity_and_priority() {
        let export = StaticComponentExport {
            interface: Models::interface_id(),
            schema: InterfaceSchema::of::<String, String>(),
            method: "run",
            public: true,
            terminal: true,
            priority: 29,
            required_authority: Authority::default(),
        };
        let layer = StaticComponentLayer::of::<Models>("policy", 17);
        let listener = StaticComponentListener::of::<String>("fixture.completed", "completed");
        let value = StaticComponentValue::of::<u64>("fixture.status@1", "status", true);

        assert_eq!(export.interface.as_str(), "fixture.models@1");
        assert_eq!(export.priority, 29);
        assert_eq!(export.required_authority, Authority::default());
        assert_eq!(layer.interface.as_str(), "fixture.models@1");
        assert_eq!(layer.priority, 17);
        assert_eq!(layer.required_authority, Authority::default());
        assert_eq!(listener.event.as_str(), "fixture.completed");
        assert_eq!(listener.payload_type, std::any::type_name::<String>());
        assert_eq!(listener.payload_schema, String::phenix_schema());
        assert_eq!(listener.projection, ListenerProjection::Project);
        assert_eq!(listener.required_authority, Authority::default());
        assert_eq!(value.id.as_str(), "fixture.status@1");
        assert!(value.public);
        assert_eq!(value.value_type, std::any::type_name::<u64>());
        assert_eq!(value.schema, u64::phenix_schema());
    }

    #[test]
    fn layer_service_preserves_explicit_authority() {
        let authority =
            Authority::new([phenix_core::PermissionId::parse("models.invoke").unwrap()]);
        let layer = StaticComponentLayer::with_authority::<Models>("policy", 17, authority.clone());

        assert_eq!(layer.required_authority, authority);
        assert_eq!(layer.service().required_authority, authority);
    }
}
