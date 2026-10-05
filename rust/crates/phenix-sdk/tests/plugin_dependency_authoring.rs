mod sessions {
    #[phenix_sdk::plugin("fixture.sessions")]
    pub struct Plugin;
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.parent")]
struct Plugin {
    #[phenix(dep)]
    sessions: sessions::Plugin,
}

mod diamond {
    #[phenix_sdk::plugin("fixture.diamond.base")]
    pub struct Base;

    #[allow(dead_code)]
    #[phenix_sdk::plugin("fixture.diamond.left")]
    pub struct Left {
        #[phenix(dep)]
        base: Base,
    }

    #[allow(dead_code)]
    #[phenix_sdk::plugin("fixture.diamond.right")]
    pub struct Right {
        #[phenix(dep)]
        base: Base,
    }

    #[allow(dead_code)]
    #[phenix_sdk::plugin("fixture.diamond.root")]
    pub struct Root {
        #[phenix(dep)]
        left: Left,
        #[phenix(dep)]
        right: Right,
    }
}

#[phenix_sdk::plugin(
    id = "fixture.resource-only",
    execution = phenix_sdk::PluginExecution::ResourceOnly
)]
struct ResourceOnly;

fn external_execution() -> phenix_sdk::PluginExecution {
    phenix_sdk::PluginExecution::Runtime {
        runtime: phenix_sdk::PluginRuntimeId::parse("fixture.runtime").unwrap(),
        artifact: phenix_sdk::PluginArtifact {
            locator: "fixture.wasm".into(),
            revision: phenix_sdk::ArtifactRevision::from_content(b"fixture"),
            configuration: std::collections::BTreeMap::new(),
        },
    }
}

#[phenix_sdk::plugin(
    id = "fixture.external",
    execution = external_execution()
)]
struct External;

struct DuplicateFirst;
struct DuplicateSecond;
struct DuplicateRoot;
struct CycleA;
struct CycleB;

fn descriptor(id: &str, definition: &'static str) -> phenix_sdk::StaticPluginDescriptor {
    phenix_sdk::StaticPluginDescriptor {
        id: phenix_sdk::PluginId::parse(id).unwrap(),
        definition,
        version: 1,
        execution: phenix_sdk::__phenix_plugin::PluginExecution::Embedded,
        maximum_authority: phenix_sdk::Authority::default(),
        dependencies: Vec::new(),
        embedded_factory: None,
    }
}

#[test]
fn direct_dependency_module_reexports_only_the_declared_plugin_type() {
    fn accepts_dependency(_: plugin::dependencies::sessions::Plugin) {}

    accepts_dependency(sessions::Plugin);
}

#[test]
fn plugin_execution_defaults_to_embedded() {
    let descriptor = <Plugin as phenix_sdk::StaticPluginDefinition>::descriptor();
    assert!(matches!(
        descriptor.execution,
        phenix_sdk::PluginExecution::Embedded
    ));
}

#[test]
fn plugin_execution_preserves_resource_only_and_runtime_metadata() {
    let resource = <ResourceOnly as phenix_sdk::StaticPluginDefinition>::descriptor();
    assert!(matches!(
        resource.execution,
        phenix_sdk::PluginExecution::ResourceOnly
    ));

    let external = <External as phenix_sdk::StaticPluginDefinition>::descriptor();
    let phenix_sdk::PluginExecution::Runtime { runtime, artifact } = external.execution else {
        panic!("external plugin should preserve runtime execution metadata");
    };
    assert_eq!(runtime.as_str(), "fixture.runtime");
    assert_eq!(artifact.locator, "fixture.wasm");
    assert_eq!(
        artifact.revision,
        phenix_sdk::ArtifactRevision::from_content(b"fixture")
    );
}

#[test]
fn concrete_dependency_still_participates_in_recursive_graph_composition() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<Plugin>().unwrap();
    let ids = graph
        .ids()
        .map(phenix_sdk::PluginId::as_str)
        .collect::<Vec<_>>();

    assert_eq!(ids, ["fixture.parent", "fixture.sessions"]);
}

#[test]
fn diamond_dependencies_are_deduplicated_by_plugin_id() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<diamond::Root>().unwrap();
    let ids = graph
        .ids()
        .map(phenix_sdk::PluginId::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        [
            "fixture.diamond.base",
            "fixture.diamond.left",
            "fixture.diamond.right",
            "fixture.diamond.root",
        ]
    );
}

impl phenix_sdk::StaticPluginDefinition for DuplicateFirst {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.duplicate", "fixture::DuplicateFirst")
    }
}

impl phenix_sdk::StaticPluginDefinition for DuplicateSecond {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        descriptor("fixture.duplicate", "fixture::DuplicateSecond")
    }
}

impl phenix_sdk::StaticPluginDefinition for DuplicateRoot {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.duplicate-root", "fixture::DuplicateRoot");
        descriptor.dependencies = vec![
            phenix_sdk::StaticPluginDependency::of::<DuplicateFirst>(),
            phenix_sdk::StaticPluginDependency::of::<DuplicateSecond>(),
        ];
        descriptor
    }
}

#[test]
fn incompatible_duplicate_plugin_ids_are_rejected() {
    let error = match phenix_sdk::StaticPluginGraph::compose::<DuplicateRoot>() {
        Ok(_) => panic!("duplicate plugin identities must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        phenix_sdk::StaticPluginGraphError::DuplicateId { ref id, first, second }
            if id.as_str() == "fixture.duplicate"
                && first == "fixture::DuplicateFirst"
                && second == "fixture::DuplicateSecond"
    ));
}

impl phenix_sdk::StaticPluginDefinition for CycleA {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.cycle-a", "fixture::CycleA");
        descriptor.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<CycleB>()];
        descriptor
    }
}

impl phenix_sdk::StaticPluginDefinition for CycleB {
    fn descriptor() -> phenix_sdk::StaticPluginDescriptor {
        let mut descriptor = descriptor("fixture.cycle-b", "fixture::CycleB");
        descriptor.dependencies = vec![phenix_sdk::StaticPluginDependency::of::<CycleA>()];
        descriptor
    }
}

#[test]
fn dependency_cycles_are_rejected() {
    let error = match phenix_sdk::StaticPluginGraph::compose::<CycleA>() {
        Ok(_) => panic!("dependency cycles must be rejected"),
        Err(error) => error,
    };

    let phenix_sdk::StaticPluginGraphError::Cycle { ref path } = error else {
        panic!("expected dependency cycle error");
    };
    assert_eq!(
        path.iter()
            .map(phenix_sdk::PluginId::as_str)
            .collect::<Vec<_>>(),
        ["fixture.cycle-a", "fixture.cycle-b", "fixture.cycle-a"]
    );
    assert_eq!(
        error.to_string(),
        "static plugin dependency cycle: fixture.cycle-a -> fixture.cycle-b -> fixture.cycle-a"
    );
}
