//! H4: simultaneous process-scoped policy notifications must neither be
//! lost nor create additional activations. No timer or mocked wake queue.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct ExternalGate(Arc<AtomicBool>);
impl ReadinessPolicy for ExternalGate {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        if !self.0.load(Ordering::Acquire) {
            return vec![];
        }
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .map(|head| head.port.clone())
            .collect()
    }

    fn notification_driven(&self) -> bool {
        true
    }
}

struct Count(Arc<AtomicUsize>);
impl Process for Count {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        let packet = cx.take("in").ok_or("no packet to count")?;
        let _ = packet.get::<u64>()?;
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}

#[test]
fn concurrent_process_notifiers_coalesce_and_preserve_every_target() {
    const PROCESSES: usize = 64;
    const WORKERS: usize = 8;
    let mut builder = NetworkBuilder::new();
    let mut options = SessionOptions::default();
    let iface = Arc::new(Interface::new([PortDef::wire::<u64>("in").unwrap()], []).unwrap());
    let hits: Vec<_> = (0..PROCESSES)
        .map(|_| Arc::new(AtomicUsize::new(0)))
        .collect();
    let gates: Vec<_> = (0..PROCESSES)
        .map(|_| Arc::new(AtomicBool::new(false)))
        .collect();
    for i in 0..PROCESSES {
        let path = format!("custom-{i}");
        let node = builder
            .node(path.clone(), iface.clone(), {
                let count = hits[i].clone();
                Arc::new(move || Count(count.clone()))
            })
            .unwrap();
        builder
            .expose_input(
                format!("in-{i}"),
                node.wire_input::<u64>("in").unwrap(),
                one(),
            )
            .unwrap();
        options.readiness.insert(
            path,
            Readiness::Custom(Arc::new(ExternalGate(gates[i].clone()))),
        );
    }
    let network = builder.build().unwrap().compile().unwrap();
    let mut session = network.start_with_options(options);
    let mut notifiers = Vec::with_capacity(PROCESSES);
    for i in 0..PROCESSES {
        notifiers.push(session.process_notifier(&format!("custom-{i}")).unwrap());
        session
            .inject(&format!("in-{i}"), Value::of(i as u64))
            .unwrap();
    }
    assert_eq!(session.run_until_idle(PROCESSES * 2).steps, 0);

    let barrier = Arc::new(Barrier::new(WORKERS + 1));
    let notifiers = Arc::new(notifiers);
    let gates = Arc::new(gates);
    let mut workers = Vec::new();
    for worker in 0..WORKERS {
        let notifiers = notifiers.clone();
        let gates = gates.clone();
        let barrier = barrier.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            for i in (worker..PROCESSES).step_by(WORKERS) {
                gates[i].store(true, Ordering::Release);
                // Many notifications for one index must coalesce; none may
                // create a separate ticket or admit the same packet twice.
                for _ in 0..16 {
                    assert!(notifiers[i].notify());
                }
            }
        }));
    }
    barrier.wait();
    // Drive while notifications may still be arriving on other threads.
    for _ in 0..WORKERS {
        session.run_until_idle(PROCESSES * 2);
        thread::yield_now();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    session.run_until_idle(PROCESSES * 2);
    for (i, count) in hits.iter().enumerate() {
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "process {i} missed or duplicated work"
        );
    }
    assert_eq!(session.run_until_idle(PROCESSES * 2).steps, 0);

    let stale = notifiers[0].clone();
    drop(session);
    // A stale notifier must not be able to reactivate or affect a new session.
    assert!(!stale.notify());
    let fresh = network.start();
    assert!(fresh.process_notifier("custom-0").unwrap().notify());
    assert!(!stale.notify());
}
