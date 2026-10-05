use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, Kernel, KernelError, PhenixValue, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, ProviderCompositionPolicy, ResolvedGeneration,
    ResolvedGenerationActivation, ServiceId,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Echo;

impl ComponentInterface for Echo {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.provider-availability.echo@1").unwrap()
    }
}

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn component(value: &str) -> ComponentId {
    ComponentId::parse(value).unwrap()
}

fn manifest(value: &str) -> PluginManifest {
    PluginManifest {
        id: plugin(value),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn provider_component(value: &str, owner: &str) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: component(value),
        owner: plugin(owner),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: Echo::interface_id(),
            schema: Echo::schema(),
            priority: 0,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn consumer_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: component("consumer-component"),
        owner: plugin("consumer"),
        imports: vec![ComponentImport {
            interface: Echo::interface_id(),
            schema: Echo::schema(),
            required: true,
            authority: Authority::default(),
        }],
        exports: vec![ComponentExport {
            interface: InterfaceId::parse("fixture.provider-availability.consumer@1").unwrap(),
            schema: Default::default(),
            priority: 0,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

struct Consumer;

impl PluginInstance for Consumer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        _component: &ComponentId,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let request: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let response = host
            .invoke_import::<Echo>(&component("consumer-component"), &request)
            .map_err(|error| error.to_string())?;
        serde_json::to_vec(&response).map_err(|error| error.to_string())
    }
}

struct Provider {
    calls: Arc<AtomicUsize>,
}

impl PluginInstance for Provider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_component(
        &mut self,
        _component: &ComponentId,
        _service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Ok(input.to_vec())
    }
}

#[test]
fn unavailable_primary_does_not_search_unplanned_providers() {
    let interface = Echo::interface_id();
    let primary = component("primary-component");
    let secondary = component("secondary-component");
    let policy =
        ProviderCompositionPolicy::new().with_explicit_binding(interface.clone(), primary.clone());
    let resolved = ResolvedGeneration::resolve_with_provider_policy(
        [
            manifest("consumer"),
            manifest("primary"),
            manifest("secondary"),
        ],
        [
            consumer_component(),
            provider_component("primary-component", "primary"),
            provider_component("secondary-component", "secondary"),
        ],
        [],
        policy,
        &Authority::default(),
    )
    .unwrap();
    let plan = resolved
        .component_graph()
        .provider_plan(&component("consumer-component"), &interface)
        .unwrap()
        .unwrap();
    assert_eq!(plan.primary().exporter(), &primary);
    assert!(plan.fallbacks().is_empty());

    let primary_calls = Arc::new(AtomicUsize::new(0));
    let secondary_calls = Arc::new(AtomicUsize::new(0));
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(plugin("consumer"), || Box::new(Consumer))
        .unwrap();
    let primary_calls_for_factory = Arc::clone(&primary_calls);
    kernel
        .register_embedded_factory(plugin("primary"), move || {
            Box::new(Provider {
                calls: Arc::clone(&primary_calls_for_factory),
            })
        })
        .unwrap();
    let secondary_calls_for_factory = Arc::clone(&secondary_calls);
    kernel
        .register_embedded_factory(plugin("secondary"), move || {
            Box::new(Provider {
                calls: Arc::clone(&secondary_calls_for_factory),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();
    kernel.stop(&plugin("primary")).unwrap();

    let error = kernel
        .invoke_component(
            &component("consumer-component"),
            &ServiceId::parse("fixture.provider-availability.consumer@1").unwrap(),
            &serde_json::to_vec(&PhenixValue::String("hello".into())).unwrap(),
            &Authority::default(),
            &plugin("consumer"),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        KernelError::ServiceInvoke { plugin: failed_plugin, .. } if failed_plugin == plugin("consumer")
    ));
    assert_eq!(primary_calls.load(Ordering::Acquire), 0);
    assert_eq!(secondary_calls.load(Ordering::Acquire), 0);
    assert_eq!(
        resolved
            .component_graph()
            .provider_plan(&component("consumer-component"), &interface)
            .unwrap()
            .unwrap()
            .primary()
            .exporter(),
        &primary
    );
    assert_ne!(&secondary, &primary);
}
