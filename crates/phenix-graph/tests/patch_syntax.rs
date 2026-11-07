//! Macro and macro-free syntax share PatchBuilder, symbolic addressing,
//! PatchPlanner conflict resolution and the ordinary compiler/executor.
use phenix_graph::Output;
use phenix_graph::Node;
use phenix_graph::{Deployment, GraphAssembly, GraphBuilder, Layout, PatchBuilder,
    PatchError, PatchPlanner, PatchSpec, PatchTarget, ReportRetention, Store, Value};

#[cfg(feature = "derive")]
use phenix_graph::{LocalPortLayout, patch};

#[derive(Clone)]
#[cfg_attr(feature = "derive", derive(LocalPortLayout))]
struct Pair { left: i64, right: i64 }

fn run(planner: PatchPlanner, x: i64) -> i64 {
    let (built, _) = planner.build("diamond").unwrap();
    let report = Deployment::new(built.graph).report_retention(ReportRetention::TerminalOutputs).build().unwrap()
        .fire(1, vec![Value::local(x)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    *report.publications().last().expect("diamond result").borrow::<i64>().unwrap()
}

#[cfg(feature = "derive")]
fn manual_diamond() -> PatchPlanner {
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("base");
    base.node("a", |x: i64| x + 1)
        .node("d", Node::with_layout(|p: Pair| p.left + p.right, Layout::<Pair>::fields()))
        .expose_input("request", "a")
        .expose_output("result", "d")
        .trigger("run", &["request"]);
    planner.add_patch(base).unwrap();
    let mut b = PatchBuilder::new("branch.b");
    b.after("base").node("b", |x: i64| x * 2);
    b.connect("a", "b").unwrap();
    b.connect("b", "d.in.run.left").unwrap();
    planner.add_patch(b).unwrap();
    let mut c = PatchBuilder::new("branch.c");
    c.after("base").node("c", |x: i64| x * 3);
    c.connect("a", "c").unwrap();
    c.connect("c", "d.in.run.right").unwrap();
    planner.add_patch(c).unwrap();
    planner
}

#[cfg(feature = "derive")]
#[test]
fn macro_free_three_patch_diamond_executes() {
    assert_eq!(run(manual_diamond(), 4), 25);
}

#[cfg(feature = "derive")]
#[test]
fn macro_and_manual_diamond_are_semantically_identical() {
    let mut planner = PatchPlanner::new();
    // Reverse registration: only declared dependencies determine order.
    planner.add_patch(patch! {
        id: "branch.c";
        after: ["base"];
        node c = |x: i64| x * 3;
        a -> c -> d.in.run.right;
    }.unwrap()).unwrap();
    planner.add_patch(patch! {
        id: "branch.b";
        after: ["base"];
        node b = |x: i64| x * 2;
        a.out.value -> b -> d.in.run.left;
    }.unwrap()).unwrap();
    planner.add_patch(patch! {
        id: "base";
        node a = |x: i64| x + 1;
        node d = |p: Pair| p.left + p.right, layout = Layout::<Pair>::fields();
        input "request" = a;
        output "result" = d;
        trigger "run" = ["request"];
    }.unwrap()).unwrap();
    let resolution = planner.resolve().unwrap();
    assert_eq!(resolution.selected, ["base", "branch.b", "branch.c"]);
    assert_eq!(run(planner, 4), run(manual_diamond(), 4));
}

#[cfg(feature = "derive")]
#[test]
fn default_endpoints_are_exact_and_reject_ambiguity() {
    let mut graph = GraphBuilder::new("ambiguous");
    let source = graph.node("source", |x: i64| x).unwrap();
    graph.node("join", Node::with_layout(|p: Pair| p.left + p.right, Layout::<Pair>::fields())).unwrap();
    assert!(graph.connect("source", "join").err().unwrap().contains("ambiguous"));
    graph.connect("source", "join.in.run.left").unwrap();
    assert!(graph.connect("source.out.value", "join.in.run.left").err().unwrap().contains("duplicate"));
    graph.connect("source", "join.in.run.right").unwrap();
    let trigger = graph.expose_input("request", "source").unwrap();
    graph.expose_output("reply", "join").unwrap();
    graph.trigger("run", [trigger]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::local(5_i64)]).unwrap();
    assert!(report.success());
    assert_eq!(*report.publications().last().unwrap().borrow::<i64>().unwrap(), 10);
    let _ = source;
}

#[test]
fn conditional_routing_keeps_one_unambiguous_output() {
    let mut g = GraphBuilder::new("branch");
    let sink = g.node("sink", |x: i64| x).unwrap();
    let target = sink.inlet();
    g.node("router", move |x: i64| {
        Output::to(&target, x)
    }).unwrap();
    // All routing decisions affect edges, not the node's output port shape.
    g.connect("router", "sink").unwrap();
    assert!(g.connect("router.out.left", "sink").is_err());
}

#[test]
fn symbolic_connections_are_type_checked_even_without_generics() {
    let mut g = GraphBuilder::new("type-check");
    g.node("number", |x: i64| x).unwrap();
    g.node("text", |x: String| x).unwrap();
    assert!(g.connect("number", "text").err().unwrap().contains("type mismatch"));
    assert!(PatchTarget::connection_pattern("number", "text.in.run.value").is_ok());
    assert!(PatchTarget::connection_pattern("number.foo", "text").is_err());
}

#[test]
fn implicit_and_explicit_addresses_conflict_before_application() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(PatchBuilder::custom(PatchSpec::new("one")
        .writes(PatchTarget::connection_pattern("a", "b").unwrap()), |_g| Ok(()))).unwrap();
    planner.add_patch(PatchBuilder::custom(PatchSpec::new("two")
        .writes(PatchTarget::edge_nodes("a", "value", "b", "run", "value")), |_g| Ok(()))).unwrap();
    assert!(matches!(planner.resolve(), Err(PatchError::Conflict { .. })));
}

#[test]
fn separate_branches_do_not_conflict_even_with_shorthand() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(PatchBuilder::custom(PatchSpec::new("one")
        .writes(PatchTarget::connection_pattern("a", "b").unwrap()), |_g| Ok(()))).unwrap();
    planner.add_patch(PatchBuilder::custom(PatchSpec::new("two")
        .writes(PatchTarget::connection_pattern("a", "c").unwrap()), |_g| Ok(()))).unwrap();
    assert!(planner.resolve().is_ok());
}

#[cfg(feature = "derive")]
#[test]
fn macro_patch_exposes_parser_errors_and_no_partial_mutations() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(patch! {
        id: "bad";
        node a = |x: i64| x;
        a.out.value -> missing;
    }.unwrap()).unwrap();
    let mut assembly = GraphAssembly::new("failed");
    assert!(matches!(planner.apply(&mut assembly), Err(PatchError::Apply { .. })));
    assert!(assembly.revisions().is_empty());
}

#[cfg(feature = "derive")]
#[test]
fn macro_disconnection_is_checked_like_manual_disconnect() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(patch! {
        id: "base";
        node a = |x: i64| x;
        node b = |x: i64| x + 1;
        a -> b;
        input "request" = a;
        output "result" = b;
        trigger "run" = ["request"];
    }.unwrap()).unwrap();
    planner.add_patch(patch! {
        id: "remove";
        after: ["base"];
        disconnect a.out.value -> b.in.run.value;
        a -> b;
    }.unwrap()).unwrap();
    assert_eq!(run(planner, 9), 10);
}

#[test]
fn length_prefixed_edge_keys_are_unambiguous_even_with_arrow_in_node_name() {
    let exact = PatchTarget::edge_nodes("a->b", "value", "sink", "run", "value");
    let pattern = PatchTarget::connection_pattern("a->b", "sink.in.run.value").unwrap();
    let mut planner = PatchPlanner::new();
    planner.add_patch(PatchBuilder::custom(PatchSpec::new("one").writes(exact), |_g| Ok(()))).unwrap();
    planner.add_patch(PatchBuilder::custom(PatchSpec::new("two").writes(pattern), |_g| Ok(()))).unwrap();
    assert!(matches!(planner.resolve(), Err(PatchError::Conflict { .. })));
}

#[test]
fn instance_public_port_default_is_supported_but_private_node_is_not() {
    let mut inner = GraphBuilder::new("child-definition");
    let worker = inner.node("worker", |x: i64| x * 4).unwrap();
    inner.expose_input("input", worker.inlet()).unwrap();
    inner.expose_output("output", worker.output()).unwrap();
    let inner = inner.module();

    let mut graph = GraphBuilder::new("container");
    graph.node("accept", |x: i64| x + 1).unwrap();
    graph.instance("child", &inner).unwrap();
    graph.node("finish", |x: i64| x + 2).unwrap();
    graph.connect("accept", "child").unwrap();
    graph.connect("child.out.output", "finish").unwrap();
    assert!(graph.connect("child/worker", "finish").is_err());
    let input = graph.expose_input("request", "accept").unwrap();
    graph.expose_output("result", "finish").unwrap();
    graph.trigger("run", [input]).unwrap();
    let result = Deployment::new(graph.compile().unwrap()).report_retention(ReportRetention::TerminalOutputs).build().unwrap()
        .fire(1, vec![Value::local(2_i64)]).unwrap();
    assert!(result.success(), "{:?}", result.error_messages());
    assert_eq!(*result.publications().last().unwrap().borrow::<i64>().unwrap(), 14);
}

#[cfg(feature = "derive")]
#[test]
fn macro_dependent_replacement_preserves_external_wiring() {
    let mut planner = PatchPlanner::new();
    planner.add_patch(patch! {
        id: "base";
        node a = |x: i64| x + 1;
        input "request" = a;
        output "result" = a;
        trigger "run" = ["request"];
    }.unwrap()).unwrap();
    planner.add_patch(patch! {
        id: "override";
        requires: ["base"];
        replace a = |x: i64| x * 5;
    }.unwrap()).unwrap();
    assert_eq!(run(planner, 3), 15);
}
