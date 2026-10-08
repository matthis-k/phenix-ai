use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    PluginExecution, PluginId, PluginManifest, ProviderCompositionPolicy, ResolvedGeneration,
};

fn plugin(id: &str) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(id).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn provider(id: &str, owner: &str, interface: &InterfaceId, priority: i32) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(id).unwrap(),
        owner: PluginId::parse(owner).unwrap(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: interface.clone(),
            schema: Default::default(),
            priority,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn consumer(interface: &InterfaceId) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("consumer").unwrap(),
        owner: PluginId::parse("consumer-package").unwrap(),
        imports: vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[test]
fn provider_rebinding_requires_a_new_resolved_generation() {
    let interface = InterfaceId::parse("fixture.provider@1").unwrap();
    let consumer = consumer(&interface);
    let provider_a = provider("provider-a", "provider-a-package", &interface, 100);
    let provider_b = provider("provider-b", "provider-b-package", &interface, 1);
    let provider_b_id = ComponentId::parse("provider-b").unwrap();
    let policy = ProviderCompositionPolicy::new().with_priority(
        interface.clone(),
        provider_b_id.clone(),
        20,
    );
    let plugins = [
        plugin("consumer-package"),
        plugin("provider-a-package"),
        plugin("provider-b-package"),
    ];

    let active = ResolvedGeneration::resolve_with_provider_policy(
        plugins.clone(),
        [consumer.clone(), provider_a.clone()],
        [],
        policy.clone(),
        &Authority::default(),
    )
    .unwrap();
    let candidate = ResolvedGeneration::resolve_with_provider_policy(
        plugins,
        [consumer.clone(), provider_a, provider_b],
        [],
        policy,
        &Authority::default(),
    )
    .unwrap();

    let active_binding = active
        .component_graph()
        .import_handle(&consumer.id, &interface)
        .unwrap()
        .unwrap();
    let candidate_binding = candidate
        .component_graph()
        .import_handle(&consumer.id, &interface)
        .unwrap()
        .unwrap();

    assert_eq!(
        active_binding.exporter(),
        &ComponentId::parse("provider-a").unwrap()
    );
    assert_eq!(candidate_binding.exporter(), &provider_b_id);
    assert_ne!(active.generation(), candidate.generation());
    assert_eq!(
        active
            .component_graph()
            .import_handle(&consumer.id, &interface)
            .unwrap()
            .unwrap()
            .exporter(),
        &ComponentId::parse("provider-a").unwrap()
    );
}

#[test]
fn explicit_provider_selection_must_not_silently_fall_back() {
    let interface = InterfaceId::parse("fixture.provider@1").unwrap();
    let selected = ComponentId::parse("provider-b").unwrap();
    let other = ComponentId::parse("provider-a").unwrap();
    let plugin_manifests = [
        plugin("consumer-package"),
        plugin("provider-a-package"),
        plugin("provider-b-package"),
    ];
    let components = [
        consumer(&interface),
        provider("provider-a", "provider-a-package", &interface, 100),
    ];
    let policy =
        ProviderCompositionPolicy::new().with_explicit_binding(interface.clone(), selected.clone());

    let error = ResolvedGeneration::resolve_with_provider_policy(
        plugin_manifests.clone(),
        components.clone(),
        [],
        policy.clone(),
        &Authority::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("explicitly requires provider provider-b"),
        "absent explicit bindings must report the selected provider: {error}"
    );

    let error = ResolvedGeneration::resolve_with_provider_policy(
        plugin_manifests,
        [
            components[0].clone(),
            components[1].clone(),
            provider("provider-b", "provider-b-package", &interface, 0),
        ],
        [],
        policy.with_disabled_provider(interface.clone(), selected.clone()),
        &Authority::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("explicitly requires provider provider-b"),
        "disabled explicit bindings must not fall back to provider-a: {error}"
    );

    // This remains a normal valid binding when the explicit provider exists.
    let valid = ResolvedGeneration::resolve_with_provider_policy(
        [
            plugin("consumer-package"),
            plugin("provider-a-package"),
            plugin("provider-b-package"),
        ],
        [
            consumer(&interface),
            provider("provider-a", "provider-a-package", &interface, 100),
            provider("provider-b", "provider-b-package", &interface, 0),
        ],
        [],
        ProviderCompositionPolicy::new().with_explicit_binding(interface.clone(), selected.clone()),
        &Authority::default(),
    )
    .unwrap();
    assert_eq!(
        valid
            .component_graph()
            .import_handle(&ComponentId::parse("consumer").unwrap(), &interface)
            .unwrap()
            .unwrap()
            .exporter(),
        &selected
    );
    assert_ne!(other, selected);
}
