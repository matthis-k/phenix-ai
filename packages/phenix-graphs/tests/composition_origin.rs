use phenix_graph::{GraphBuilder, GraphOrigin, Layout, ModulePackage, NodeTemplate};

fn module(name: &str, origin: &GraphOrigin) -> ModulePackage {
    let mut graph = GraphBuilder::new(name);
    graph.with_origin(origin, |graph| {
        let node = graph.node("private", |x: i64| x).unwrap();
        graph.expose_input("request", node.inlet()).unwrap();
        graph.expose_output("response", node.output()).unwrap();
    });
    graph.module()
}

#[test]
fn origin_is_allocation_identity_and_scope_does_not_relabel_existing_units() {
    let owner = GraphOrigin::default();
    let foreign = GraphOrigin::default();
    assert_eq!(owner, owner.clone());
    assert_ne!(owner, foreign);
    let mut graph = GraphBuilder::new("host");
    graph.node("unattributed", |x: i64| x).unwrap();
    graph.with_origin(&owner, |graph| {
        graph.node("owned", |x: i64| x).unwrap();
    });
    graph.with_origin(&foreign, |graph| {
        graph.inspect("owned").unwrap();
    });
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.find("owned").unwrap().origin(), Some(&owner));
    assert_eq!(compiled.find("unattributed").unwrap().origin(), None);
}

#[test]
fn origins_survive_cloning_pending_layout_and_low_level_registration() {
    let owner = GraphOrigin::default();
    let mut graph = GraphBuilder::new("host");
    let handle = graph.with_origin(&owner, |graph| {
        graph
            .add("raw", NodeTemplate::local_function(|x: i64| x))
            .unwrap();
        graph.node("configured", |x: i64| x).unwrap()
    });
    handle.input(Layout::atomic()).unwrap();
    for graph in [graph.clone(), graph] {
        let compiled = graph.compile().unwrap();
        for name in ["raw", "configured"] {
            assert_eq!(compiled.find(name).unwrap().origin(), Some(&owner));
        }
    }
}

#[test]
fn replacements_record_the_replacing_origin_and_failed_edits_preserve_it() {
    let original = GraphOrigin::default();
    let replacement = GraphOrigin::default();
    let mut graph = GraphBuilder::new("host");
    graph.with_origin(&original, |graph| {
        graph.node("node", |x: i64| x).unwrap();
    });
    graph.with_origin(&replacement, |graph| {
        assert!(
            graph
                .replace("node", NodeTemplate::local_function(|x: String| x))
                .is_err()
        );
    });
    assert_eq!(
        graph
            .clone()
            .compile()
            .unwrap()
            .find("node")
            .unwrap()
            .origin(),
        Some(&original)
    );
    graph.with_origin(&replacement, |graph| {
        graph
            .replace("node", NodeTemplate::local_function(|x: i64| x + 1))
            .unwrap();
    });
    assert_eq!(
        graph
            .clone()
            .compile()
            .unwrap()
            .find("node")
            .unwrap()
            .origin(),
        Some(&replacement)
    );
    graph
        .replace("node", NodeTemplate::local_function(|x: i64| x + 2))
        .unwrap();
    assert_eq!(
        graph.compile().unwrap().find("node").unwrap().origin(),
        None
    );
}

#[test]
fn module_boundary_origin_tracks_instantiation_not_definition_or_contract() {
    let implementation = GraphOrigin::default();
    let first = GraphOrigin::default();
    let second = GraphOrigin::default();
    let package = module("implementation", &implementation);
    let mut nested = GraphBuilder::new("nested");
    nested.with_origin(&first, |graph| {
        graph.instance("service", &package).unwrap();
        let output = graph
            .discover("service")
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()
            .unwrap();
        graph.expose_output("response", output).unwrap();
    });
    let nested = nested.module();
    let mut graph = GraphBuilder::new("host");
    graph.with_origin(&second, |graph| {
        graph.instance("outer", &nested).unwrap();
    });
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.find("outer").unwrap().origin(), Some(&second));
    assert_eq!(
        compiled.find("outer/service").unwrap().origin(),
        Some(&first)
    );
    assert_eq!(
        compiled.find("outer/service/private").unwrap().origin(),
        Some(&implementation)
    );

    let mut graph = GraphBuilder::new("replacement-host");
    graph.with_origin(&first, |graph| {
        graph.instance("service", &package).unwrap();
    });
    graph.with_origin(&second, |graph| {
        graph
            .replace("service", module("new-implementation", &implementation))
            .unwrap();
    });
    assert_eq!(
        graph
            .clone()
            .compile()
            .unwrap()
            .find("service")
            .unwrap()
            .origin(),
        Some(&second)
    );
    graph
        .replace("service", module("unscoped-replacement", &implementation))
        .unwrap();
    assert_eq!(
        graph.compile().unwrap().find("service").unwrap().origin(),
        None
    );
}

#[test]
fn nested_origin_scopes_restore_after_error_and_panic() {
    let outer = GraphOrigin::default();
    let inner = GraphOrigin::default();
    let mut graph = GraphBuilder::new("host");
    graph.with_origin(&outer, |graph| {
        let result: Result<(), ()> = graph.with_origin(&inner, |graph| {
            graph.node("inner", |x: i64| x).unwrap();
            Err(())
        });
        assert!(result.is_err());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            graph.with_origin(&inner, |_| panic!("scope test"));
        }));
        assert!(panic.is_err());
        graph.node("outer", |x: i64| x).unwrap();
    });
    graph.node("unscoped", |x: i64| x).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.find("inner").unwrap().origin(), Some(&inner));
    assert_eq!(compiled.find("outer").unwrap().origin(), Some(&outer));
    assert_eq!(compiled.find("unscoped").unwrap().origin(), None);
}
