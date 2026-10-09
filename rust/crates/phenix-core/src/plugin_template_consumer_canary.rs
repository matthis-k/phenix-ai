//! Non-agent composition canary for the future Stage E template consumers.
//!
//! This fixture proves that independently authored, resource-only topology
//! and two executable providers can resolve using Core without selecting the
//! first-party Tool, Skill, Model or Agent loop plugin. Kind/template metadata
//! publication is owned by Stage D; no mock template registry is added here.

use crate::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    PluginExecution, PluginId, PluginManifest, ResolvedComponentGraph,
};

fn plugin(name: &str, execution: PluginExecution) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(name).unwrap(),
        version: 1,
        execution,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn component(
    name: &str,
    owner: &str,
    import: Vec<ComponentImport>,
    export: Vec<ComponentExport>,
) -> ComponentManifest {
    ComponentManifest {
        id: ComponentId::parse(name).unwrap(),
        owner: PluginId::parse(owner).unwrap(),
        imports: import,
        exports: export,
        listeners: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn service(name: &str) -> InterfaceId {
    InterfaceId::parse(name).unwrap()
}

#[test]
fn resource_only_non_agent_consumer_resolves_two_independent_provider_plugins() {
    let source = service("example.measurement@1");
    let sink = service("example.archive@1");
    let manifests = [
        plugin("example.measurement-plugin", PluginExecution::Embedded),
        plugin("example.archive-plugin", PluginExecution::Embedded),
        plugin("example.workflow-topology", PluginExecution::ResourceOnly),
    ];
    assert!(
        manifests
            .iter()
            .all(|plugin| !plugin.id.as_str().contains("agent"))
    );
    let components = [
        component(
            "example.measurement-provider",
            "example.measurement-plugin",
            Vec::new(),
            vec![ComponentExport {
                interface: source.clone(),
                schema: Default::default(),
                priority: 100,
                required_authority: Authority::default(),
            }],
        ),
        component(
            "example.archive-provider",
            "example.archive-plugin",
            Vec::new(),
            vec![ComponentExport {
                interface: sink.clone(),
                schema: Default::default(),
                priority: 100,
                required_authority: Authority::default(),
            }],
        ),
        component(
            "example.measurement-topology",
            "example.workflow-topology",
            vec![source.clone(), sink.clone()]
                .into_iter()
                .map(|interface| ComponentImport {
                    interface,
                    schema: Default::default(),
                    required: true,
                    authority: Authority::default(),
                })
                .collect(),
            Vec::new(),
        ),
    ];
    let graph = ResolvedComponentGraph::compile(
        manifests.clone(),
        components.clone(),
        &Authority::default(),
    )
    .unwrap();
    let consumer = ComponentId::parse("example.measurement-topology").unwrap();
    assert_eq!(
        graph
            .import_handle(&consumer, &source)
            .unwrap()
            .unwrap()
            .exporter(),
        &ComponentId::parse("example.measurement-provider").unwrap()
    );
    assert_eq!(
        graph
            .import_handle(&consumer, &sink)
            .unwrap()
            .unwrap()
            .exporter(),
        &ComponentId::parse("example.archive-provider").unwrap()
    );
    // Removing either selected provider fails admission; Core does not
    // synthesize a default tool catalog or an agent-specific fallback.
    assert!(
        ResolvedComponentGraph::compile(
            manifests
                .into_iter()
                .filter(|plugin| plugin.id.as_str() != "example.archive-plugin"),
            components
                .into_iter()
                .filter(|component| component.owner.as_str() != "example.archive-plugin"),
            &Authority::default(),
        )
        .is_err()
    );
}
