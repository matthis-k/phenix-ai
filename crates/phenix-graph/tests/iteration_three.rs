//! Core-first iteration: the same graph API handles conditional nodes,
//! interface-preserving replacements, compilation, and execution. These are
//! real Rust integration tests: no Python oracle and no secondary executor.
use phenix_graph::Output;
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{GraphBuilder, Node, NodeTemplate, PatchBuilder, PatchPlanner, Value};

fn increment_graph(delta: i64, connect_sink: bool) -> std::sync::Arc<phenix_graph::CompiledGraph> {
    let mut graph = GraphBuilder::new("increment");
    let inc = graph.node("inc", move |x: i64| x + delta).unwrap();
    let input = graph.expose_input("request", inc.inlet()).unwrap();
    graph.expose_output("result", inc.output()).unwrap();
    graph.trigger("run", [input]).unwrap();
    if connect_sink {
        let sink = graph.node("sink", |x: i64| x * 3).unwrap();
        graph.connect(inc.output(), sink.inlet()).unwrap();
    }
    graph.compile().unwrap()
}

#[test]
fn topology_manifest_is_reproducible_but_never_pretends_to_hash_closure_behavior() {
    let one = increment_graph(1, false).topology_manifest();
    let again = increment_graph(1, false).topology_manifest();
    let different_closure = increment_graph(100, false).topology_manifest();
    let different_topology = increment_graph(1, true).topology_manifest();
    assert_eq!(one, again);
    assert_eq!(
        one, different_closure,
        "the descriptor is deliberately NOT an executable/generation identity"
    );
    assert_ne!(one, different_topology);
    assert!(one.contains("trigger"));
    assert!(different_topology.contains("edge"));
}

#[test]
fn conditional_node_uses_regular_node_definition_and_named_output_assertions() {
    let mut graph = GraphBuilder::new("choice");
    let yes = graph.node("yes", |x: i64| x + 10).unwrap();
    let no = graph.node("no", |x: i64| x + 20).unwrap();
    let yes_target = yes.inlet();
    let no_target = no.inlet();
    let choice = graph
        .node("choice", move |x: i64| {
            Output::to(if x >= 0 { &yes_target } else { &no_target }, x.abs())
        })
        .unwrap();
    graph.connect(choice.output(), yes.inlet()).unwrap();
    graph.connect(choice.output(), no.inlet()).unwrap();
    let trigger = graph.expose_input("request", choice.inlet()).unwrap();
    graph.trigger("run", [trigger]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let left = fixture.output("yes", "value");
    let right = fixture.output("no", "value");
    fixture
        .fire("run", vec![Value::local(2_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(left, &12_i64)
        .expect_publication_count(left, 1)
        .expect_publication_count(right, 0);
    fixture
        .fire("run", vec![Value::local(-3_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(right, &23_i64)
        .expect_publication_count(left, 0)
        .expect_publication_count(right, 1);
}

#[test]
fn invalid_conditional_output_type_is_rejected_without_partial_connection() {
    let mut graph = GraphBuilder::new("type-reject");
    let choice = graph.node("choice", |x: i64| x).unwrap();
    let sink = graph.node("sink", |x: String| x).unwrap();
    assert!(graph.connect(choice.output(), sink.inlet()).is_err());
    let input = graph.expose_input("request", choice.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("run", vec![Value::local(1_i64)])
        .unwrap()
        .expect_success();
}

#[test]
fn patch_replacement_uses_same_validated_graph_operation() {
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("base");
    base.node("inc", |x: i64| x + 1)
        .expose_input("request", "inc")
        .expose_output("result", "inc")
        .trigger("run", &["request"]);
    let mut replace = PatchBuilder::new("replace");
    replace
        .after("base")
        .replace("inc", NodeTemplate::local_function(|x: i64| x * 4));
    planner.add_patch(replace).unwrap();
    planner.add_patch(base).unwrap();
    let (built, resolution) = planner.build("replacement").unwrap();
    assert_eq!(resolution.selected, ["base", "replace"]);
    let fixture = Fixture::new(built.graph).unwrap();
    let result = fixture.output("inc", "value");
    fixture
        .fire("run", vec![Value::local(5_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(result, &20_i64);
}

#[test]
fn result_payloads_are_retained_and_subsequent_waves_use_the_real_executor() {
    let mut graph = GraphBuilder::new("errors");
    let check = graph
        .node(
            "check",
            Node::new(|x: i64| Output::all(if x < 0 { Err("negative") } else { Ok(x + 1) })),
        )
        .unwrap();
    let input = graph.expose_input("request", check.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("run", vec![Value::local(-1_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(
            fixture.output("check", "value"),
            &Err::<i64, &str>("negative"),
        );
    fixture
        .fire("run", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(fixture.output("check", "value"), &Ok::<i64, &str>(5_i64));
}

fn module(name: &str, factor: i64) -> phenix_graph::ModulePackage {
    let mut graph = GraphBuilder::new(name);
    let node = graph.node("worker", move |x: i64| x * factor).unwrap();
    graph.expose_input("request", node.inlet()).unwrap();
    graph.expose_output("answer", node.output()).unwrap();
    graph.module()
}

#[test]
fn module_replacement_is_the_same_patch_verb_with_instance_provenance() {
    let mut base = PatchBuilder::new("base");
    base.instance("math", module("multiply_two", 2))
        .expose_input("request", "math")
        .expose_output("answer", "math")
        .trigger("run", &["request"]);
    let mut override_patch = PatchBuilder::new("override");
    override_patch
        .after("base")
        .replace("math", module("multiply_three", 3));
    let mut planner = PatchPlanner::new();
    planner.add_patch(override_patch).unwrap();
    planner.add_patch(base).unwrap();
    let (assembled, plan) = planner.build("modules").unwrap();
    assert_eq!(plan.selected, ["base", "override"]);
    let fixture = Fixture::new(assembled.graph).unwrap();
    fixture
        .fire("run", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(fixture.output("math/worker", "value"), &12_i64);
}

#[test]
fn publication_count_assertion_does_not_require_payload_retention() {
    use phenix_graph::{Deployment, ReportRetention};
    let graph = increment_graph(1, false);
    let fixture = Fixture::from_deployment(
        Deployment::new(graph).report_retention(ReportRetention::MetadataOnly),
    )
    .unwrap();
    let source = fixture.output("inc", "value");
    let report = fixture.fire("run", vec![Value::local(5_i64)]).unwrap();
    report.expect_success().expect_publication_count(source, 1);
    assert_eq!(report.publications().count(), 0);
}

#[test]
fn duplicate_conditional_output_names_fail_compilation() {
    let mut graph = GraphBuilder::new("invalid-choice");
    // Duplicate output names are illegal even through the low-level node
    // declaration API; there is no special conditional node kind.
    use phenix_graph::{InputSlot, OutputSlot, Port};
    let choice = graph
        .add(
            "choice",
            NodeTemplate::with_action(
                0,
                vec![Port::all(0, vec![InputSlot::local::<i64>(0).named("value")]).named("run")],
                vec![
                    OutputSlot::local::<i64>(0).named("same"),
                    OutputSlot::local::<i64>(1).named("same"),
                ],
                |_ctx| Ok(vec![phenix_graph::emit_local(0, 0_i64)]),
            ),
        )
        .unwrap();
    let choice_input = graph
        .local_input::<i64>(choice.name(), "run", "value")
        .unwrap();
    let input = graph.expose_input("request", choice_input).unwrap();
    graph.trigger("run", [input]).unwrap();
    assert!(
        graph.compile().is_err(),
        "duplicate output names must never alias"
    );
}

#[cfg(feature = "derive")]
#[test]
fn patch_macro_uses_same_replacement_api_for_explicit_templates() {
    use phenix_graph::patch;
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(
            patch! {
                id: "base";
                node inc = |x: i64| x + 1;
                input "request" = inc;
                output "result" = inc;
                trigger "run" = ["request"];
            }
            .unwrap(),
        )
        .unwrap();
    planner
        .add_patch(
            patch! {
                id: "override";
                after: ["base"];
                replace inc = NodeTemplate::local_function(|x: i64| x - 2);
            }
            .unwrap(),
        )
        .unwrap();
    let (assembled, _) = planner.build("macro-explicit").unwrap();
    let fixture = Fixture::new(assembled.graph).unwrap();
    fixture
        .fire("run", vec![Value::local(12_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(fixture.output("inc", "value"), &10_i64);
}
