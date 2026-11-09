use phenix_graph::{
    DEFAULT_MAX_ACTIVATIONS, Deployment, Engine, ExecutionSession, Executor, GraphBuilder,
    LocalExecutor, Value, WaveCancellation, WaveError,
};
use std::sync::{Arc, Mutex};

fn productive_cycle() -> Engine {
    let mut graph = GraphBuilder::new("productive-cycle");
    let step = graph.node("step", |x: i64| x + 1).unwrap();
    graph.connect(step.output(), step.inlet()).unwrap();
    let input = graph.expose_input("input", step.inlet()).unwrap();
    graph.trigger("start", [input]).unwrap();
    Deployment::new(graph.compile().unwrap()).build().unwrap()
}

fn step(wave: &mut ExecutionSession<'_>) {
    let (ticket, ctx) = wave
        .next_ready()
        .expect("activation within budget")
        .into_parts();
    let result = wave.action(ticket).unwrap().execute(ctx);
    wave.complete(ticket, result).unwrap();
}

#[test]
fn productive_cycle_stops_at_default_budget() {
    let engine = productive_cycle();
    let mut wave = engine.begin("start", vec![Value::local(0_i64)]).unwrap();
    // A bounded driver makes this regression fail instead of hanging on old code.
    for _ in 0..DEFAULT_MAX_ACTIVATIONS {
        step(&mut wave);
    }
    assert!(
        wave.next_ready().is_none(),
        "productive cycle exceeded budget"
    );
    let report = wave.finish().unwrap();
    assert!(!report.success());
    assert!(!report.cancelled());
    assert_eq!(report.error_count(), 1);
    assert!(report.budget_exhausted());
    assert_eq!(report.activation_count(), DEFAULT_MAX_ACTIVATIONS);
    assert_eq!(
        report.errors(),
        &[WaveError::BudgetExhausted {
            max_activations: DEFAULT_MAX_ACTIVATIONS,
            activations: DEFAULT_MAX_ACTIVATIONS,
        }]
    );
}

#[test]
fn synchronous_fire_returns_a_structured_budget_diagnostic() {
    let engine = productive_cycle().with_max_activations(3);
    let report = engine.fire("start", vec![Value::local(0_i64)]).unwrap();
    assert_eq!(report.activation_count(), 3);
    assert!(!report.success());
    assert!(!report.cancelled());
    assert!(report.budget_exhausted());
    assert_eq!(report.publication_events().len(), 3);
    assert_eq!(
        report.errors(),
        &[WaveError::BudgetExhausted {
            max_activations: 3,
            activations: 3,
        }]
    );
    assert!(report.error_messages()[0].contains("BudgetExhausted"));
}

#[test]
fn dag_is_unchanged_and_completing_exactly_at_limit_succeeds() {
    let mut graph = GraphBuilder::new("dag");
    let a = graph.node("a", |x: i64| x + 1).unwrap();
    let b = graph.node("b", |x: i64| x * 2).unwrap();
    graph.connect(a.output(), b.inlet()).unwrap();
    let input = graph.expose_input("input", a.inlet()).unwrap();
    graph.trigger("start", [input]).unwrap();
    let graph = graph.compile().unwrap();
    for limit in [2, DEFAULT_MAX_ACTIVATIONS] {
        let engine = Deployment::new(Arc::clone(&graph))
            .max_activations(limit)
            .build()
            .unwrap();
        let report = engine.fire("start", vec![Value::local(5_i64)]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert!(!report.budget_exhausted());
        assert_eq!(report.activation_count(), 2);
        assert_eq!(
            report.publications().last().unwrap().borrow::<i64>(),
            Some(&12)
        );
    }
}

#[test]
fn interleaved_and_later_waves_have_independent_budgets() {
    let engine = productive_cycle().with_max_activations(2);
    let mut a = engine.begin("start", vec![Value::local(0_i64)]).unwrap();
    let mut b = engine.begin("start", vec![Value::local(10_i64)]).unwrap();
    step(&mut a);
    step(&mut b);
    step(&mut a);
    assert!(a.next_ready().is_none());
    assert!(a.finish().unwrap().budget_exhausted());
    step(&mut b);
    assert!(b.next_ready().is_none());
    let b = b.finish().unwrap();
    assert_eq!(b.activation_count(), 2);
    assert!(b.budget_exhausted());
    let c = engine.fire("start", vec![Value::local(20_i64)]).unwrap();
    assert_eq!(c.activation_count(), 2);
    assert!(c.budget_exhausted());
}

#[test]
fn zero_budget_does_not_dispatch_or_report_incomplete_inputs() {
    let engine = productive_cycle().with_max_activations(0);
    let report = engine.fire("start", vec![Value::local(0_i64)]).unwrap();
    assert_eq!(report.activation_count(), 0);
    assert!(report.publication_events().is_empty());
    assert_eq!(
        report.errors(),
        &[WaveError::BudgetExhausted {
            max_activations: 0,
            activations: 0,
        }]
    );
}

#[test]
fn exhaustion_invalidates_other_in_flight_tickets() {
    let mut graph = GraphBuilder::new("in-flight-budget");
    let cycle = graph.node("cycle", |x: i64| x + 1).unwrap();
    let held = graph.node("held", |x: i64| x).unwrap();
    graph.connect(cycle.output(), cycle.inlet()).unwrap();
    let cycle_input = graph.expose_input("cycle", cycle.inlet()).unwrap();
    let held_input = graph.expose_input("held", held.inlet()).unwrap();
    graph.trigger("start", [cycle_input, held_input]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .max_activations(2)
        .build()
        .unwrap();
    let mut wave = engine
        .begin("start", vec![Value::local(0_i64), Value::local(0_i64)])
        .unwrap();
    let cycle_id = engine.trigger_targets("start").unwrap()[0][0].node_id();
    let first = wave.next_ready().unwrap().into_parts();
    let second = wave.next_ready().unwrap().into_parts();
    let ((cycle_ticket, cycle_ctx), (ticket, ctx)) = if first.0.node_id() == cycle_id {
        (first, second)
    } else {
        (second, first)
    };
    let cycle_result = wave.action(cycle_ticket).unwrap().execute(cycle_ctx);
    wave.complete(cycle_ticket, cycle_result).unwrap();
    let result = wave.action(ticket).unwrap().execute(ctx);
    assert_eq!(wave.in_flight_count(), 1);
    assert!(wave.next_ready().is_none());
    assert!(wave.action(ticket).is_none());
    assert!(
        wave.complete(ticket, result)
            .unwrap_err()
            .contains("budget exhausted")
    );
    assert_eq!(wave.in_flight_count(), 0);
    assert!(wave.finish().unwrap().budget_exhausted());
}

#[test]
fn cancellation_during_dispatch_suppresses_results_without_preempting_action() {
    for fail in [false, true] {
        let handle: Arc<Mutex<Option<WaveCancellation>>> = Arc::new(Mutex::new(None));
        let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut graph = GraphBuilder::new("cancel-during-dispatch");
        let node = graph
            .node("action", {
                let handle = Arc::clone(&handle);
                let effects = Arc::clone(&effects);
                move |x: i64| {
                    handle.lock().unwrap().as_ref().unwrap().request();
                    // Cancellation is a request, not preemption or rollback.
                    effects.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    x + 1
                }
            })
            .unwrap();
        let input = graph.expose_input("input", node.inlet()).unwrap();
        graph.trigger("start", [input]).unwrap();
        let engine = Deployment::new(graph.compile().unwrap())
            .max_activations(1)
            .build()
            .unwrap();
        let mut wave = engine.begin("start", vec![Value::local(0_i64)]).unwrap();
        *handle.lock().unwrap() = Some(wave.cancellation());
        let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
        let result = LocalExecutor.dispatch(wave.action(ticket).unwrap(), ctx);
        let result = if fail {
            Err("dispatch failed after cancellation".into())
        } else {
            result
        };
        assert!(
            wave.complete(ticket, result)
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(wave.next_ready().is_none());
        let report = wave.finish().unwrap();
        assert_eq!(effects.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(report.cancelled());
        assert!(!report.success());
        assert!(!report.budget_exhausted());
        assert!(report.errors().is_empty());
        assert!(report.publication_events().is_empty());
        assert!(report.delivery_events().is_empty());
    }
}
