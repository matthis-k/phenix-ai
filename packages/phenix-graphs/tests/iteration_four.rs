//! Iteration 4: observable graph structure and strict output assertions.
//! These are ordinary integration tests of the production compiler and engine.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{GraphBuilder, InputSlot, NodeTemplate, OutputSlot, Port, Value};

fn manifest(public_input: &str, public_output: &str, any: bool) -> String {
    let mut graph = GraphBuilder::new("manifest");
    let slot = InputSlot::local::<i64>(0).named("value");
    let port = if any {
        Port::any(0, vec![slot])
    } else {
        Port::all(0, vec![slot])
    }
    .named("run");
    graph
        .add(
            "worker",
            NodeTemplate::with_action(
                0,
                vec![port],
                vec![OutputSlot::local::<i64>(0).named("value")],
                |_ctx| Ok(vec![phenix_graph::emit_local(0, 0_i64)]),
            ),
        )
        .unwrap();
    let input = graph.expose_input(public_input, "worker").unwrap();
    graph.expose_output(public_output, "worker").unwrap();
    graph.trigger("go", [input]).unwrap();
    graph.compile().unwrap().topology_manifest()
}

#[test]
fn public_module_names_are_part_of_the_structural_descriptor() {
    let baseline = manifest("request", "result", false);
    let changed_input = manifest("query", "result", false);
    let changed_output = manifest("request", "reply", false);
    assert_ne!(baseline, changed_input);
    assert_ne!(baseline, changed_output);
    assert!(baseline.contains("public input \"request\""));
    assert!(baseline.contains("public output \"result\""));
    assert_eq!(baseline, manifest("request", "result", false));
}

#[test]
fn input_readiness_is_visible_without_claiming_policy_behavior_identity() {
    let all = manifest("request", "result", false);
    let any = manifest("request", "result", true);
    assert_ne!(
        all, any,
        "distinct port policy types must not disappear from inspection"
    );
    assert!(all.contains("AllSameCohort"));
    assert!(any.contains("AnyOne"));
    // Both describe structure, not the meaning of arbitrary policy state.
}

#[test]
fn named_output_assertion_accepts_one_real_publication() {
    let mut graph = GraphBuilder::new("strict-output");
    let echo = graph.node("echo", |v: i64| v + 1).unwrap();
    let root = graph.expose_input("request", echo.inlet()).unwrap();
    graph.trigger("run", [root]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let from = fixture.output("echo", "value");
    fixture
        .fire("run", vec![Value::local(9_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(from, 1)
        .expect_local_at(from, &10_i64);
}

#[test]
#[should_panic(expected = "expected exactly one retained publication")]
fn named_output_assertion_does_not_silently_accept_multiple_publications() {
    let mut graph = GraphBuilder::new("multiple-publications");
    let echo = graph.node("echo", |v: i64| v).unwrap();
    // Two distinct exported inputs target the same DEFERRED node slot.
    // Both are admitted in the same wave and must be executed separately.
    let first = graph.expose_input("first", echo.inlet()).unwrap();
    let second = graph.expose_input("second", echo.inlet()).unwrap();
    graph.trigger("run", [first, second]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let from = fixture.output("echo", "value");
    let report = fixture
        .fire("run", vec![Value::local(8_i64), Value::local(8_i64)])
        .unwrap();
    report.expect_success().expect_publication_count(from, 2);
    // This assertion used to inspect .last() and silently report success.
    report.expect_local_at(from, &8_i64);
}

fn unnamed_slot_manifest(target_slot: u16) -> String {
    use phenix_graph::{
        ContractValue, ModuleCatalog, ModuleDefBuilder, ModuleInputTarget, ModuleTriggerTarget,
        input,
    };
    let port = Port::all(
        0,
        vec![
            InputSlot::immediate::<i64>(0),
            InputSlot::immediate::<i64>(1),
        ],
    );
    let node = NodeTemplate::with_action(1, vec![port], vec![OutputSlot::typed::<()>(0)], |_ctx| {
        Ok(vec![phenix_graph::emit(0, ())])
    });
    let mut module = ModuleDefBuilder::new("unnamed-slots");
    module.add(node).unwrap();
    module.input(
        0,
        "request",
        i64::wire_schema(),
        vec![ModuleInputTarget::Node(input(1, 0, target_slot))],
    );
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    catalog
        .compile("unnamed-slots")
        .unwrap()
        .topology_manifest()
}

#[test]
fn unnamed_physical_slots_have_distinct_manifest_addresses() {
    let first = unnamed_slot_manifest(0);
    let second = unnamed_slot_manifest(1);
    assert_ne!(
        first, second,
        "unnamed slots must not collapse into a single synthetic #unnamed label"
    );
    assert!(
        first.contains("/#0"),
        "first endpoint was not identified: {first}"
    );
    assert!(
        second.contains("/#1"),
        "second endpoint was not identified: {second}"
    );
}

fn nested_manifest(child_input_name: &str) -> String {
    let mut child = GraphBuilder::new("child-definition");
    let worker = child.node("worker", |v: i64| v + 2).unwrap();
    child
        .expose_input(child_input_name, worker.inlet())
        .unwrap();
    child.expose_output("answer", worker.output()).unwrap();
    let module = child.module();

    let mut parent = GraphBuilder::new("parent");
    let instance = parent.instance("child", &module).unwrap();
    let input = parent
        .expose_input(
            "request",
            instance.local_input::<i64>(child_input_name).unwrap(),
        )
        .unwrap();
    parent
        .expose_output("response", instance.local_output::<i64>("answer").unwrap())
        .unwrap();
    parent.trigger("run", [input]).unwrap();
    parent.compile().unwrap().topology_manifest()
}

#[test]
fn nested_module_interface_names_are_preserved_after_flattening() {
    let before = nested_manifest("argument");
    let after = nested_manifest("payload");
    assert_ne!(before, after);
    assert!(before.contains("module \"child\""));
    assert!(before.contains("module input \"argument\""));
    assert!(after.contains("module input \"payload\""));
}

#[test]
fn documented_patch_replacement_runs_through_the_only_graph_executor() {
    use phenix_graph::{PatchBuilder, PatchPlanner};
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("agent.base");
    base.node("accept", |v: i64| v + 1)
        .node("output", |v: i64| v * 2)
        .expose_input("request", "accept")
        .expose_output("response", "output")
        .trigger("submit", &["request"]);
    base.connect("accept", "output").unwrap();
    planner.add_patch(base).unwrap();

    let mut extension = PatchBuilder::new("plugin.contribution");
    extension
        .after("agent.base")
        .replace("output", NodeTemplate::local_function(|v: i64| v * 3));
    planner.add_patch(extension).unwrap();
    let (compiled, _) = planner.build("agent").unwrap();
    let fixture = Fixture::new(compiled.graph).unwrap();
    fixture
        .fire("submit", vec![Value::local(4_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(fixture.output("output", "value"), &15_i64);
}
