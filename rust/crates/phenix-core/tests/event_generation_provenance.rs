use phenix_core::{
    Authority, EventBus, EventDeliveryStatus, EventEnvelope, EventFailurePolicy,
    EventHandler, EventSubscription,
    EventTypeId, GenerationId, Kernel, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResolvedGeneration, ResolvedGenerationActivation, ServiceContribution, ServiceId,
    ServiceRole, SubscriptionId, SubscriptionSpec,
};
use std::sync::{Arc, Mutex};

const EVENT: &str = "fixture.generation.observed";
const SERVICE: &str = "fixture.generation.emit@1";

struct Emitter;

impl PluginInstance for Emitter {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        _input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service.as_str() != SERVICE {
            return Err(format!("unsupported service: {service}"));
        }
        let receipt = host
            .dispatch_event(
                EventTypeId::parse(EVENT).unwrap(),
                1,
                0,
                0,
                Vec::new(),
            )
            .map_err(|error| error.to_string())?;
        let EventDeliveryStatus::Succeeded(report) = receipt.wait() else {
            return Err("event delivery did not succeed".into());
        };
        Ok(report
            .graph_generation
            .map(|generation| generation.as_str().as_bytes().to_vec())
            .unwrap_or_default())
    }
}

struct GenerationProbe {
    observed: Arc<Mutex<Vec<GenerationId>>>,
}

impl EventHandler for GenerationProbe {
    fn handle(&self, _event: &EventEnvelope, _authority: &Authority) -> Result<(), String> {
        Ok(())
    }

    fn handle_with_provenance(
        &self,
        _bus: &EventBus,
        _event: &EventEnvelope,
        _authority: &Authority,
        graph_generation: Option<&GenerationId>,
    ) -> Result<(), String> {
        self.observed
            .lock()
            .unwrap()
            .push(graph_generation.expect("host delivery is generation-pinned").clone());
        Ok(())
    }
}

#[test]
fn plugin_host_event_delivery_is_pinned_to_the_active_graph_generation() {
    let plugin = PluginId::parse("fixture.generation").unwrap();
    let service = ServiceId::parse(SERVICE).unwrap();
    let manifest = PluginManifest {
        id: plugin.clone(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            service: service.clone(),
            role: ServiceRole::Terminal,
            priority: 0,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    };
    let resolved =
        ResolvedGeneration::resolve([manifest], [], [], &Authority::default()).unwrap();
    let generation = resolved.generation().clone();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel
        .register_embedded_factory(plugin.clone(), || Box::new(Emitter))
        .unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    let observed = Arc::new(Mutex::new(Vec::new()));
    kernel
        .events()
        .install_subscriptions([EventSubscription {
            spec: SubscriptionSpec {
                id: SubscriptionId::parse("fixture.generation/probe").unwrap(),
                owner: PluginId::parse("fixture.generation.probe").unwrap(),
                event_type: EventTypeId::parse(EVENT).unwrap(),
                event_version: 1,
                dependencies: Vec::new(),
                failure_policy: EventFailurePolicy::FailDelivery,
                required_authority: Authority::default(),
                maximum_authority: Authority::default(),
                kernel_policy_revision: 0,
            },
            handler: Arc::new(GenerationProbe {
                observed: Arc::clone(&observed),
            }),
        }])
        .unwrap();

    let output = kernel
        .invoke(&service, &[], &Authority::default(), None)
        .unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), generation.as_str());
    assert_eq!(&*observed.lock().unwrap(), &[generation]);
}
