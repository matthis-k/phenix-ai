//! Shared values are explicit Rust types, not a separate graph node category.
//! All tests execute through the production graph compiler and Deployment.
use phenix_graph::{Deployment, GraphBuilder, ReportRetention, Value};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct NoClone {
    id: usize,
}

#[test]
fn arc_is_part_of_the_graph_type_and_fanout_shares_its_allocation() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut graph = GraphBuilder::new("explicit-arc");
    let source = graph.node("source", |value: Arc<NoClone>| value).unwrap();
    let first_observed = Arc::clone(&observed);
    let first = graph.node("first", move |value: Arc<NoClone>| {
        first_observed.lock().unwrap().push((value.id, Arc::as_ptr(&value) as usize));
    }).unwrap();
    let second_observed = Arc::clone(&observed);
    let second = graph.node("second", move |value: Arc<NoClone>| {
        second_observed.lock().unwrap().push((value.id, Arc::as_ptr(&value) as usize));
    }).unwrap();
    graph.connect(source.output(), first.inlet()).unwrap();
    graph.connect(source.output(), second.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let original = Arc::new(NoClone { id: 37 });
    let pointer = Arc::as_ptr(&original) as usize;
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build().unwrap()
        .fire("run", vec![Value::local(original)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    let values = observed.lock().unwrap();
    assert_eq!(values.len(), 2);
    assert!(values.iter().all(|&(id, ptr)| id == 37 && ptr == pointer));
}

#[test]
fn explicit_arc_input_does_not_depend_on_domain_clone() {
    let mut graph = GraphBuilder::new("no-domain-clone");
    let inspect = graph.node("inspect", |value: Arc<NoClone>| value.id * 2).unwrap();
    let request = graph.expose_input("request", inspect.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build().unwrap()
        .fire("run", vec![Value::local(Arc::new(NoClone { id: 21 }))]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().borrow::<usize>(), Some(&42));
}

#[test]
fn explicit_arc_input_rejects_a_bare_domain_value() {
    let mut graph = GraphBuilder::new("arc-is-a-different-input-type");
    let inspect = graph.node("inspect", |value: Arc<NoClone>| value.id).unwrap();
    let request = graph.expose_input("request", inspect.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap()).build().unwrap();
    assert!(engine.fire("run", vec![Value::local(NoClone { id: 21 })]).is_err());
}
