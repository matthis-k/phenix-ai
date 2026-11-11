use phenix_graph::{
    Value,
    component::{Body, ComponentDef},
    connection::*,
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessHandoff, Step},
    runtime::SessionState,
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn spawn(def: &ComponentDef) -> Box<dyn Process> {
    let Body::Primitive(factory) = def.body() else {
        panic!("expected primitive")
    };
    factory.spawn()
}
fn step(handoff: &mut ProcessHandoff) -> Step {
    let job = handoff.next_job().unwrap();
    handoff.complete(job.run()).unwrap().unwrap()
}

#[test]
fn three_closure_components_and_zero_output_sink() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let collected = seen.clone();
    let sink = ComponentDef::sink(move |s: &String| collected.lock().unwrap().push(s.clone()));
    assert!(sink.interface().outputs().is_empty());
    let mut b = NetworkBuilder::new();
    let source = b
        .add("source", Arc::new(ComponentDef::source(|| 1..=3_i64)))
        .unwrap();
    b.start(&source).unwrap();
    let map = b
        .add("map", Arc::new(ComponentDef::map(|n: i64| n.to_string())))
        .unwrap();
    let sink = b.add("sink", Arc::new(sink)).unwrap();
    let one = NonZeroUsize::new(1).unwrap();
    b.connect(
        source.output::<i64>("output").unwrap(),
        map.input::<i64>("input").unwrap(),
        one,
    )
    .unwrap();
    b.connect(
        map.output::<String>("output").unwrap(),
        sink.input::<String>("input").unwrap(),
        one,
    )
    .unwrap();
    let compiled = b.build().unwrap().compile().unwrap();
    for _ in 0..2 {
        let mut session = compiled.start();
        assert_eq!(
            session.run_until_complete(64).state,
            SessionState::Completed
        );
    }
    assert_eq!(*seen.lock().unwrap(), ["1", "2", "3", "1", "2", "3"]);
}

#[test]
fn source_iterator_is_private_send_not_sync_and_steps_are_bounded() {
    let spawns = Arc::new(AtomicUsize::new(0));
    let count = spawns.clone();
    let source = ComponentDef::source(move || {
        count.fetch_add(1, Ordering::SeqCst);
        let next = std::cell::Cell::new(0);
        std::iter::from_fn(move || {
            let n = next.get();
            next.set(n + 1);
            (n < 2).then_some(n)
        })
    });
    let mut a = spawn(&source);
    let mut b = spawn(&source);
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    for expected in [0, 1] {
        let process = &mut a;
        let Step::Emit(value) = process.step(&mut ProcessContext::default()).unwrap() else {
            panic!("expected output")
        };
        assert_eq!(*value.value.borrow::<i32>().unwrap(), expected);
        assert_eq!(value.output, "output");
        assert!(value.value.bytes().is_empty());
    }
    let Step::Emit(value) = b.step(&mut ProcessContext::default()).unwrap() else {
        panic!("expected output")
    };
    assert_eq!(value.value.borrow::<i32>(), Some(&0));
    assert!(matches!(
        a.step(&mut ProcessContext::default()).unwrap(),
        Step::Finished
    ));
    let Step::Emit(value) = b.step(&mut ProcessContext::default()).unwrap() else {
        panic!("expected output")
    };
    assert_eq!(value.value.borrow::<i32>(), Some(&1));
}

#[test]
fn map_full_retry_retains_emission_without_repeating_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let map = ComponentDef::map(move |n: i64| {
        count.fetch_add(1, Ordering::SeqCst);
        n * 2
    });
    let mut handoff = ProcessHandoff::new(spawn(&map));
    let mut queues = ConnectionSet::new(vec![ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        Value::local(0_i64).identity(),
    )]);
    let targets = [FanoutTarget::identity(0)];
    let mut occupied = queues
        .prepare(&Packet::new(Value::local(0_i64)), &targets)
        .unwrap();
    queues.commit(&mut occupied).unwrap();
    // A leased input must work through borrow + explicit Clone, not bare extraction.
    handoff
        .admit(ProcessContext::new(
            BTreeMap::from([("input".into(), Value::local(3_i64).with_lease(Arc::new(())))]),
            vec![],
        ))
        .unwrap();
    let Step::Emit(emission) = step(&mut handoff) else {
        panic!("expected emission")
    };
    let mut pending = queues
        .prepare(&Packet::new(emission.value), &targets)
        .unwrap();
    for _ in 0..8 {
        assert_eq!(queues.commit(&mut pending), Err(ConnectionError::Full));
        assert!(handoff.next_job().is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    queues.pop(0).unwrap();
    queues.commit(&mut pending).unwrap();
    assert_eq!(queues.pop(0).unwrap().value().borrow::<i64>(), Some(&6));
    handoff.publication_committed().unwrap();
    assert!(matches!(step(&mut handoff), Step::Complete));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn sink_borrows_non_clone_leased_input_once_and_handles_eos() {
    struct NotClone(i64);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut sink = spawn(&ComponentDef::sink(move |n: &NotClone| {
        assert_eq!(n.0, 7);
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let mut cx = ProcessContext::new(
        BTreeMap::from([(
            "input".into(),
            Value::local(NotClone(7)).with_lease(Arc::new(())),
        )]),
        vec![],
    );
    assert!(matches!(sink.step(&mut cx).unwrap(), Step::Complete));
    assert!(matches!(sink.step(&mut cx).unwrap(), Step::Complete));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        sink.step(&mut ProcessContext::new(
            BTreeMap::new(),
            vec!["input".into()]
        ))
        .unwrap(),
        Step::Finished
    ));
}

#[test]
fn empty_source_finishes_without_emitting_or_calling_sink() {
    let mut b = NetworkBuilder::new();
    let source = b
        .add(
            "source",
            Arc::new(ComponentDef::source(std::iter::empty::<i64>)),
        )
        .unwrap();
    b.start(&source).unwrap();
    let sink = b
        .add(
            "sink",
            Arc::new(ComponentDef::sink(|_: &i64| panic!("empty source emitted"))),
        )
        .unwrap();
    b.connect(
        source.output::<i64>("output").unwrap(),
        sink.input::<i64>("input").unwrap(),
        NonZeroUsize::new(1).unwrap(),
    )
    .unwrap();
    let mut session = b.build().unwrap().compile().unwrap().start();
    assert_eq!(session.run_until_complete(8).state, SessionState::Completed);
}
