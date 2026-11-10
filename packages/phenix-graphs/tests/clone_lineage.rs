use phenix_graph::{Deployment, GraphBuilder, ModulePackage, NodeTemplate, Output, Store, Value};

fn module(name: &str) -> ModulePackage {
    let mut graph = GraphBuilder::new(name);
    let node = graph.node("inner", |x: i64| x).unwrap();
    graph.expose_input("request", node.inlet()).unwrap();
    graph.expose_output("result", node.output()).unwrap();
    graph.module()
}

fn module_input(
    instance: &phenix_graph::ModuleHandle,
    port: &'static str,
) -> Result<phenix_graph::Input<i64, phenix_graph::LocalValue>, String> {
    instance
        .operation()?
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new(port))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
}

#[test]
fn unchanged_inherited_endpoints_allow_clone_preflight() {
    let mut original = GraphBuilder::new("inherited");
    let source = original.node("source", |x: i64| x).unwrap();
    let sink = original.node("sink", |x: i64| x).unwrap();
    let input = original.expose_input("request", source.inlet()).unwrap();
    let mut preflight = original.clone();
    preflight.connect(source.output(), sink.inlet()).unwrap();
    preflight.trigger("run", [input.clone()]).unwrap();
    preflight.compile().unwrap();
    original.connect(source.output(), sink.inlet()).unwrap();
    original.trigger("run", [input]).unwrap();
    original.compile().unwrap();
}

#[test]
fn sibling_same_name_nodes_and_public_inputs_do_not_alias() {
    let mut original = GraphBuilder::new("siblings");
    let mut left = original.clone();
    let mut right = original.clone();
    let a = left.node("same", |x: i64| x).unwrap();
    let b = right.node("same", |x: i64| x).unwrap();
    let c = original.node("same", |x: i64| x).unwrap();
    assert!(right.connect(a.output(), b.inlet()).is_err());
    assert!(left.connect(b.output(), a.inlet()).is_err());
    assert!(original.expose_input("foreign", a.inlet()).is_err());
    assert!(original.expose_output("foreign", a.output()).is_err());
    let input = left.expose_input("request", a.inlet()).unwrap();
    right.expose_input("request", b.inlet()).unwrap();
    original.expose_input("request", c.inlet()).unwrap();
    assert!(right.trigger("run", [input.clone()]).is_err());
    assert!(original.trigger("run", [input]).is_err());
    right.trigger("run", ["request"]).unwrap();
    right.compile().unwrap();
}

#[test]
fn same_shape_node_replacement_revokes_cached_and_newly_projected_endpoints() {
    let mut graph = GraphBuilder::new("replace");
    let source = graph.node("source", |x: i64| x).unwrap();
    let sink = graph.node("sink", |x: i64| x).unwrap();
    let old_input = sink.inlet();
    let old_output = sink.output();
    let mut branch = graph.clone();
    branch
        .replace("sink", NodeTemplate::local_function(|x: i64| x + 1))
        .unwrap();
    assert!(branch.connect(source.output(), old_input.clone()).is_err());
    assert!(branch.expose_input("stale", sink.inlet()).is_err());
    assert!(branch.expose_output("stale", old_output).is_err());
    graph.connect(source.output(), old_input.clone()).unwrap();
    assert!(branch.disconnect(source.output(), old_input).is_err());
    let fresh = branch
        .discover("sink")
        .unwrap()
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("run"))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
        .unwrap();
    assert!(graph.expose_input("foreign", fresh.clone()).is_err());
    branch.connect(source.output(), fresh).unwrap();
    branch.compile().unwrap();
    graph.compile().unwrap();
}

#[test]
fn module_handles_are_revoked_on_replacement_and_isolated_between_siblings() {
    let first = module("first");
    let second = module("second");
    let mut original = GraphBuilder::new("module-lineage");
    let mut sibling = original.clone();
    let old = original.instance("same", &first).unwrap();
    sibling.instance("same", &first).unwrap();
    assert!(
        sibling
            .expose_input("foreign", module_input(&old, "request").unwrap())
            .is_err()
    );
    let mut branch = original.clone();
    branch
        .expose_output(
            "inherited",
            old.operation()
                .unwrap()
                .output::<i64, phenix_graph::LocalValue>()
                .unwrap(),
        )
        .unwrap();
    branch.replace("same", &second).unwrap();
    assert!(
        branch
            .expose_input("stale", module_input(&old, "request").unwrap())
            .is_err()
    );
    assert!(
        branch
            .expose_output(
                "stale",
                old.operation()
                    .unwrap()
                    .output::<i64, phenix_graph::LocalValue>()
                    .unwrap()
            )
            .is_err()
    );
    let fresh = branch
        .discover("same")
        .unwrap()
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("request"))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
        .unwrap();
    assert!(original.expose_input("foreign", fresh.clone()).is_err());
    branch.expose_input("fresh", fresh).unwrap();
    original
        .expose_input("valid", module_input(&old, "request").unwrap())
        .unwrap();
    branch.compile().unwrap();
    original.compile().unwrap();
}

#[test]
fn cloned_definition_packages_are_not_conflated_even_without_edits() {
    for edited in [false, true] {
        let mut definition = GraphBuilder::new("definition");
        let node = definition.node("n", |x: i64| x).unwrap();
        definition.expose_output("value", node.output()).unwrap();
        let mut clone = definition.clone();
        if edited {
            clone
                .replace("n", NodeTemplate::local_function(|x: i64| x + 1))
                .unwrap();
        }
        let package = definition.module();
        let clone = clone.module();
        let mut parent = GraphBuilder::new("parent");
        parent.instance("one", &package).unwrap();
        assert!(
            parent
                .instance("conflict", &clone)
                .err()
                .unwrap()
                .contains("conflicting module definition")
        );
        parent.instance("two", &package).unwrap();
        parent.compile().unwrap();
    }
}

#[test]
fn discovered_operations_reject_same_name_sibling_declarations() {
    let mut original = GraphBuilder::new("discovered-lineage");
    let mut sibling = original.clone();
    original.node("same", |x: i64| x).unwrap();
    let second = sibling.node("same", |x: i64| x).unwrap();
    let first = original.discover("same").unwrap();
    assert!(
        sibling
            .connect(
                first.output::<i64, phenix_graph::LocalValue>().unwrap(),
                second.inlet()
            )
            .is_err()
    );
    let input = first
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("run"))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
        .unwrap();
    assert!(sibling.expose_input("foreign", input.clone()).is_err());
    let mut inherited = original.clone();
    inherited.expose_input("valid", input).unwrap();
}

#[test]
fn directed_output_checks_declaration_lineage_for_nodes_and_modules() {
    for is_module in [false, true] {
        for mode in ["sibling", "replacement", "inherited"] {
            let package = module("destination");
            let replacement = module("replacement");
            let mut original = GraphBuilder::new("routing");
            let mut branch = original.clone();
            let target = if is_module {
                let instance = original.instance("sink", &package).unwrap();
                module_input(&instance, "request").unwrap()
            } else {
                original.node("sink", |x: i64| x).unwrap().inlet()
            };
            if mode != "sibling" {
                branch = original.clone();
                if mode == "replacement" {
                    if is_module {
                        branch.replace("sink", &replacement).unwrap();
                    } else {
                        branch
                            .replace("sink", NodeTemplate::local_function(|x: i64| x + 1))
                            .unwrap();
                    }
                }
            } else {
                if is_module {
                    branch.instance("sink", &package).unwrap();
                } else {
                    branch.node("sink", |x: i64| x).unwrap();
                }
            }
            assert_selection_lineage(branch, target, mode == "inherited");
        }
    }
}

fn assert_selection_lineage(
    mut graph: GraphBuilder,
    target: phenix_graph::Input<i64>,
    valid: bool,
) {
    graph
        .node("source", move |x: i64| Output::to(&target, x))
        .unwrap();
    graph.connect("source", "sink").unwrap();
    let request = graph.expose_input("request", "source").unwrap();
    graph.trigger("run", [request]).unwrap();
    let compiled = graph.compile().unwrap();
    let sink_id = match compiled.find("sink").unwrap() {
        phenix_graph::FindResult::Node(node) => node.id(),
        phenix_graph::FindResult::Module(_) => match compiled.find("sink/inner").unwrap() {
            phenix_graph::FindResult::Node(node) => node.id(),
            _ => unreachable!("fixture contains a node"),
        },
    };
    let report = Deployment::new(compiled)
        .store(Store::new())
        .build()
        .unwrap()
        .fire("run", vec![Value::local(1_i64)])
        .unwrap();
    assert_eq!(report.invocation_count(sink_id), usize::from(valid));
    if valid {
        assert!(report.success(), "{:?}", report.error_messages());
        return;
    }
    assert!(!report.success());
    assert!(
        report
            .error_messages()
            .iter()
            .any(|message| message.contains("stale or from a divergent declaration")),
        "{:?}",
        report.error_messages()
    );
}
