//! A production dispatcher adapter should replace the mock here, not create
//! another graph executor. The graph, compiler and patch planner are identical
//! for native-operation and local-function nodes.
use phenix_graph::{
    BufferMode, ContractValue, Deployment, InputSlot, NodeTemplate, OutputSlot, PatchBuilder,
    PatchPlanner, Port, ReportRetention, Store, Value, emit,
};
#[path = "support/selected_operation.rs"]
mod selected_operation;
use selected_operation::{SelectedOperation, SelectedOperationDispatcher};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MockDispatcher {
    calls: Mutex<Vec<String>>,
}
impl SelectedOperationDispatcher for MockDispatcher {
    fn invoke(&self, selected: &str, operation: &str, request: Value) -> Result<Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{selected}/{operation}"));
        let x = request.get::<i64>()?;
        let y = match (selected, operation) {
            ("selected.b", "process") => x * 2,
            ("selected.c", "process") => x * 3,
            ("selected.fail", "invalid_schema") => return Ok(Value::of("bad".to_string())),
            _ => return Err("unknown selected operation".into()),
        };
        Ok(Value::of(y))
    }
}

fn join() -> NodeTemplate {
    let input = Port::all(
        0,
        vec![
            InputSlot::new(0, i64::wire_schema(), BufferMode::Deferred).named("left"),
            InputSlot::new(1, i64::wire_schema(), BufferMode::Deferred).named("right"),
        ],
    )
    .named("run");
    NodeTemplate::new(
        0,
        vec![input],
        OutputSlot::typed::<i64>(0).named("value"),
        |ctx| Ok(vec![emit(0, ctx.arg::<i64>(0)? + ctx.arg::<i64>(1)?)]),
    )
}

fn plan(mock: Arc<MockDispatcher>, reversed: bool) -> PatchPlanner {
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("base");
    base.add("a", NodeTemplate::function(|x: i64| x + 1))
        .add("d", join())
        .expose_input("request", "a")
        .expose_output("result", "d")
        .trigger("run", &["request"]);
    let mut b = PatchBuilder::new("branch.b");
    b.after("base").add(
        "b",
        SelectedOperation::new("selected.b", "process", mock.clone())
            .unwrap()
            .into_node::<i64, i64>(),
    );
    b.connect("a", "b").unwrap();
    b.connect("b", "d.in.run.left").unwrap();
    let mut c = PatchBuilder::new("branch.c");
    c.after("base").add(
        "c",
        SelectedOperation::new("selected.c", "process", mock)
            .unwrap()
            .into_node::<i64, i64>(),
    );
    c.connect("a", "c").unwrap();
    c.connect("c", "d.in.run.right").unwrap();
    if reversed {
        planner.add_patch(c).unwrap();
        planner.add_patch(b).unwrap();
    } else {
        planner.add_patch(b).unwrap();
        planner.add_patch(c).unwrap();
    }
    planner.add_patch(base).unwrap();
    planner
}

#[test]
fn independently_selected_operations_form_one_compiled_diamond() {
    for reversed in [false, true] {
        let mock = Arc::new(MockDispatcher::default());
        let (assembled, decisions) = plan(mock.clone(), reversed)
            .build("selected-diamond")
            .unwrap();
        assert_eq!(decisions.selected, ["base", "branch.b", "branch.c"]);
        let engine = Deployment::new(assembled.graph)
            .report_retention(ReportRetention::TerminalOutputs)
            .build()
            .unwrap();
        let report = engine.fire(1, vec![Value::of(4_i64)]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        // A(4)=5; selected B=10; selected C=15; D=25
        assert_eq!(
            report
                .publications()
                .last()
                .unwrap()
                .value::<i64>()
                .unwrap(),
            25
        );
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.contains(&"selected.b/process".into()));
        assert!(calls.contains(&"selected.c/process".into()));
    }
}

#[test]
fn binding_must_be_explicit_and_returns_are_schema_checked() {
    let mock = Arc::new(MockDispatcher::default());
    assert!(SelectedOperation::new("", "process", mock.clone()).is_err());
    assert!(SelectedOperation::new("selected.b", "", mock.clone()).is_err());
    let action = SelectedOperation::new("selected.fail", "invalid_schema", mock).unwrap();
    // A mismatched response codec is detected at the adapter boundary, not
    // silently handed to a downstream graph slot.
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("base");
    base.add("native", action.into_node::<i64, i64>())
        .expose_input("request", "native")
        .expose_output("answer", "native")
        .trigger("run", &["request"]);
    planner.add_patch(base).unwrap();
    let (built, _) = planner.build("bad-binding").unwrap();
    let report = Deployment::new(built.graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(1_i64)])
        .unwrap();
    assert!(!report.success());
    assert!(
        report
            .error_messages()
            .iter()
            .any(|s| s.contains("NodeFailed"))
    );
}

#[test]
fn later_contribution_rebinds_operation_without_rewiring_graph() {
    let mock = Arc::new(MockDispatcher::default());
    let mut planner = PatchPlanner::new();
    let mut base = PatchBuilder::new("base");
    base.add("first", NodeTemplate::function(|x: i64| x + 1))
        .add(
            "operation",
            SelectedOperation::new("selected.b", "process", mock.clone())
                .unwrap()
                .into_node::<i64, i64>(),
        )
        .expose_input("request", "first")
        .expose_output("result", "operation")
        .trigger("run", &["request"]);
    base.connect("first", "operation").unwrap();
    let mut replacement = PatchBuilder::new("override");
    replacement.after("base").replace(
        "operation",
        SelectedOperation::new("selected.c", "process", mock.clone())
            .unwrap()
            .into_node::<i64, i64>(),
    );
    planner.add_patch(replacement).unwrap();
    planner.add_patch(base).unwrap();

    let (assembled, resolution) = planner.build("replacement").unwrap();
    assert_eq!(resolution.selected, ["base", "override"]);
    let result = Deployment::new(assembled.graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(4_i64)])
        .unwrap();
    assert!(result.success(), "{:?}", result.error_messages());
    assert_eq!(
        result
            .publications()
            .last()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        15
    );
    assert_eq!(
        *mock.calls.lock().unwrap(),
        ["selected.c/process".to_string()]
    );
}
