//! Boundary and layout regression tests for the 9/10-target improvement pass.
use phenix_graph::Node;
use phenix_graph::ReportRetention;
use phenix_graph::{GraphBuilder, Store, Value, Layout};

#[test]
fn public_input_from_another_graph_is_never_accepted_by_trigger() {
    let mut a = GraphBuilder::new("origin");
    let a_node = a.node("same", |x: i32| x + 1).unwrap();
    let wrong = a.expose_input("request", a_node.inlet()).unwrap();
    let mut b = GraphBuilder::new("destination");
    let b_node = b.node("same", |x: i32| x + 1).unwrap();
    let _right = b.expose_input("request", b_node.inlet()).unwrap();
    assert!(b.trigger("wrong", [wrong]).is_err(),
        "same local ID/name/schema in another builder must not confer authority");
}

#[test]
fn trigger_cannot_repeat_an_exported_port() {
    let mut g = GraphBuilder::new("duplicate-trigger-ports");
    let n = g.node("echo", |x: i32| x).unwrap();
    let input = g.expose_input("input", n.inlet()).unwrap();
    assert!(g.trigger("twice", [input.clone(), input.clone()]).is_err());
    let fire = g.trigger("once", [input]).unwrap();
    let report = Deployment::new(g.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(fire.get(), vec![Value::local(2_i32)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
}

#[test]
fn duplicate_connections_are_rejected_when_authored() {
    let mut g = GraphBuilder::new("no-duplicate-edges");
    let a = g.node("a", |x: i32| x + 1).unwrap();
    let b = g.node("b", |x: i32| x + 1).unwrap();
    g.connect(a.output(), b.inlet()).unwrap();
    assert!(g.connect(a.output(), b.inlet()).is_err());
}

#[test]
fn duplicate_executor_bindings_are_explicit_errors() {
    use phenix_graph::{Deployment, Executor, NodeAction, InvocationCtx, Emission};
    use std::sync::Arc;
    struct Local;
    impl Executor for Local {
        fn dispatch(&self, action: &dyn NodeAction, ctx: InvocationCtx)
            -> Result<Vec<Emission>, String> { action.execute(ctx) }
    }
    let mut g = GraphBuilder::new("duplicate-executors");
    let _n = g.node("n", |x: i32| x).unwrap();
    let mut deployment = Deployment::new(g.compile().unwrap());
    deployment.node_executor("n", Arc::new(Local)).unwrap();
    assert!(deployment.node_executor("n", Arc::new(Local)).is_err());
}

#[test]
fn edge_adapter_failures_are_attributed_to_edges() {
    use phenix_graph::{Deployment, EdgeAdapter, WaveError};
    use std::sync::Arc;
    struct Reject;
    impl EdgeAdapter for Reject {
        fn prepare(&self, _value: &Value) -> Result<Value, String> {
            Err("adapter unavailable".into())
        }
    }
    let mut g = GraphBuilder::new("adapter-errors");
    let source = g.node("source", |x: i32| x + 1).unwrap();
    let sink = g.node("sink", |x: i32| x + 1).unwrap();
    g.connect(source.output(), sink.inlet()).unwrap();
    let ingress = g.expose_input("request", source.inlet()).unwrap();
    let fire = g.trigger("go", [ingress]).unwrap();
    let mut deployment = Deployment::new(g.compile().unwrap());
    deployment.edge_adapter("source", "value", "sink", "run", "value", Arc::new(Reject)).unwrap();
    let report = deployment.build().unwrap().fire(fire.get(), vec![Value::local(5_i32)]).unwrap();
    assert!(!report.success());
    assert!(report.errors().iter().any(|error| matches!(error,
        WaveError::EdgeFailed {reason, ..} if reason.contains("adapter unavailable"))));
}

#[cfg(feature = "derive")]
mod nested {
    use super::*;
    use phenix_graph::LocalPortLayout;
    #[derive(Clone, LocalPortLayout)]
    struct L9 { value: i32 }
    #[derive(Clone, LocalPortLayout)]
    struct L8 { inner: L9 }
    #[derive(Clone, LocalPortLayout)]
    struct L7 { inner: L8 }
    #[derive(Clone, LocalPortLayout)]
    struct L6 { inner: L7 }
    #[derive(Clone, LocalPortLayout)]
    struct L5 { inner: L6 }
    #[derive(Clone, LocalPortLayout)]
    struct L4 { inner: L5 }
    #[derive(Clone, LocalPortLayout)]
    struct L3 { inner: L4 }
    #[derive(Clone, LocalPortLayout)]
    struct L2 { inner: L3 }
    #[derive(Clone, LocalPortLayout)]
    struct L1 { inner: L2 }

    #[test]
    fn deep_layout_is_not_silently_limited_to_eight_levels() {
        let mut g = GraphBuilder::new("deep-9");
        let n = g.node("read", |x: L1| x.inner.inner.inner.inner.inner.inner.inner.inner.value).unwrap().input(Layout::deep()).unwrap();
        let ingress = g.expose_input("leaf", n.field(L1::SLOT_INNER.then(L2::SLOT_INNER).then(L3::SLOT_INNER).then(L4::SLOT_INNER).then(L5::SLOT_INNER).then(L6::SLOT_INNER).then(L7::SLOT_INNER).then(L8::SLOT_INNER).then(L9::SLOT_VALUE))).unwrap();
        g.expose_output("answer", n.output()).unwrap();
        let trigger = g.trigger("go", [ingress]).unwrap();
        let report = Deployment::new(g.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(trigger.get(), vec![Value::local(1337_i32)]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.publications().last().unwrap().borrow::<i32>(), Some(&1337));
    }

    #[test]
    fn bounded_layout_has_a_distinct_depth_semantics() {
        let mut g = GraphBuilder::new("bounded-2");
        let n = g.node("read", |x: L1| x.inner.inner.inner.inner.inner.inner.inner.inner.value).unwrap().input(Layout::flatten_to::<2>()).unwrap();
        let ingress = g.expose_input("nested", n.field(L1::SLOT_INNER.then(L2::SLOT_INNER))).unwrap();
        let trigger = g.trigger("go", [ingress]).unwrap();
        let value = L3 { inner: L4 { inner: L5 { inner: L6 { inner: L7 { inner: L8 { inner: L9 { value: 47 } } } } } } };
        let report = Deployment::new(g.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(trigger.get(), vec![Value::local(value)]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.publications().last().unwrap().borrow::<i32>(), Some(&47));
    }

    #[derive(LocalPortLayout)]
    struct Address { city: String, zip: u32 }
    #[derive(LocalPortLayout)]
    struct Order {
        #[port(flatten)]
        address: Address,
        amount: u32,
    }
    #[test]
    fn explicit_flatten_overrides_the_depth_budget() {
        let mut g = GraphBuilder::new("force-flatten");
        let n = g.node("read", Node::with_layout(|o: Order| o.address.city.len() as u32
            + o.address.zip + o.amount, Layout::<Order>::fields())).unwrap();
        let city = g.expose_input("city",
            n.field(Order::SLOT_ADDRESS.then(Address::SLOT_CITY))).unwrap();
        let zip = g.expose_input("zip",
            n.field(Order::SLOT_ADDRESS.then(Address::SLOT_ZIP))).unwrap();
        let amount = g.expose_input("amount", n.field(Order::SLOT_AMOUNT)).unwrap();
        let fire = g.trigger("go", [city, zip, amount]).unwrap();
        let report = Deployment::new(g.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap().fire(fire.get(),
            vec![Value::local("Rome".to_string()), Value::local(10_u32), Value::local(3_u32)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.publications().last().unwrap().borrow::<u32>(), Some(&17));
    }

    #[derive(LocalPortLayout)]
    struct WithTuple { point: (i32, i32) }
    #[test]
    fn nested_tuple_deep_layout_uses_local_values() {
        let mut g = GraphBuilder::new("deep-tuple");
        let n = g.node("sum", Node::with_layout(|x: WithTuple| x.point.0 + x.point.1, Layout::<WithTuple>::deep())).unwrap();
        let a = g.expose_input("x", n.local_slot::<i32>("point.0")).unwrap();
        let b = g.expose_input("y", n.local_slot::<i32>("point.1")).unwrap();
        let fire = g.trigger("go", [a, b]).unwrap();
        let result = Deployment::new(g.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
            .fire(fire.get(), vec![Value::local(3_i32), Value::local(7_i32)]).unwrap();
        assert!(result.success(), "{:?}", result.error_messages());
        assert_eq!(result.publications().last().unwrap().borrow::<i32>(), Some(&10));
    }
}
