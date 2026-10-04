use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, Kernel, PhenixValue, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, ResolvedComposition, ResolvedCompositionActivation, ServiceId,
};

struct Demo;
impl ComponentInterface for Demo {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.component-endpoint@1").unwrap()
    }
}

struct Provider;
impl PluginInstance for Provider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        component: &ComponentId,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&PhenixValue::String(component.as_str().to_owned()))
            .map_err(|error| error.to_string())
    }
}

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn component(value: &str) -> ComponentId {
    ComponentId::parse(value).unwrap()
}

#[test]
fn typed_import_dispatches_the_exact_resolved_component_endpoint() {
    let provider = PluginManifest {
        id: plugin("provider"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let consumer = PluginManifest {
        id: plugin("consumer"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let resolved = ResolvedComposition::resolve(
        [provider.clone(), consumer.clone()],
        [
            ComponentManifest {
                listeners: Vec::new(),
                id: component("provider-low"),
                owner: provider.id.clone(),
                imports: Vec::new(),
                exports: vec![ComponentExport {
                    interface: Demo::interface_id(),
                    schema: Demo::schema(),
                    priority: 1,
                    required_authority: Authority::default(),
                }],
                maximum_authority: Authority::default(),
            },
            ComponentManifest {
                listeners: Vec::new(),
                id: component("provider-high"),
                owner: provider.id.clone(),
                imports: Vec::new(),
                exports: vec![ComponentExport {
                    interface: Demo::interface_id(),
                    schema: Demo::schema(),
                    priority: 100,
                    required_authority: Authority::default(),
                }],
                maximum_authority: Authority::default(),
            },
            ComponentManifest {
                listeners: Vec::new(),
                id: component("consumer"),
                owner: consumer.id.clone(),
                imports: vec![ComponentImport {
                    interface: Demo::interface_id(),
                    schema: Demo::schema(),
                    required: true,
                    authority: Authority::default(),
                }],
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            },
        ],
        [],
        &Authority::default(),
    )
    .unwrap();
    let handle = resolved
        .component_graph()
        .import_handle(&component("consumer"), &Demo::interface_id())
        .unwrap()
        .unwrap()
        .clone();
    assert_eq!(handle.exporter(), &component("provider-high"));

    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(provider.id, || Box::new(Provider))
        .unwrap();
    kernel
        .register_embedded_factory(consumer.id, || Box::new(Provider))
        .unwrap();
    kernel.activate_all().unwrap();

    let output = kernel
        .invoke_component(
            handle.exporter(),
            &ServiceId::parse(Demo::interface_id().as_str().to_owned()).unwrap(),
            &serde_json::to_vec(&PhenixValue::Unit).unwrap(),
            handle.effective_authority(),
            handle.owning_plugin(),
        )
        .unwrap();
    let response: PhenixValue = serde_json::from_slice(&output).unwrap();
    assert_eq!(response, PhenixValue::String("provider-high".into()));
}
