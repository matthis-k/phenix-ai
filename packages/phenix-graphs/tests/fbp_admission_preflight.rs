//! Admission is transport-only. Startup must not execute invalid declarations.
use phenix_graph::{
    ComponentDef, Interface, PortDef,
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions, SessionState},
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct CountingReadiness(Arc<AtomicUsize>);
impl ReadinessPolicy for CountingReadiness {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

#[test]
fn admission_does_not_call_custom_readiness_but_driver_selects_the_packet() {
    let seen = Arc::new(AtomicUsize::new(0));
    let seen_by_sink = seen.clone();
    let mut builder = NetworkBuilder::new();
    let sink = builder
        .add(
            "sink",
            Arc::new(ComponentDef::sink(move |value: &i64| {
                seen_by_sink.fetch_add(*value as usize, Ordering::SeqCst);
            })),
        )
        .unwrap();
    builder
        .expose_input("in", sink.input::<i64>("input").unwrap(), capacity())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let selections = Arc::new(AtomicUsize::new(0));
    let mut session = compiled.start_with_options(SessionOptions {
        readiness: BTreeMap::from([(
            "sink".to_owned(),
            Readiness::Custom(Arc::new(CountingReadiness(selections.clone()))),
        )]),
        ..Default::default()
    });
    assert_eq!(selections.load(Ordering::SeqCst), 0);
    session.inject_local("in", 7_i64).unwrap();
    assert_eq!(selections.load(Ordering::SeqCst), 0);
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    session.run_until_idle(16);
    assert!(selections.load(Ordering::SeqCst) > 0);
    assert_eq!(seen.load(Ordering::SeqCst), 7);
}

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Complete)
    }
}

#[test]
fn invalid_session_options_do_not_spawn_any_process_factory() {
    let spawned = Arc::new(AtomicUsize::new(0));
    let spawned_in_factory = spawned.clone();
    let mut builder = NetworkBuilder::new();
    builder
        .node(
            "worker",
            Arc::new(Interface::new([PortDef::local::<i64>("input")], []).unwrap()),
            Arc::new(move || {
                spawned_in_factory.fetch_add(1, Ordering::SeqCst);
                Noop
            }),
        )
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();

    let invalid_readiness = SessionOptions {
        readiness: BTreeMap::from([(
            "worker".to_owned(),
            Readiness::AtLeast(NonZeroUsize::new(2).unwrap()),
        )]),
        ..Default::default()
    };
    let mut failed = compiled.start_with_options(invalid_readiness);
    assert_eq!(failed.state(), SessionState::Failed);
    assert_eq!(spawned.load(Ordering::SeqCst), 0);

    let mut invalid_resource = SessionOptions::default();
    invalid_resource
        .resources
        .insert("unknown".to_owned(), vec![]);
    let mut failed = compiled.start_with_options(invalid_resource);
    assert_eq!(failed.state(), SessionState::Failed);
    assert_eq!(spawned.load(Ordering::SeqCst), 0);

    let mut invalid_output = SessionOptions::default();
    invalid_output
        .discard_outputs
        .insert(("worker".to_owned(), "missing".to_owned()));
    let mut failed = compiled.start_with_options(invalid_output);
    assert_eq!(failed.state(), SessionState::Failed);
    assert_eq!(spawned.load(Ordering::SeqCst), 0);

    let mut valid = compiled.start();
    // An unconnected input may legitimately deliver EOS immediately.
    assert_ne!(valid.state(), SessionState::Failed);
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
}
