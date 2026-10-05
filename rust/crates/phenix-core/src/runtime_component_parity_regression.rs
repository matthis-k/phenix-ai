use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, InterfaceSchema, InvocationOutcome, Kernel, PhenixValue,
    PluginArtifact, PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest,
    PluginRuntimeAdapter, PluginRuntimeCandidate, PluginRuntimeId, ResolvedGeneration,
    ResolvedGenerationActivation, ResolvedImportHandle, ServiceContribution, ServiceId,
    ServiceRole, plugin_runtime_adapter_service,
};
use std::collections::BTreeMap;

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn component(value: &str) -> ComponentId {
    ComponentId::parse(value).unwrap()
}

struct EchoInterface;

impl ComponentInterface for EchoInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.echo@1").unwrap()
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::fallible_of::<String, String, String>()
    }
}

struct RelayInterface;

impl ComponentInterface for RelayInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.relay@1").unwrap()
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::fallible_of::<String, String, String>()
    }
}

struct Noop;

impl PluginInstance for Noop {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }
}

struct RelayConsumer;

impl PluginInstance for RelayConsumer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        target: &ComponentId,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if target != &component("fixture.consumer") {
            return Err(format!("unexpected relay target: {target}"));
        }
        let request: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let response = host
            .invoke_import::<EchoInterface>(&component("fixture.consumer"), &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_vec(&response).map_err(|error| error.to_string())
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
        let PhenixValue::String(value) = request else {
            return Err("expected string request".into());
        };
        let response = if value == "domain-error" {
            InvocationOutcome::domain_error(PhenixValue::String(format!("{}:conflict", self.0)))
                .into_transport_value()
        } else {
            PhenixValue::String(format!("{}:{value}", self.0))
        };
        serde_json::to_vec(&response).map_err(|error| error.to_string())
    }
}

struct EchoRuntimeBridge;

impl PluginRuntimeAdapter for EchoRuntimeBridge {
    fn prepare(
        &mut self,
        _candidate: PluginRuntimeCandidate<'_>,
    ) -> Result<Box<dyn PluginInstance>, String> {
        Ok(Box::new(EchoProvider("runtime")))
    }
}

impl PluginInstance for EchoRuntimeBridge {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn plugin_runtime_adapter(&mut self) -> Option<&mut dyn PluginRuntimeAdapter> {
        Some(self)
    }
}

fn consumer_manifest() -> PluginManifest {
    PluginManifest {
        id: plugin("fixture.consumer"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn embedded_provider_manifest() -> PluginManifest {
    PluginManifest {
        id: plugin("fixture.provider"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn runtime_bridge_manifest(runtime: &PluginRuntimeId) -> PluginManifest {
    PluginManifest {
        id: plugin("fixture.runtime-bridge"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: plugin_runtime_adapter_service(runtime),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn plugin_runtime_adapter_manifest(runtime: PluginRuntimeId) -> PluginManifest {
    PluginManifest {
        id: plugin("fixture.provider"),
        version: 1,
        execution: PluginExecution::Runtime {
            runtime,
            artifact: PluginArtifact {
                locator: "fixture.echo".into(),
                revision: crate::ArtifactRevision::from_content(b"echo-v1"),
                configuration: BTreeMap::new(),
            },
        },
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn components() -> [ComponentManifest; 2] {
    [
        ComponentManifest {
            listeners: Vec::new(),
            id: component("fixture.consumer"),
            owner: plugin("fixture.consumer"),
            imports: vec![ComponentImport {
                interface: EchoInterface::interface_id(),
                schema: EchoInterface::schema(),
                required: true,
                authority: Authority::default(),
            }],
            exports: vec![ComponentExport {
                interface: RelayInterface::interface_id(),
                schema: RelayInterface::schema(),
                priority: 0,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        },
        ComponentManifest {
            listeners: Vec::new(),
            id: component("fixture.provider"),
            owner: plugin("fixture.provider"),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: EchoInterface::interface_id(),
                schema: EchoInterface::schema(),
                priority: 0,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        },
    ]
}

fn handle(resolved: &ResolvedGeneration) -> ResolvedImportHandle {
    resolved
        .component_graph()
        .import_handle(
            &component("fixture.consumer"),
            &EchoInterface::interface_id(),
        )
        .unwrap()
        .unwrap()
        .clone()
}

fn invoke_request(
    handle: &ResolvedImportHandle,
    kernel: &mut Kernel,
    request: &str,
) -> PhenixValue {
    handle
        .invoke_value::<EchoInterface>(kernel, &PhenixValue::String(request.into()))
        .unwrap()
}

fn invoke(handle: &ResolvedImportHandle, kernel: &mut Kernel) -> PhenixValue {
    invoke_request(handle, kernel, "hello")
}

#[test]
fn typed_component_consumer_is_runtime_agnostic() {
    let consumer = consumer_manifest();
    let embedded_provider = embedded_provider_manifest();
    let embedded_resolved = ResolvedGeneration::resolve(
        [consumer.clone(), embedded_provider.clone()],
        components(),
        [],
        &Authority::default(),
    )
    .unwrap();
    let embedded_handle = handle(&embedded_resolved);
    let mut embedded_kernel = Kernel::new(embedded_resolved.kernel_config().clone());
    embedded_kernel
        .activate_resolved_generation(&embedded_resolved)
        .unwrap();
    embedded_kernel
        .register_embedded_factory(consumer.id.clone(), || Box::new(Noop))
        .unwrap();
    embedded_kernel
        .register_embedded_factory(embedded_provider.id, || Box::new(EchoProvider("embedded")))
        .unwrap();
    embedded_kernel.activate_all().unwrap();

    let runtime = PluginRuntimeId::parse("fixture.runtime").unwrap();
    let bridge = runtime_bridge_manifest(&runtime);
    let plugin_runtime_adapter = plugin_runtime_adapter_manifest(runtime);
    let runtime_resolved = ResolvedGeneration::resolve(
        [consumer.clone(), bridge.clone(), plugin_runtime_adapter],
        components(),
        [],
        &Authority::default(),
    )
    .unwrap();
    let runtime_handle = handle(&runtime_resolved);
    let mut runtime_kernel = Kernel::new(runtime_resolved.kernel_config().clone());
    runtime_kernel
        .activate_resolved_generation(&runtime_resolved)
        .unwrap();
    runtime_kernel
        .register_embedded_factory(consumer.id, || Box::new(Noop))
        .unwrap();
    runtime_kernel
        .register_embedded_factory(bridge.id, || Box::new(EchoRuntimeBridge))
        .unwrap();
    runtime_kernel.activate_all().unwrap();

    assert_eq!(
        invoke(&embedded_handle, &mut embedded_kernel),
        PhenixValue::String("embedded:hello".into())
    );
    assert_eq!(
        invoke(&runtime_handle, &mut runtime_kernel),
        PhenixValue::String("runtime:hello".into())
    );
}

#[test]
fn structural_domain_error_survives_nested_import() {
    let consumer = consumer_manifest();
    let provider = embedded_provider_manifest();
    let resolved = ResolvedGeneration::resolve(
        [consumer.clone(), provider.clone()],
        components(),
        [],
        &Authority::default(),
    )
    .unwrap();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(consumer.id, || Box::new(RelayConsumer))
        .unwrap();
    kernel
        .register_embedded_factory(provider.id, || Box::new(EchoProvider("embedded")))
        .unwrap();
    kernel.activate_all().unwrap();

    let input = serde_json::to_vec(&PhenixValue::String("domain-error".into())).unwrap();
    let output = kernel
        .invoke_component(
            &component("fixture.consumer"),
            &ServiceId::parse(RelayInterface::interface_id().as_str().to_owned()).unwrap(),
            &input,
            &Authority::default(),
            &plugin("fixture.consumer"),
        )
        .unwrap();
    let value: PhenixValue = serde_json::from_slice(&output).unwrap();

    assert_eq!(
        InvocationOutcome::from_transport_value(value),
        InvocationOutcome::DomainError(PhenixValue::String("embedded:conflict".into()))
    );
}

#[test]
fn structural_domain_error_survives_runtime_bridge() {
    let consumer = consumer_manifest();
    let runtime = PluginRuntimeId::parse("fixture.runtime").unwrap();
    let bridge = runtime_bridge_manifest(&runtime);
    let plugin_runtime_adapter = plugin_runtime_adapter_manifest(runtime);
    let resolved = ResolvedGeneration::resolve(
        [consumer.clone(), bridge.clone(), plugin_runtime_adapter],
        components(),
        [],
        &Authority::default(),
    )
    .unwrap();
    let handle = handle(&resolved);
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(consumer.id, || Box::new(Noop))
        .unwrap();
    kernel
        .register_embedded_factory(bridge.id, || Box::new(EchoRuntimeBridge))
        .unwrap();
    kernel.activate_all().unwrap();

    let outcome = InvocationOutcome::from_transport_value(invoke_request(
        &handle,
        &mut kernel,
        "domain-error",
    ));
    assert_eq!(
        outcome,
        InvocationOutcome::DomainError(PhenixValue::String("runtime:conflict".into()))
    );
}
