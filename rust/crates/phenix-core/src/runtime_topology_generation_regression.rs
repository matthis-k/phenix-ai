use crate::{
    Authority, ComponentExport, ComponentId, ComponentListener, ComponentManifest, EventEnvelope,
    EventFailurePolicy, EventHandler, EventTypeId, GraphGenerationId, GraphReconciler, InterfaceId,
    Kernel, KernelError, ListenerProjection, LiveReconciliationError, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, ResolvedHarness, ResolvedHarnessActivation,
    ResolvedListener, ServiceContribution, ServiceId, ServiceRole, SubscriptionId,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

fn plugin(id: &str, service: Option<ServiceId>) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(id).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: service
            .map(|service| ServiceContribution {
                service,
                role: ServiceRole::Terminal,
                priority: 0,
                required_authority: Authority::default(),
            })
            .into_iter()
            .collect(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn listener_component(
    owner: &PluginId,
    name: &str,
    service: Option<ServiceId>,
) -> ComponentManifest {
    let component = ComponentId::parse(format!("{name}.component")).unwrap();
    ComponentManifest {
        id: component.clone(),
        owner: owner.clone(),
        imports: Vec::new(),
        exports: service
            .map(|service| ComponentExport {
                interface: InterfaceId::parse(service.as_str()).unwrap(),
                schema: Default::default(),
                priority: 0,
                required_authority: Authority::default(),
            })
            .into_iter()
            .collect(),
        listeners: vec![ComponentListener {
            id: SubscriptionId::parse(format!("{name}/listener/observed")).unwrap(),
            event: EventTypeId::parse("fixture.topology.changed").unwrap(),
            event_version: 1,
            method: "observed".into(),
            payload_schema: crate::PhenixSchema::Any,
            projection: ListenerProjection::Project,
            dependencies: Vec::new(),
            failure_policy: EventFailurePolicy::FailDelivery,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn event(causality_id: u64) -> EventEnvelope {
    EventEnvelope {
        event_type: EventTypeId::parse("fixture.topology.changed").unwrap(),
        version: 1,
        emitter: PluginId::parse("fixture.emitter").unwrap(),
        causality_id,
        kernel_policy_revision: 0,
        payload: Vec::new(),
    }
}

struct TrackingInstance {
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    deliveries: Arc<AtomicUsize>,
    delivered_generations: Arc<Mutex<Vec<GraphGenerationId>>>,
    fail_binding: bool,
    response: Vec<u8>,
}

impl PluginInstance for TrackingInstance {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(self.response.clone())
    }

    fn bind_listener(
        &mut self,
        _listener: &ResolvedListener,
        generation: &GraphGenerationId,
    ) -> Result<Arc<dyn EventHandler>, String> {
        if self.fail_binding {
            return Err("candidate listener rejected".into());
        }
        let deliveries = Arc::clone(&self.deliveries);
        let delivered_generations = Arc::clone(&self.delivered_generations);
        let generation = generation.clone();
        Ok(Arc::new(
            move |_: &EventEnvelope, _: &Authority| -> Result<(), String> {
                deliveries.fetch_add(1, Ordering::Relaxed);
                delivered_generations
                    .lock()
                    .unwrap()
                    .push(generation.clone());
                Ok(())
            },
        ))
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.stops.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn complete_listener_topology_is_replaced_with_each_live_generation() {
    let a = plugin("fixture.a", None);
    let a_component = listener_component(&a.id, "fixture.a", None);
    let initial = ResolvedHarness::resolve(
        [a.clone()],
        [a_component.clone()],
        [],
        &Authority::default(),
    )
    .unwrap();
    let a_starts = Arc::new(AtomicUsize::new(0));
    let a_stops = Arc::new(AtomicUsize::new(0));
    let a_deliveries = Arc::new(AtomicUsize::new(0));
    let a_generations = Arc::new(Mutex::new(Vec::new()));
    let mut kernel = Kernel::new(initial.kernel_config().clone());
    kernel.preload_embedded_factory(a.id.clone(), {
        let starts = Arc::clone(&a_starts);
        let stops = Arc::clone(&a_stops);
        let deliveries = Arc::clone(&a_deliveries);
        let generations = Arc::clone(&a_generations);
        move || {
            Box::new(TrackingInstance {
                starts: Arc::clone(&starts),
                stops: Arc::clone(&stops),
                deliveries: Arc::clone(&deliveries),
                delivered_generations: Arc::clone(&generations),
                fail_binding: false,
                response: b"a".to_vec(),
            })
        }
    });
    kernel.activate_resolved_harness(&initial).unwrap();
    kernel.activate_all().unwrap();
    let events = kernel.events();
    events.dispatch(&event(1), &Authority::default()).unwrap();
    assert_eq!(a_starts.load(Ordering::Relaxed), 1);
    assert_eq!(a_deliveries.load(Ordering::Relaxed), 1);

    let resources = PluginManifest::resource_only(PluginId::parse("fixture.resources").unwrap());
    let retained = ResolvedHarness::resolve(
        [a.clone(), resources.clone()],
        [a_component.clone()],
        [],
        &Authority::default(),
    )
    .unwrap();
    let retained_generation = retained.generation().clone();
    let mut reconciler = GraphReconciler::new(initial);
    reconciler
        .activate_candidate_on_kernel(&mut kernel, retained)
        .unwrap();
    events.dispatch(&event(2), &Authority::default()).unwrap();
    assert_eq!(a_starts.load(Ordering::Relaxed), 1);
    assert_eq!(a_deliveries.load(Ordering::Relaxed), 2);
    assert_eq!(a_generations.lock().unwrap()[1], retained_generation);

    let service = ServiceId::parse("fixture.b.echo@1").unwrap();
    let b = plugin("fixture.b", Some(service.clone()));
    let b_component = listener_component(&b.id, "fixture.b", Some(service.clone()));
    let added = ResolvedHarness::resolve(
        [a.clone(), b.clone(), resources.clone()],
        [a_component.clone(), b_component.clone()],
        [],
        &Authority::default(),
    )
    .unwrap();
    let added_generation = added.generation().clone();
    let b_starts = Arc::new(AtomicUsize::new(0));
    let b_stops = Arc::new(AtomicUsize::new(0));
    let b_deliveries = Arc::new(AtomicUsize::new(0));
    let b_generations = Arc::new(Mutex::new(Vec::new()));
    kernel.preload_embedded_factory(b.id.clone(), {
        let starts = Arc::clone(&b_starts);
        let stops = Arc::clone(&b_stops);
        let deliveries = Arc::clone(&b_deliveries);
        let generations = Arc::clone(&b_generations);
        move || {
            Box::new(TrackingInstance {
                starts: Arc::clone(&starts),
                stops: Arc::clone(&stops),
                deliveries: Arc::clone(&deliveries),
                delivered_generations: Arc::clone(&generations),
                fail_binding: false,
                response: b"b".to_vec(),
            })
        }
    });
    reconciler
        .activate_candidate_on_kernel(&mut kernel, added)
        .unwrap();
    let active = kernel.active_resolved_graph().unwrap();
    assert!(active.listeners().all(|binding| {
        binding.generation == &added_generation
            && (binding.listener.owning_plugin == a.id || binding.listener.owning_plugin == b.id)
    }));
    assert_eq!(
        kernel
            .invoke_component(&b_component.id, &service, &[], &Authority::default(), &b.id,)
            .unwrap(),
        b"b"
    );
    events.dispatch(&event(3), &Authority::default()).unwrap();
    assert_eq!(a_starts.load(Ordering::Relaxed), 1);
    assert_eq!(b_starts.load(Ordering::Relaxed), 1);
    assert_eq!(b_deliveries.load(Ordering::Relaxed), 1);
    assert_eq!(b_generations.lock().unwrap()[0], added_generation);

    let removed = ResolvedHarness::resolve(
        [a.clone(), resources.clone()],
        [a_component.clone()],
        [],
        &Authority::default(),
    )
    .unwrap();
    reconciler
        .activate_candidate_on_kernel(&mut kernel, removed)
        .unwrap();
    events.dispatch(&event(4), &Authority::default()).unwrap();
    assert_eq!(a_deliveries.load(Ordering::Relaxed), 4);
    assert_eq!(b_deliveries.load(Ordering::Relaxed), 1);
    assert_eq!(b_stops.load(Ordering::Relaxed), 1);
    assert!(kernel
        .invoke(&service, &[], &Authority::default(), None)
        .is_err());

    let c = plugin("fixture.c", None);
    let c_component = listener_component(&c.id, "fixture.c", None);
    let rejected = ResolvedHarness::resolve(
        [a.clone(), c.clone(), resources],
        [a_component, c_component],
        [],
        &Authority::default(),
    )
    .unwrap();
    let active_generation = kernel.graph_generation().unwrap().clone();
    let c_stops = Arc::new(AtomicUsize::new(0));
    kernel.preload_embedded_instance(
        c.id.clone(),
        Box::new(TrackingInstance {
            starts: Arc::new(AtomicUsize::new(0)),
            stops: Arc::clone(&c_stops),
            deliveries: Arc::new(AtomicUsize::new(0)),
            delivered_generations: Arc::new(Mutex::new(Vec::new())),
            fail_binding: true,
            response: Vec::new(),
        }),
    );
    let error = reconciler
        .activate_candidate_on_kernel(&mut kernel, rejected)
        .unwrap_err();
    assert!(matches!(
        error,
        LiveReconciliationError::Runtime(KernelError::ListenerBinding { plugin, .. })
            if plugin == c.id
    ));
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(reconciler.active().generation(), &active_generation);
    events.dispatch(&event(5), &Authority::default()).unwrap();
    assert_eq!(a_deliveries.load(Ordering::Relaxed), 5);
    assert_eq!(b_deliveries.load(Ordering::Relaxed), 1);
    assert_eq!(c_stops.load(Ordering::Relaxed), 1);
}

#[test]
fn listener_dependency_cycles_fail_during_candidate_resolution() {
    let owner = plugin("fixture.cycle", None);
    let mut first = listener_component(&owner.id, "fixture.first", None);
    let mut second = listener_component(&owner.id, "fixture.second", None);
    first.listeners[0].dependencies = vec![second.listeners[0].id.clone()];
    second.listeners[0].dependencies = vec![first.listeners[0].id.clone()];

    let error =
        ResolvedHarness::resolve([owner], [first, second], [], &Authority::default()).unwrap_err();

    assert!(matches!(
        error,
        crate::ResolvedHarnessError::ComponentGraph(crate::ComponentGraphError::ListenerTopology(
            crate::EventError::DependencyCycle(_)
        ))
    ));
}
