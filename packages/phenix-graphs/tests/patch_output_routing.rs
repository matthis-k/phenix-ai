//! Output routing is a property of the producing action's return value.
//! Patches change the action with the same `replace` operation used for any
//! other node behavior; there is no second output-policy patch vocabulary.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{
    Deployment, GraphAssembly, NodeTemplate, Output, PatchBuilder, PatchError, PatchPlanner,
    PatchSpec, ReportRetention, Value,
};

fn base() -> PatchBuilder {
    let mut base = PatchBuilder::new("base");
    base.node("source", |x: i64| x + 1)
        .node("left", |x: i64| x * 2)
        .node("right", |x: i64| x * 3)
        .expose_input("request", "source")
        .expose_output("left_result", "left")
        .expose_output("right_result", "right")
        .trigger("go", &["request"]);
    base.connect("source.out.value", "left.in.run.value")
        .unwrap();
    base.connect("source.out.value", "right.in.run.value")
        .unwrap();
    base
}

#[test]
fn patch_replaces_action_with_per_activation_routing() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(base()).unwrap();
    let mut selection = PatchBuilder::new("route");
    selection.after("base");
    selection.replace(
        "source",
        NodeTemplate::local_function(|x: i64| Output::to_prefix("left", x + 1)),
    );
    planner.add_patch(selection).unwrap();
    let (assembled, resolution) = planner.build("selected").unwrap();
    assert_eq!(resolution.selected, ["base", "route"]);
    let replacements: Vec<_> = assembled
        .revisions
        .iter()
        .flat_map(|revision| &revision.changes)
        .filter(|change| {
            matches!(change,
            phenix_graph::AssemblyChange::NodeReplaced(node) if node == "source")
        })
        .collect();
    assert_eq!(replacements.len(), 1);
    let fixture = Fixture::new(assembled.graph).unwrap();
    let source = fixture.output("source", "value");
    let left = fixture.input("left", "run", "value");
    fixture
        .fire("go", vec![Value::local(3_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(source, &[left])
        .expect_invocations(fixture.node_id("left"), 1)
        .expect_invocations(fixture.node_id("right"), 0);
}

#[test]
fn independent_action_replacements_conflict_before_apply() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(base()).unwrap();
    for id in ["first", "second"] {
        let mut patch = PatchBuilder::new(id);
        patch.after("base");
        patch.replace(
            "source",
            NodeTemplate::local_function(|x: i64| Output::to_all([], x + 1)),
        );
        planner.add_patch(patch).unwrap();
    }
    assert!(matches!(
        planner.resolve(),
        Err(PatchError::Conflict { .. })
    ));
}

#[test]
fn undeclared_action_replacement_fails_atomically() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(base()).unwrap();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("unclaimed").after("base"),
            |graph| {
                graph.replace(
                    "source",
                    NodeTemplate::local_function(|x: i64| Output::to_all([], x + 1)),
                )
            },
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("atomic");
    assert!(matches!(
        planner.apply(&mut assembly),
        Err(PatchError::UndeclaredWrite { .. })
    ));
    assert!(
        assembly.revisions().is_empty(),
        "failed plan must not commit edits"
    );
}

#[test]
fn later_action_replacement_replaces_routing_behavior_too() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(base()).unwrap();
    let mut route = PatchBuilder::new("route");
    route.after("base");
    route.replace(
        "source",
        NodeTemplate::local_function(|x: i64| Output::to_all([], x + 1)),
    );
    planner.add_patch(route).unwrap();
    let mut replace = PatchBuilder::new("replace");
    replace.after("route");
    replace.replace("source", NodeTemplate::local_function(|x: i64| x + 10));
    planner.add_patch(replace).unwrap();
    let (assembled, _) = planner.build("replaced").unwrap();
    let report = Deployment::new(assembled.graph)
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("go", vec![Value::local(3_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().count(),
        3,
        "replacement restores ordinary broadcast"
    );
    assert_eq!(report.delivery_events().len(), 2);
}

#[test]
fn patch_selected_target_never_creates_an_unconnected_edge() {
    let mut planner = PatchPlanner::new();
    let mut base = base();
    base.node("unconnected", |x: i64| x);
    planner.add_patch(base).unwrap();
    let mut replace = PatchBuilder::new("route");
    replace.after("base");
    replace.replace(
        "source",
        NodeTemplate::local_function(|x: i64| Output::to_prefix("unconnected", x + 1)),
    );
    planner.add_patch(replace).unwrap();
    let (assembled, _) = planner.build("not-forged").unwrap();
    let fixture = Fixture::new(assembled.graph).unwrap();
    fixture
        .fire("go", vec![Value::local(3_i64)])
        .unwrap()
        .expect_success()
        .expect_publication_count(fixture.output("source", "value"), 1)
        .expect_deliveries_from(fixture.output("source", "value"), &[])
        .expect_invocations(fixture.node_id("unconnected"), 0);
}
