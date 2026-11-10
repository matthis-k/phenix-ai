//! New ergonomic facade and backend boundary tests. Unlike the original tests,
//! these use the single GraphBuilder declaration API.
use phenix_graph::Output;
use phenix_graph::ReportRetention;
use phenix_graph::{
    Deployment, EdgeAdapter, Emission, Executor, GraphBuilder, InvocationCtx, Layout, NodeAction,
    Store, Value,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn square(x: i32) -> i32 {
    x * x
}
fn increment(x: i32) -> i32 {
    x + 1
}

#[test]
fn inferred_graph_builder_preserves_typed_links() {
    let mut g = GraphBuilder::new("inferred");
    let a = g.node("square", square).unwrap();
    let b = g.node("increment", increment).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    let request = g.expose_input("request", a.inlet()).unwrap();
    g.expose_output("result", b.output()).unwrap();
    let trigger = g.trigger("run", [request]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(4_i32)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .last()
            .unwrap()
            .borrow::<i32>()
            .copied()
            .unwrap(),
        17
    );
}

#[cfg(feature = "derive")]
#[test]
fn one_layout_type_and_flattens_to_original_domain_arg() {
    use phenix_graph::LocalPortLayout;
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct AddArgs {
        left: i32,
        right: i32,
    }
    let mut g = GraphBuilder::new("flatten");
    let plus = g
        .node("plus", |args: AddArgs| args.left + args.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    let a = g
        .expose_input(
            "a",
            plus.inlet().field(AddArgs::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    let b = g
        .expose_input(
            "b",
            plus.inlet().field(AddArgs::SLOT_RIGHT).target().unwrap(),
        )
        .unwrap();
    g.expose_output("sum", plus.output()).unwrap();
    let trigger = g.trigger("add", [a, b]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![Value::local(5_i32), Value::local(8_i32)],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i32>(),
        Some(&13)
    );
}

#[derive(Debug)]
struct NonClone {
    value: usize,
}

#[test]
fn local_values_need_no_codec_or_clone_and_fanout_shares_allocation() {
    let seen = Arc::new(AtomicUsize::new(0));
    let mut g = GraphBuilder::new("local-nonclone");
    let source = g
        .node("source", |data: Arc<NonClone>| {
            Arc::new(NonClone {
                value: data.value + 1,
            })
        })
        .unwrap();
    let first_seen = Arc::clone(&seen);
    let first = g
        .node("first", move |value: Arc<NonClone>| {
            first_seen.fetch_add(
                usize::from(Arc::strong_count(&value) >= 2),
                Ordering::SeqCst,
            );
            value.value * 2
        })
        .unwrap();
    let second = g
        .node("second", |value: Arc<NonClone>| value.value * 3)
        .unwrap();
    g.connect(source.output(), first.inlet()).unwrap();
    g.connect(source.output(), second.inlet()).unwrap();
    let ingress = g.expose_input("request", source.inlet()).unwrap();
    let fire = g.trigger("run", [ingress]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(
            fire.get(),
            vec![Value::local(Arc::new(NonClone { value: 4 }))],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    let outputs: Vec<usize> = report
        .publications()
        .filter_map(|publication| publication.borrow::<usize>().copied())
        .collect();
    assert!(
        outputs.contains(&10) && outputs.contains(&15),
        "{outputs:?}"
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}

#[test]
fn module_instances_share_only_public_typed_endpoints() {
    let mut child = GraphBuilder::new("multiply-module");
    let mul = child.node("triple", |value: i64| value * 3).unwrap();
    child.expose_input("number", mul.inlet()).unwrap();
    child.expose_output("result", mul.output()).unwrap();
    let definition = child.module();

    let mut root = GraphBuilder::new("root");
    let instance = root.instance("multiply", &definition).unwrap();
    let plus = root.node("plus", |value: i64| value + 2).unwrap();
    root.connect(
        instance
            .operation()
            .unwrap()
            .output::<i64, phenix_graph::LocalValue>()
            .unwrap(),
        plus.inlet(),
    )
    .unwrap();
    let public = root
        .expose_input(
            "number",
            instance
                .operation()
                .unwrap()
                .inlets::<(), phenix_graph::LocalValue>()
                .port(phenix_graph::InletPort::<(), i64>::new("number"))
                .field(phenix_graph::SlotField::<i64, i64>::new("value"))
                .target()
                .unwrap(),
        )
        .unwrap();
    root.expose_output("out", plus.output()).unwrap();
    let fire = root.trigger("start", [public]).unwrap();
    let graph = root.compile().unwrap();
    assert!(graph.find("multiply/triple").is_some());
    let report = Deployment::new(graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(fire.get(), vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .last()
            .unwrap()
            .borrow::<i64>()
            .copied()
            .unwrap(),
        23
    );
}

#[test]
fn builder_rejects_cross_scope_and_invalid_slot_even_with_matching_type() {
    let mut a = GraphBuilder::new("a");
    let node_a = a.node("a", square).unwrap();
    let mut b = GraphBuilder::new("b");
    let node_b = b.node("b", increment).unwrap();
    assert!(b.connect(node_a.output(), node_b.inlet()).is_err());
    assert!(
        a.discover("a")
            .unwrap()
            .inlets::<(), phenix_graph::EncodedValue>()
            .port(phenix_graph::InletPort::<(), i32>::new("run"))
            .field(phenix_graph::SlotField::<i32, i32>::new("missing"))
            .target()
            .is_err()
    );
    assert!(
        a.discover("a")
            .unwrap()
            .inlets::<(), phenix_graph::EncodedValue>()
            .port(phenix_graph::InletPort::<(), i32>::new("run"))
            .field(phenix_graph::SlotField::<i32, String>::new("value"))
            .target()
            .is_err()
    );
}

struct CountingExecutor(Arc<AtomicUsize>);
impl Executor for CountingExecutor {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        action.execute(ctx)
    }
}

#[test]
fn deferred_execution_session_can_complete_out_of_dispatch_order() {
    let mut g = GraphBuilder::new("async-session");
    let a = g.node("a", |x: i32| x + 1).unwrap();
    let b = g.node("b", |x: i32| x + 2).unwrap();
    let ia = g.expose_input("a", a.inlet()).unwrap();
    let ib = g.expose_input("b", b.inlet()).unwrap();
    let fire = g.trigger("start", [ia, ib]).unwrap();
    let engine = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let mut session = engine
        .begin(fire.get(), vec![Value::local(1_i32), Value::local(2_i32)])
        .unwrap();
    let (ticket_a, ctx_a) = session.next_ready().unwrap().into_parts();
    let (ticket_b, ctx_b) = session.next_ready().unwrap().into_parts();
    assert_eq!(session.in_flight_count(), 2);
    assert!(!session.can_finish());
    let result_a = session.action(ticket_a).unwrap().execute(ctx_a);
    let result_b = session.action(ticket_b).unwrap().execute(ctx_b);
    session.complete(ticket_b, result_b).unwrap();
    assert!(!session.can_finish());
    session.complete(ticket_a, result_a).unwrap();
    assert!(session.complete(ticket_a, Ok(vec![])).is_err());
    assert!(session.can_finish());
    let report = session.finish().unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    let mut outputs: Vec<i32> = report
        .publications()
        .map(|p| *p.borrow::<i32>().unwrap())
        .collect();
    outputs.sort();
    assert_eq!(outputs, vec![2, 4]);
}

struct CountingAdapter(Arc<AtomicUsize>);
impl EdgeAdapter for CountingAdapter {
    fn prepare(&self, value: &Value) -> Result<Value, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(value.clone())
    }
}

#[test]
fn deployment_can_configure_one_edge_and_one_node_independently() {
    let mut g = GraphBuilder::new("deploy");
    let a = g.node("a", square).unwrap();
    let b = g.node("b", increment).unwrap();
    let c = g.node("c", increment).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    g.connect(a.output(), c.inlet()).unwrap();
    let input = g.expose_input("request", a.inlet()).unwrap();
    let fire = g.trigger("run", [input]).unwrap();
    let transfers = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut deployment = Deployment::new(g.compile().unwrap());
    deployment
        .edge_adapter(
            "a",
            "value",
            "b",
            "run",
            "value",
            Arc::new(CountingAdapter(Arc::clone(&transfers))),
        )
        .unwrap();
    deployment
        .node_executor("c", Arc::new(CountingExecutor(Arc::clone(&calls))))
        .unwrap();
    let report = deployment
        .build()
        .unwrap()
        .fire(fire.get(), vec![Value::local(3_i32)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(transfers.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct RejectAdapter;
impl EdgeAdapter for RejectAdapter {
    fn prepare(&self, _: &Value) -> Result<Value, String> {
        Err("adapter unavailable".into())
    }
}

#[test]
fn transport_failure_settles_wave_without_downstream_execution() {
    let mut g = GraphBuilder::new("failed-edge");
    let a = g.node("a", square).unwrap();
    let b = g.node("b", increment).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    let input = g.expose_input("request", a.inlet()).unwrap();
    let fire = g.trigger("run", [input]).unwrap();
    let mut deployment = Deployment::new(g.compile().unwrap());
    deployment
        .edge_adapter("a", "value", "b", "run", "value", Arc::new(RejectAdapter))
        .unwrap();
    let report = deployment
        .build()
        .unwrap()
        .fire(fire.get(), vec![Value::local(3_i32)])
        .unwrap();
    assert!(!report.success());
    assert!(
        report
            .error_messages()
            .iter()
            .any(|e| e.contains("adapter unavailable"))
    );
    assert_eq!(report.publications().count(), 0);
}

#[test]
fn option_and_result_outputs_are_payloads_not_execution_modes() {
    let mut g = GraphBuilder::new("output-modes");
    let option = g
        .node("optional", |x: i32| Output::all((x > 0).then_some(x)))
        .unwrap();
    let fallible = g
        .node("fallible", |x: i32| {
            Output::all(if x > 0 { Ok(x) } else { Err("negative") })
        })
        .unwrap();
    let a = g.expose_input("a", option.inlet()).unwrap();
    let b = g.expose_input("b", fallible.inlet()).unwrap();
    let fire = g.trigger("run", [a, b]).unwrap();
    let graph = g.compile().unwrap();
    let report = Deployment::new(graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(fire.get(), vec![Value::local(-1_i32), Value::local(-2_i32)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().count(), 2);
    assert!(
        report
            .publications()
            .any(|p| p.borrow::<Option<i32>>() == Some(&None))
    );
    assert!(
        report
            .publications()
            .any(|p| p.borrow::<Result<i32, &'static str>>() == Some(&Err("negative")))
    );
}

#[derive(Debug)]
struct CloneProbe {
    payload: usize,
    clones: Arc<AtomicUsize>,
}
impl Clone for CloneProbe {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::SeqCst);
        Self {
            payload: self.payload,
            clones: Arc::clone(&self.clones),
        }
    }
}

#[test]
fn single_consumer_owned_input_moves_without_cloning() {
    let clones = Arc::new(AtomicUsize::new(0));
    let mut g = GraphBuilder::new("owned-fast-path");
    let n = g
        .node("consume", |value: CloneProbe| value.payload + 1)
        .unwrap();
    let input = g.expose_input("value", n.inlet()).unwrap();
    let trigger = g.trigger("go", [input]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![Value::local(CloneProbe {
                payload: 41,
                clones: Arc::clone(&clones),
            })],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<usize>(),
        Some(&42)
    );
    assert_eq!(
        clones.load(Ordering::SeqCst),
        0,
        "single-owner path must move the value"
    );
}

#[test]
fn distinct_modules_with_identical_names_and_shapes_are_not_conflated() {
    let make = |delta: i32| {
        let mut g = GraphBuilder::new("duplicate-definition");
        let n = g.node("step", move |value: i32| value + delta).unwrap();
        g.expose_input("in", n.inlet()).unwrap();
        g.expose_output("out", n.output()).unwrap();
        g.module()
    };
    let first = make(1);
    let second = make(2);
    let mut root = GraphBuilder::new("root-with-duplicates");
    root.instance("one", &first).unwrap();
    root.instance("same-package", &first).unwrap();
    assert!(root.instance("conflict", &second).is_err());
}

#[test]
fn invocation_ticket_cannot_complete_another_wave_or_engine() {
    let mut g = GraphBuilder::new("ticket-isolation");
    let n = g.node("increment", |x: i32| x + 1).unwrap();
    let input = g.expose_input("in", n.inlet()).unwrap();
    let trigger = g.trigger("go", [input]).unwrap();
    let graph = g.compile().unwrap();
    let first_engine = Deployment::new(Arc::clone(&graph))
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let second_engine = Deployment::new(graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let mut first = first_engine
        .begin(trigger.get(), vec![Value::local(1_i32)])
        .unwrap();
    let mut second = first_engine
        .begin(trigger.get(), vec![Value::local(2_i32)])
        .unwrap();
    let mut other_engine = second_engine
        .begin(trigger.get(), vec![Value::local(3_i32)])
        .unwrap();
    let (ticket_a, ctx_a) = first.next_ready().unwrap().into_parts();
    let (ticket_b, ctx_b) = second.next_ready().unwrap().into_parts();
    let (ticket_c, ctx_c) = other_engine.next_ready().unwrap().into_parts();
    assert!(second.action(ticket_a).is_none());
    assert!(other_engine.action(ticket_a).is_none());
    assert!(second.complete(ticket_a, Ok(vec![])).is_err());
    assert!(other_engine.complete(ticket_a, Ok(vec![])).is_err());
    let a = first.action(ticket_a).unwrap().execute(ctx_a);
    let b = second.action(ticket_b).unwrap().execute(ctx_b);
    let c = other_engine.action(ticket_c).unwrap().execute(ctx_c);
    first.complete(ticket_a, a).unwrap();
    second.complete(ticket_b, b).unwrap();
    other_engine.complete(ticket_c, c).unwrap();
    assert!(first.finish().unwrap().success());
    assert!(second.finish().unwrap().success());
    assert!(other_engine.finish().unwrap().success());
}

#[cfg(feature = "derive")]
#[test]
fn structured_node_handle_does_not_require_an_atomic_input() {
    use phenix_graph::LocalPortLayout;
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Args {
        left: i32,
        right: i32,
    }
    let mut g = GraphBuilder::new("structured-handle");
    let add = g
        .node("add", |a: Args| a.left + a.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    assert!(
        add.operation()
            .inlet()
            .field(phenix_graph::SlotField::<Args, i32>::new("value"))
            .target()
            .is_err()
    );
    assert!(
        add.operation()
            .inlet()
            .field(phenix_graph::SlotField::<Args, i32>::new("left"))
            .target()
            .is_ok()
    );
}

struct WrongTypeAdapter;
impl EdgeAdapter for WrongTypeAdapter {
    fn prepare(&self, _: &Value) -> Result<Value, String> {
        Ok(Value::local("wrong".to_string()))
    }
}

#[test]
fn transport_must_preserve_declared_endpoint_type() {
    let mut g = GraphBuilder::new("edge-adapter-type-check");
    let a = g.node("a", |x: i32| x + 1).unwrap();
    let b = g.node("b", |x: i32| x + 1).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    let input = g.expose_input("request", a.inlet()).unwrap();
    let trigger = g.trigger("go", [input]).unwrap();
    let mut deployment = Deployment::new(g.compile().unwrap());
    deployment
        .edge_adapter(
            "a",
            "value",
            "b",
            "run",
            "value",
            Arc::new(WrongTypeAdapter),
        )
        .unwrap();
    let report = deployment
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(1_i32)])
        .unwrap();
    assert!(!report.success());
    assert!(
        report
            .error_messages()
            .iter()
            .any(|error| error.contains("edge adapter changed"))
    );
}

struct PanicAdapter;
impl EdgeAdapter for PanicAdapter {
    fn prepare(&self, _: &Value) -> Result<Value, String> {
        panic!("transport panic")
    }
}

#[test]
fn transport_panic_is_recorded_as_a_settled_failure() {
    let mut g = GraphBuilder::new("edge-adapter-panic");
    let a = g.node("a", |x: i32| x + 1).unwrap();
    let b = g.node("b", |x: i32| x + 1).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    let input = g.expose_input("request", a.inlet()).unwrap();
    let trigger = g.trigger("go", [input]).unwrap();
    let mut deployment = Deployment::new(g.compile().unwrap());
    deployment
        .edge_adapter("a", "value", "b", "run", "value", Arc::new(PanicAdapter))
        .unwrap();
    let report = deployment
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(1_i32)])
        .unwrap();
    assert!(!report.success());
    assert!(
        report
            .error_messages()
            .iter()
            .any(|error| error.contains("edge adapter panicked"))
    );
}

#[cfg(feature = "derive")]
#[test]
fn codec_free_local_nested_flattening_and_generated_fields() {
    use phenix_graph::LocalPortLayout;
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Address {
        city: String,
        zip: u32,
    }
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Request {
        id: u64,
        address: Address,
        #[port(atomic)]
        metadata: String,
    }
    let mut g = GraphBuilder::new("local-nested-layout");
    let n = g
        .node("format", |r: Request| {
            format!(
                "{}:{}:{}:{}",
                r.id, r.address.city, r.address.zip, r.metadata
            )
        })
        .unwrap()
        .input(Layout::flatten_to::<2>())
        .unwrap();
    // Immediate child field keys remain typed; deeper paths can be addressed
    // with the checked manual form until nested descriptor generation arrives.
    let id = g
        .expose_input("id", n.inlet().field(Request::SLOT_ID).target().unwrap())
        .unwrap();
    let city = g
        .expose_input(
            "city",
            n.inlets()
                .port(phenix_graph::InletPort::<_, String>::new("run"))
                .field(phenix_graph::SlotField::<String, String>::new(
                    "address.city",
                ))
                .target()
                .unwrap(),
        )
        .unwrap();
    let zip = g
        .expose_input(
            "zip",
            n.inlets()
                .port(phenix_graph::InletPort::<_, u32>::new("run"))
                .field(phenix_graph::SlotField::<u32, u32>::new("address.zip"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let meta = g
        .expose_input(
            "meta",
            n.inlet().field(Request::SLOT_METADATA).target().unwrap(),
        )
        .unwrap();
    g.expose_output("text", n.output()).unwrap();
    let trigger = g.trigger("run", [id, city, zip, meta]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![
                Value::local(7_u64),
                Value::local("Madrid".to_string()),
                Value::local(28001_u32),
                Value::local("ok".to_string()),
            ],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .last()
            .unwrap()
            .borrow::<String>()
            .unwrap(),
        "7:Madrid:28001:ok"
    );
}

#[cfg(feature = "derive")]
#[test]
fn codec_free_flattening_accepts_a_tuple_field() {
    use phenix_graph::LocalPortLayout;
    #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
    struct Pair {
        #[port(flatten)]
        pair: (i32, String),
    }
    let mut g = GraphBuilder::new("local-tuple");
    let node = g
        .node("combine", |x: Pair| format!("{}:{}", x.pair.0, x.pair.1))
        .unwrap()
        .input(Layout::flatten_to::<2>())
        .unwrap();
    let left = g
        .expose_input(
            "left",
            node.inlets()
                .port(phenix_graph::InletPort::<_, i32>::new("run"))
                .field(phenix_graph::SlotField::<i32, i32>::new("pair.0"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let right = g
        .expose_input(
            "right",
            node.inlets()
                .port(phenix_graph::InletPort::<_, String>::new("run"))
                .field(phenix_graph::SlotField::<String, String>::new("pair.1"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let trigger = g.trigger("go", [left, right]).unwrap();
    let report = Deployment::new(g.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![Value::local(7_i32), Value::local("four".to_string())],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .last()
            .unwrap()
            .borrow::<String>()
            .unwrap(),
        "7:four"
    );
}

#[test]
fn local_output_modes_do_not_require_a_wire_codec() {
    #[derive(Debug, PartialEq, Eq)]
    struct Domain {
        n: i32,
    }
    let mut g = GraphBuilder::new("local-output-mode");
    let optional = g
        .node("optional", |n: i32| {
            Output::all((n > 0).then_some(Domain { n }))
        })
        .unwrap();
    let fallible = g
        .node("fallible", |n: i32| {
            Output::all(if n > 0 {
                Ok(Domain { n })
            } else {
                Err("rejected")
            })
        })
        .unwrap();
    let a = g.expose_input("a", optional.inlet()).unwrap();
    let b = g.expose_input("b", fallible.inlet()).unwrap();
    let trigger = g.trigger("run", [a, b]).unwrap();
    let graph = g.compile().unwrap();
    let engine = Deployment::new(graph)
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    let ok = engine
        .fire(
            trigger.get(),
            vec![Value::local(1_i32), Value::local(2_i32)],
        )
        .unwrap();
    assert!(ok.success(), "{:?}", ok.error_messages());
    assert!(
        ok.publications()
            .any(|p| p.borrow::<Result<Domain, &'static str>>() == Some(&Ok(Domain { n: 2 })))
    );
    let bad = engine
        .fire(
            trigger.get(),
            vec![Value::local(-1_i32), Value::local(-2_i32)],
        )
        .unwrap();
    assert!(bad.success(), "{:?}", bad.error_messages());
    assert_eq!(bad.publications().count(), 2);
    assert!(
        bad.publications()
            .any(|p| p.borrow::<Option<Domain>>() == Some(&None))
    );
    assert!(
        bad.publications()
            .any(|p| p.borrow::<Result<Domain, &'static str>>() == Some(&Err("rejected")))
    );
}
