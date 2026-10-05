use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, InterfaceId, Kernel, PhenixValue, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, ProviderCompositionPolicy, ProviderFallbackReason,
    ProviderSelectionReason, ResolvedGeneration, ResolvedGenerationActivation, ServiceId,
};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

struct Echo;

impl ComponentInterface for Echo {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.provider-fallback.echo@1").unwrap()
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
            interface: InterfaceId::parse("fixture.provider-fallback.consumer@1").unwrap(),
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
    label: &'static str,
    fail: Arc<AtomicBool>,
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
        if self.fail.load(Ordering::Acquire) {
            return Err(format!("{} failed", self.label));
        }
        let request: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let PhenixValue::String(value) = request else {
            return Err("expected string".into());
        };
        serde_json::to_vec(&PhenixValue::String(format!("{}:{value}", self.label)))
            .map_err(|error| error.to_string())
    }
}

#[test]
fn fallback_is_generation_pinned_and_execution_failure_never_switches_provider() {
    let primary_id = component("primary-component");
    let fallback_id = component("fallback-component");
    let interface = Echo::interface_id();
    let policy = ProviderCompositionPolicy::new()
        .with_explicit_binding(interface.clone(), primary_id.clone())
        .with_interface_fallback(interface.clone())
        .with_fallback_enabled(interface.clone());
    let resolved = ResolvedGeneration::resolve_with_provider_policy(
        [
            manifest("consumer"),
            manifest("primary"),
            manifest("fallback"),
        ],
        [
            consumer_component(),
            provider_component("primary-component", "primary"),
            provider_component("fallback-component", "fallback"),
        ],
        [],
        policy,
        &Authority::default(),
    )
    .unwrap();
    let generation = resolved.generation().clone();
    let plan = resolved
        .component_graph()
        .provider_plan(&component("consumer-component"), &interface)
        .unwrap()
        .unwrap();
    assert_eq!(plan.primary().exporter(), &primary_id);
    assert_eq!(plan.fallbacks().len(), 1);
    assert_eq!(plan.fallbacks()[0].exporter(), &fallback_id);
    assert_eq!(
        plan.selection_reason(),
        ProviderSelectionReason::ExplicitBinding
    );

    let primary_fail = Arc::new(AtomicBool::new(false));
    let primary_calls = Arc::new(AtomicUsize::new(0));
    let fallback_calls = Arc::new(AtomicUsize::new(0));
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(plugin("consumer"), || Box::new(Consumer))
        .unwrap();
    let primary_fail_for_factory = Arc::clone(&primary_fail);
    let primary_calls_for_factory = Arc::clone(&primary_calls);
    kernel
        .register_embedded_factory(plugin("primary"), move || {
            Box::new(Provider {
                label: "primary",
                fail: Arc::clone(&primary_fail_for_factory),
                calls: Arc::clone(&primary_calls_for_factory),
            })
        })
        .unwrap();
    let fallback_calls_for_factory = Arc::clone(&fallback_calls);
    kernel
        .register_embedded_factory(plugin("fallback"), move || {
            Box::new(Provider {
                label: "fallback",
                fail: Arc::new(AtomicBool::new(false)),
                calls: Arc::clone(&fallback_calls_for_factory),
            })
        })
        .unwrap();
    kernel.activate_all().unwrap();

    kernel.stop(&plugin("primary")).unwrap();
    let output = kernel
        .invoke_component(
            &component("consumer-component"),
            &ServiceId::parse("fixture.provider-fallback.consumer@1").unwrap(),
            &serde_json::to_vec(&PhenixValue::String("hello".into())).unwrap(),
            &Authority::default(),
            &plugin("consumer"),
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    assert_eq!(output, PhenixValue::String("fallback:hello".into()));
    assert_eq!(primary_calls.load(Ordering::Acquire), 0);
    assert_eq!(fallback_calls.load(Ordering::Acquire), 1);

    let fallback_entry = kernel
        .service_invocation_provenance()
        .into_iter()
        .find(|entry| entry.component_provider.is_some())
        .unwrap();
    assert_eq!(fallback_entry.graph_generation, Some(generation));
    let fallback_provenance = fallback_entry.component_provider.unwrap();
    assert_eq!(fallback_provenance.primary.component, primary_id);
    assert_eq!(fallback_provenance.fallbacks.len(), 1);
    assert_eq!(fallback_provenance.fallbacks[0].component, fallback_id);
    assert_eq!(
        fallback_provenance.selection_reason,
        ProviderSelectionReason::ExplicitBinding
    );
    assert_eq!(
        fallback_provenance.executed_provider.plugin,
        plugin("fallback")
    );
    assert_eq!(
        fallback_provenance.fallback_reason,
        Some(ProviderFallbackReason::PrimaryUnavailable)
    );

    kernel.activate_all().unwrap();
    primary_fail.store(true, Ordering::Release);
    let fallback_calls_before_failure = fallback_calls.load(Ordering::Acquire);
    assert!(kernel
        .invoke_component(
            &component("consumer-component"),
            &ServiceId::parse("fixture.provider-fallback.consumer@1").unwrap(),
            &serde_json::to_vec(&PhenixValue::String("fail".into())).unwrap(),
            &Authority::default(),
            &plugin("consumer"),
        )
        .is_err());
    assert_eq!(primary_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        fallback_calls.load(Ordering::Acquire),
        fallback_calls_before_failure
    );

    let provider_entries = kernel
        .service_invocation_provenance()
        .into_iter()
        .filter_map(|entry| entry.component_provider)
        .collect::<Vec<_>>();
    let failed_primary = provider_entries.last().unwrap();
    assert_eq!(failed_primary.executed_provider.plugin, plugin("primary"));
    assert_eq!(failed_primary.fallback_reason, None);
}
