#![cfg(feature = "derive")]

use phenix_graph::{
    Deployment, GraphAssembly, Layout, LocalPortLayout, PatchBuilder, PatchPlanner, PatchSpec,
    PatchTarget, ReportRetention, Value,
};

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct Pair {
    left: i64,
    right: i64,
}

fn base() -> GraphAssembly {
    let mut planner = PatchPlanner::new();
    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("base").writes_all([
                PatchTarget::Node("source".into()),
                PatchTarget::Node("sum".into()),
            ]),
            |graph| {
                graph
                    .node("source", |x: i64| x * 2)
                    .map_err(|e| e.to_string())?;
                graph
                    .node("sum", |x: Pair| x.left + x.right)
                    .map_err(|e| e.to_string())?
                    .input(Layout::fields())
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        ))
        .unwrap();
    let mut assembly = GraphAssembly::new("symbolic-pending");
    planner.apply(&mut assembly).unwrap();
    assembly
}

fn wire(assembly: &mut GraphAssembly) {
    let mut patch = PatchBuilder::new("wire");
    patch.connect("source", "sum.in.run.left").unwrap();
    patch.expose_input("request", "source");
    patch.expose_input("other", "sum.in.run.right");
    patch.expose_output("result", "sum");
    patch.trigger("run", &["request", "other"]);
    let mut planner = PatchPlanner::new();
    planner.add_patch(patch).unwrap();
    planner.apply(assembly).unwrap();
}

fn execute(assembly: GraphAssembly) {
    let report = Deployment::new(assembly.finish().unwrap().graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(20_i64), Value::local(2_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&42)
    );
}

#[test]
fn symbolic_fields_connect_and_expose_the_pending_local_identity() {
    let mut assembly = base();
    wire(&mut assembly);
    execute(assembly);
}

#[test]
fn stale_atomic_and_ambiguous_default_paths_fail_without_changing_the_graph() {
    for path in ["sum.in.run.value", "sum", "sum.in.run"] {
        for connect in [false, true] {
            let mut assembly = base();
            let revisions = assembly.revisions().to_vec();
            let mut patch = PatchBuilder::new("wire");
            // This successful edit must also roll back when the stale path fails.
            patch.expose_output("result", "source");
            if connect {
                patch.connect("source", path).unwrap();
            } else {
                patch.expose_input("stale", path);
            }
            let mut planner = PatchPlanner::new();
            planner.add_patch(patch).unwrap();
            assert!(planner.apply(&mut assembly).is_err(), "{path}");
            assert_eq!(assembly.revisions(), revisions);
            // Reusing the failed contributor name and executing proves the
            // pending layout and existing graph survived the failed patch.
            wire(&mut assembly);
            execute(assembly);
        }
    }
}
