use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    PermissionId, PluginExecution, PluginId, PluginManifest, ResolvedComponentGraph,
};

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).unwrap()
}

fn plugin(id: &str, authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(id).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: authority,
    }
}

#[test]
fn third_party_plugin_defines_a_typed_runtime_interface_without_core_registration() {
    let interface = InterfaceId::parse("acme.compiler-review@7").unwrap();
    let use_review = capability("acme.compiler-review.use");
    let unrelated = capability("acme.unrelated");
    let harness_authority = Authority::new([use_review.clone(), unrelated]);

    let provider = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("acme.review-provider").unwrap(),
        owner: PluginId::parse("acme.review-plugin").unwrap(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: interface.clone(),
            schema: Default::default(),
            priority: 100,
            required_authority: Authority::new([use_review.clone()]),
        }],
        maximum_authority: Authority::new([use_review.clone()]),
    };
    let consumer = ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("acme.compiler-agent").unwrap(),
        owner: PluginId::parse("acme.agent-plugin").unwrap(),
        imports: vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: Authority::new([use_review.clone()]),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::new([use_review.clone()]),
    };

    let graph = ResolvedComponentGraph::compile(
        [
            plugin("acme.review-plugin", Authority::new([use_review.clone()])),
            plugin("acme.agent-plugin", Authority::new([use_review.clone()])),
        ],
        [provider, consumer],
        &harness_authority,
    )
    .unwrap();

    let handle = graph
        .import_handle(
            &ComponentId::parse("acme.compiler-agent").unwrap(),
            &interface,
        )
        .unwrap()
        .expect("required third-party import is bound before activation");
    assert_eq!(
        handle.exporter(),
        &ComponentId::parse("acme.review-provider").unwrap()
    );
    assert!(handle.effective_authority().permits(&use_review));
    assert!(
        !handle
            .effective_authority()
            .permits(&capability("acme.unrelated"))
    );
}
