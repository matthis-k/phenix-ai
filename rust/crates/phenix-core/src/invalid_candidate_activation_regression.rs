use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    Kernel, KernelConfig, PluginExecution, PluginId, PluginManifest, ResolvedComponentGraph,
    ResolvedGeneration, ResolvedGenerationActivation,
};

fn plugin() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("fixture.package").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[test]
fn invalid_candidate_resolution_leaves_active_generation_unchanged() {
    let plugin = plugin();
    let active =
        ResolvedGeneration::resolve([plugin.clone()], [], [], &Authority::default()).unwrap();
    let mut kernel = Kernel::new(KernelConfig::new([plugin.clone()]).unwrap());
    kernel.activate_resolved_generation(&active).unwrap();
    let active_generation = kernel.graph_generation().cloned().unwrap();
    let active_graph: ResolvedComponentGraph = kernel.component_graph().clone();

    let invalid_component = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("fixture.consumer").unwrap(),
        owner: plugin.id.clone(),
        imports: vec![ComponentImport {
            interface: InterfaceId::parse("fixture.missing@1").unwrap(),
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    };

    assert!(
        ResolvedGeneration::resolve([plugin], [invalid_component], [], &Authority::default(),)
            .is_err()
    );
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(kernel.component_graph(), &active_graph);
}

#[test]
fn removing_a_required_live_provider_rejects_the_candidate_and_retains_the_active_generation() {
    let plugin = plugin();
    let interface = InterfaceId::parse("fixture.required@1").unwrap();
    let provider = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("fixture.provider").unwrap(),
        owner: plugin.id.clone(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: interface.clone(),
            schema: Default::default(),
            priority: 0,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    };
    let consumer = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("fixture.consumer").unwrap(),
        owner: plugin.id.clone(),
        imports: vec![ComponentImport {
            interface,
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    };

    let active = ResolvedGeneration::resolve(
        [plugin.clone()],
        [provider, consumer.clone()],
        [],
        &Authority::default(),
    )
    .unwrap();
    let mut kernel = Kernel::new(KernelConfig::new([plugin.clone()]).unwrap());
    kernel.activate_resolved_generation(&active).unwrap();
    let active_generation = kernel.graph_generation().cloned().unwrap();
    let active_graph: ResolvedComponentGraph = kernel.component_graph().clone();

    let candidate = ResolvedGeneration::resolve([plugin], [consumer], [], &Authority::default());

    assert!(candidate.is_err());
    assert_eq!(kernel.graph_generation(), Some(&active_generation));
    assert_eq!(kernel.component_graph(), &active_graph);
}
