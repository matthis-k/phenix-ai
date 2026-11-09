use phenix_graph::{Deployment, GraphBuilder, Value, WaveError};

#[test]
fn dispatch_limit_allows_outstanding_completions_when_no_more_work_is_ready() {
    let mut graph = GraphBuilder::new("outstanding-at-limit");
    let a = graph.node("a", |x: i64| x + 1).unwrap();
    let b = graph.node("b", |x: i64| x + 2).unwrap();
    let a_input = graph.expose_input("a", a.inlet()).unwrap();
    let b_input = graph.expose_input("b", b.inlet()).unwrap();
    graph.trigger("start", [a_input, b_input]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .max_activations(2)
        .build()
        .unwrap();
    let mut wave = engine
        .begin("start", vec![Value::local(1_i64), Value::local(2_i64)])
        .unwrap();
    let first = wave.next_ready().unwrap().into_parts();
    let second = wave.next_ready().unwrap().into_parts();
    assert!(wave.next_ready().is_none());
    assert!(!wave.can_finish());
    assert_eq!(wave.in_flight_count(), 2);
    for (ticket, ctx) in [second, first] {
        let result = wave.action(ticket).unwrap().execute(ctx);
        wave.complete(ticket, result).unwrap();
    }
    let report = wave.finish().unwrap();
    assert!(report.success(), "{:?}", report.errors());
    assert_eq!(report.activation_count(), 2);
    assert_eq!(report.publication_events().len(), 2);
}

#[test]
fn cancellation_precedes_zero_budget_exhaustion_and_is_wave_local() {
    let mut graph = GraphBuilder::new("cancel-before-budget");
    let node = graph.node("node", |x: i64| x).unwrap();
    let input = graph.expose_input("input", node.inlet()).unwrap();
    graph.trigger("start", [input]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .max_activations(0)
        .build()
        .unwrap();
    let mut wave = engine.begin("start", vec![Value::local(0_i64)]).unwrap();
    wave.cancellation().request();
    assert!(wave.can_finish());
    assert!(wave.next_ready().is_none());
    let report = wave.finish().unwrap();
    assert!(report.cancelled());
    assert!(!report.budget_exhausted());
    assert_eq!(report.activation_count(), 0);
    assert!(report.errors().is_empty());

    let mut later = engine.begin("start", vec![Value::local(0_i64)]).unwrap();
    assert!(later.next_ready().is_none());
    assert!(later.next_ready().is_none());
    assert!(later.cancel().is_empty());
    let report = later.finish().unwrap();
    assert!(report.cancelled());
    assert!(report.budget_exhausted());
    assert_eq!(
        report.errors(),
        &[WaveError::BudgetExhausted {
            max_activations: 0,
            activations: 0,
        }]
    );
}
