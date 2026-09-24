use crate::{
    ComponentInterface, InterfaceId, Kernel, KernelError, PhenixValue, ResolvedImportHandle,
    ServiceId,
};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Debug)]
pub enum ComponentInvocationError {
    InterfaceMismatch {
        handle: InterfaceId,
        requested: InterfaceId,
    },
    UnboundImport {
        component: crate::ComponentId,
        interface: InterfaceId,
    },
    Graph(crate::ComponentGraphError),
    InvalidInterface {
        interface: InterfaceId,
        message: String,
    },
    Encode(String),
    Kernel(KernelError),
    Decode(String),
}

impl Display for ComponentInvocationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InterfaceMismatch { handle, requested } => write!(
                f,
                "component import handle is for {handle}, not requested interface {requested}"
            ),
            Self::UnboundImport {
                component,
                interface,
            } => write!(
                f,
                "component {component} has no bound provider for optional import {interface}"
            ),
            Self::Graph(error) => Display::fmt(error, f),
            Self::InvalidInterface { interface, message } => {
                write!(
                    f,
                    "component interface {interface} is not invokable: {message}"
                )
            }
            Self::Encode(message) => write!(f, "component request encoding failed: {message}"),
            Self::Kernel(error) => Display::fmt(error, f),
            Self::Decode(message) => write!(f, "component response decoding failed: {message}"),
        }
    }
}

impl Error for ComponentInvocationError {}

impl From<KernelError> for ComponentInvocationError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

impl From<crate::ComponentGraphError> for ComponentInvocationError {
    fn from(error: crate::ComponentGraphError) -> Self {
        Self::Graph(error)
    }
}

impl ResolvedImportHandle {
    pub fn invoke_value<I: ComponentInterface>(
        &self,
        kernel: &mut Kernel,
        request: &PhenixValue,
    ) -> Result<PhenixValue, ComponentInvocationError> {
        let requested = I::interface_id();
        if self.interface() != &requested {
            return Err(ComponentInvocationError::InterfaceMismatch {
                handle: self.interface().clone(),
                requested,
            });
        }

        let service =
            ServiceId::parse(self.interface().as_str().to_owned()).map_err(|message| {
                ComponentInvocationError::InvalidInterface {
                    interface: self.interface().clone(),
                    message: message.into(),
                }
            })?;
        let input = serde_json::to_vec(request)
            .map_err(|error| ComponentInvocationError::Encode(error.to_string()))?;
        let output = kernel.invoke_component(
            self.exporter(),
            &service,
            &input,
            self.effective_authority(),
            self.owning_plugin(),
        )?;
        serde_json::from_slice(&output)
            .map_err(|error| ComponentInvocationError::Decode(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, CapabilityId, ComponentExport, ComponentId, ComponentImport, ComponentManifest,
        KernelConfig, Key, PhenixValue, PluginExecution, PluginHost, PluginId, PluginInstance,
        PluginManifest, ResolvedComponentGraph, ResolvedHarness, ResolvedHarnessActivation,
        ServiceContribution, ServiceRole,
    };
    use std::collections::BTreeMap;

    fn key(value: &str) -> Key {
        Key::parse(value.to_owned()).unwrap()
    }

    fn echo_request(value: &str) -> PhenixValue {
        PhenixValue::Table(BTreeMap::from([(
            key("value"),
            PhenixValue::String(value.to_owned()),
        )]))
    }

    fn echo_response(provider: &str, value: &str) -> PhenixValue {
        PhenixValue::Table(BTreeMap::from([
            (key("provider"), PhenixValue::String(provider.to_owned())),
            (key("value"), PhenixValue::String(value.to_owned())),
        ]))
    }

    struct EchoInterface;

    impl ComponentInterface for EchoInterface {
        fn interface_id() -> InterfaceId {
            InterfaceId::parse("fixture.echo@1").unwrap()
        }
    }

    struct OtherInterface;

    impl ComponentInterface for OtherInterface {
        fn interface_id() -> InterfaceId {
            InterfaceId::parse("fixture.other@1").unwrap()
        }
    }

    struct EchoProvider(&'static str);

    impl PluginInstance for EchoProvider {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            let request: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let value = match request.get("value").map_err(|error| error.to_string())? {
                PhenixValue::String(value) => value.clone(),
                other => return Err(format!("expected string value, got {:?}", other.kind())),
            };
            serde_json::to_vec(&echo_response(self.0, &value)).map_err(|error| error.to_string())
        }
    }

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn component(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn plugin_manifest(id: &str, service_priority: i32, authority: Authority) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                service: ServiceId::parse("fixture.echo@1").unwrap(),
                role: ServiceRole::Terminal,
                priority: service_priority,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn provider_component(
        id: &str,
        owner: &str,
        priority: i32,
        authority: Authority,
    ) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component(id),
            owner: plugin(owner),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: EchoInterface::interface_id(),
                schema: EchoInterface::schema(),
                priority,
                required_authority: Authority::default(),
            }],
            maximum_authority: authority,
        }
    }

    #[test]
    fn typed_import_handle_uses_graph_binding_instead_of_global_provider_selection() {
        let read = capability("fixture.read");
        let authority = Authority::new([read]);
        let consumer = PluginManifest {
            id: plugin("consumer-owner"),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: authority.clone(),
        };
        let graph_selected = plugin_manifest("graph-selected", 10, authority.clone());
        let registry_preferred = plugin_manifest("registry-preferred", 1000, authority.clone());
        let manifests = vec![
            consumer.clone(),
            graph_selected.clone(),
            registry_preferred.clone(),
        ];
        let components = vec![
            ComponentManifest {
                listeners: Vec::new(),
                id: component("consumer"),
                owner: consumer.id,
                imports: vec![ComponentImport {
                    interface: EchoInterface::interface_id(),
                    schema: EchoInterface::schema(),
                    required: true,
                    authority: authority.clone(),
                }],
                exports: Vec::new(),
                maximum_authority: authority.clone(),
            },
            provider_component("graph-provider", "graph-selected", 100, authority.clone()),
            provider_component(
                "registry-provider",
                "registry-preferred",
                1,
                authority.clone(),
            ),
        ];
        let resolved =
            ResolvedHarness::resolve(manifests.clone(), components, [], &authority).unwrap();
        let handle = resolved
            .component_graph()
            .import_handle(&component("consumer"), &EchoInterface::interface_id())
            .unwrap()
            .unwrap()
            .clone();
        assert_eq!(handle.owning_plugin(), &plugin("graph-selected"));

        let mut kernel = Kernel::new(resolved.kernel_config().clone());
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(plugin("consumer-owner"), || {
                Box::new(EchoProvider("consumer"))
            })
            .unwrap();
        kernel
            .register_embedded_factory(plugin("graph-selected"), || Box::new(EchoProvider("graph")))
            .unwrap();
        kernel
            .register_embedded_factory(plugin("registry-preferred"), || {
                Box::new(EchoProvider("registry"))
            })
            .unwrap();
        kernel.activate_all().unwrap();

        let response = handle
            .invoke_value::<EchoInterface>(&mut kernel, &echo_request("hello"))
            .unwrap();
        assert_eq!(response, echo_response("graph", "hello"));
    }

    #[test]
    fn typed_import_handle_rejects_a_different_interface_before_dispatch() {
        let owner = plugin_manifest("provider", 1, Authority::default());
        let consumer = PluginManifest {
            id: plugin("consumer-owner"),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let graph = ResolvedComponentGraph::compile(
            [consumer.clone(), owner.clone()],
            [
                ComponentManifest {
                    listeners: Vec::new(),
                    id: component("consumer"),
                    owner: consumer.id.clone(),
                    imports: vec![ComponentImport {
                        interface: EchoInterface::interface_id(),
                        schema: EchoInterface::schema(),
                        required: true,
                        authority: Authority::default(),
                    }],
                    exports: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                provider_component("provider-component", "provider", 1, Authority::default()),
            ],
            &Authority::default(),
        )
        .unwrap();
        let handle = graph
            .import_handle(&component("consumer"), &EchoInterface::interface_id())
            .unwrap()
            .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([consumer, owner]).unwrap());

        assert!(matches!(
            handle.invoke_value::<OtherInterface>(&mut kernel, &echo_request("never dispatched")),
            Err(ComponentInvocationError::InterfaceMismatch { .. })
        ));
    }
}
