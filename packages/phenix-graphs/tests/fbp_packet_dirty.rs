//! Packet-driven dirty scheduling must preserve bounded capture progress,
//! EOS propagation and externally delivered wakeups.
use phenix_graph::{
    Value,
    component::ComponentDef,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct RunningSum {
    value: i64,
    just_emitted: bool,
    effects: Arc<AtomicUsize>,
}
impl Process for RunningSum {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.just_emitted {
            self.just_emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.value += value.get::<i64>()?;
        self.effects.fetch_add(1, Ordering::SeqCst);
        self.just_emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::of(self.value),
        )))
    }
}

#[test]
fn bounded_capture_release_retries_publisher_with_unrelated_idle_process() {
    let effects = Arc::new(AtomicUsize::new(0));
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let mut builder = NetworkBuilder::new();
    let factory = {
        let effects = effects.clone();
        Arc::new(move || RunningSum {
            value: 0,
            just_emitted: false,
            effects: effects.clone(),
        })
    };
    let active = builder
        .node("active", interface.clone(), factory.clone())
        .unwrap();
    let idle = builder.node("idle", interface, factory).unwrap();
    builder
        .expose_input("active_in", active.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_input("idle_in", idle.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_output("capture", active.wire_output::<i64>("out").unwrap())
        .unwrap();

    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            capture_capacity: one(),
            ..Default::default()
        });

    session.inject("active_in", Value::of(1_i64)).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 2);
    session.inject("active_in", Value::of(2_i64)).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(session.state(), SessionState::Waiting);
    let first = session.take_output("capture").unwrap().unwrap();
    assert_eq!(first.value().get::<i64>().unwrap(), 1);
    assert_eq!(session.run_until_idle(32).steps, 1);
    let second = session.take_output("capture").unwrap().unwrap();
    assert_eq!(second.value().get::<i64>().unwrap(), 3);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert_eq!(session.state(), SessionState::Quiescent);

    session.close_input("active_in").unwrap();
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert!(session.output_closed("capture").unwrap());
    // An unrelated ingress stays open; only the requested branch completed.
    assert_eq!(session.state(), SessionState::Quiescent);
}

struct WaitOnWake {
    stage: u8,
    resumed: Arc<AtomicUsize>,
}
impl Process for WaitOnWake {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        match self.stage {
            0 => {
                cx.take("in").ok_or("missing input")?;
                self.stage = 1;
                Ok(Step::WaitWake)
            }
            1 => {
                self.resumed.fetch_add(1, Ordering::SeqCst);
                self.stage = 2;
                Ok(Step::Complete)
            }
            _ => Ok(Step::Finished),
        }
    }
}

#[test]
fn targeted_wakes_preserve_process_identity_and_listener_notifications() {
    use std::time::Duration;

    let left_count = Arc::new(AtomicUsize::new(0));
    let right_count = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let iface = Arc::new(Interface::new([PortDef::wire::<i64>("in").unwrap()], []).unwrap());

    for (path, count) in [("left", left_count.clone()), ("right", right_count.clone())] {
        let node = builder
            .node(
                path,
                iface.clone(),
                Arc::new(move || WaitOnWake {
                    stage: 0,
                    resumed: count.clone(),
                }),
            )
            .unwrap();
        builder
            .expose_input(path, node.wire_input::<i64>("in").unwrap(), one())
            .unwrap();
    }

    // An unrelated large packet-driven inventory must not be a prerequisite
    // for either targeted wake to make progress.
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        std::hint::black_box(*value);
    }));
    for index in 0..64 {
        let node = builder.add(format!("idle-{index}"), sink.clone()).unwrap();
        builder
            .expose_input(
                format!("idle-in-{index}"),
                node.input::<u64>("input").unwrap(),
                one(),
            )
            .unwrap();
    }

    let mut session = builder.build().unwrap().compile().unwrap().start();
    let left = session.wake_handle("left").unwrap();
    let right = session.wake_handle("right").unwrap();
    let mut listener = session.wake_listener();
    session.inject("left", Value::of(1_i64)).unwrap();
    session.inject("right", Value::of(2_i64)).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 2);

    assert!(left.wake());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(left_count.load(Ordering::SeqCst), 1);
    assert_eq!(right_count.load(Ordering::SeqCst), 0);

    // The generic notifier has no process identity. It must not synthesize a
    // wake for the still-waiting right process.
    assert!(session.notifier().notify());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(session.run_until_idle(32).steps, 0);
    assert_eq!(right_count.load(Ordering::SeqCst), 0);

    assert!(right.wake());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(right_count.load(Ordering::SeqCst), 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

#[test]
fn external_wake_epoch_resumes_waiting_packet_driven_process() {
    let resumed = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let iface = Arc::new(Interface::new([PortDef::wire::<i64>("in").unwrap()], []).unwrap());
    let node = builder
        .node(
            "waker",
            iface,
            Arc::new({
                let resumed = resumed.clone();
                move || WaitOnWake {
                    stage: 0,
                    resumed: resumed.clone(),
                }
            }),
        )
        .unwrap();
    builder
        .expose_input("input", node.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    session.inject("input", Value::of(42_i64)).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(resumed.load(Ordering::SeqCst), 0);
    assert_eq!(session.state(), SessionState::Waiting);
    assert!(session.wake_handle("waker").unwrap().wake());
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(resumed.load(Ordering::SeqCst), 1);
}

struct GatedPolicy {
    calls: Arc<AtomicUsize>,
    allowed: Arc<AtomicBool>,
}
impl ReadinessPolicy for GatedPolicy {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.allowed.load(Ordering::SeqCst) {
            return Vec::new();
        }
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

#[test]
fn mixed_custom_node_is_polled_without_disabling_unrelated_packet_nodes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let allowed = Arc::new(AtomicBool::new(false));
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        std::hint::black_box(*value);
    }));
    let mut builder = NetworkBuilder::new();
    let custom = builder.add("special", sink.clone()).unwrap();
    let ordinary = builder.add("ordinary", sink).unwrap();
    builder
        .expose_input("special_in", custom.input::<u64>("input").unwrap(), one())
        .unwrap();
    builder
        .expose_input(
            "ordinary_in",
            ordinary.input::<u64>("input").unwrap(),
            one(),
        )
        .unwrap();

    let mut options = SessionOptions::default();
    options.readiness.insert(
        "special".into(),
        Readiness::Custom(Arc::new(GatedPolicy {
            calls: calls.clone(),
            allowed: allowed.clone(),
        })),
    );
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);
    session.inject_local("special_in", 7_u64).unwrap();
    let before = calls.load(Ordering::SeqCst);
    assert!(before > 0);
    session.inject_local("ordinary_in", 42_u64).unwrap();
    assert!(calls.load(Ordering::SeqCst) > before);
    // The custom policy must still be observed on every drive, even though
    // its own physical queue did not change when the ordinary sink ran.
    assert_eq!(session.run_until_idle(32).steps, 1);
    allowed.store(true, Ordering::SeqCst);
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

struct FinishOnEos(Arc<AtomicUsize>);
impl Process for FinishOnEos {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if !cx.is_eos("in") {
            return Err("EOS was not delivered".into());
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Finished)
    }
}

#[test]
fn eos_closes_only_affected_queues_even_for_reverse_ordered_chains() {
    for (length, reverse) in [(1, false), (32, false), (32, true), (64, true)] {
        let count = Arc::new(AtomicUsize::new(0));
        let iface = Arc::new(
            Interface::new(
                [PortDef::wire::<i64>("in").unwrap()],
                [PortDef::wire::<i64>("out").unwrap()],
            )
            .unwrap(),
        );
        let factory = {
            let count = count.clone();
            Arc::new(move || FinishOnEos(count.clone()))
        };
        let mut builder = NetworkBuilder::new();
        let mut nodes = Vec::new();
        for index in 0..length {
            nodes.push(
                builder
                    .node(format!("finish-{index}"), iface.clone(), factory.clone())
                    .unwrap(),
            );
        }
        for index in 1..length {
            let (producer, consumer) = if reverse {
                (index, index - 1)
            } else {
                (index - 1, index)
            };
            builder
                .connect(
                    nodes[producer].wire_output::<i64>("out").unwrap(),
                    nodes[consumer].wire_input::<i64>("in").unwrap(),
                    one(),
                )
                .unwrap();
        }
        let root = if reverse { length - 1 } else { 0 };
        let tail = if reverse { 0 } else { length - 1 };
        builder
            .expose_input("seed", nodes[root].wire_input::<i64>("in").unwrap(), one())
            .unwrap();
        builder
            .expose_output("tail", nodes[tail].wire_output::<i64>("out").unwrap())
            .unwrap();

        // Keep many unrelated writer-open branches in the same network to
        // detect accidental full EOS propagation into independent inputs.
        let sink = Arc::new(ComponentDef::sink(|value: &u64| {
            std::hint::black_box(*value);
        }));
        for index in 0..64 {
            let idle = builder.add(format!("idle-{index}"), sink.clone()).unwrap();
            builder
                .expose_input(
                    format!("idle-in-{index}"),
                    idle.input::<u64>("input").unwrap(),
                    one(),
                )
                .unwrap();
        }

        let mut session = builder.build().unwrap().compile().unwrap().start();
        session.close_input("seed").unwrap();
        let report = session.run_until_idle(length + 16);
        assert_eq!(report.steps, length, "reverse={reverse}, length={length}");
        assert!(!report.budget_exhausted);
        assert_eq!(count.load(Ordering::SeqCst), length);
        assert!(session.output_closed("tail").unwrap());
        assert_eq!(session.run_until_idle(16).steps, 0);
        assert_eq!(session.state(), SessionState::Quiescent);
    }
}

struct NotifiedPolicy {
    calls: Arc<AtomicUsize>,
    allowed: Arc<AtomicBool>,
}
impl ReadinessPolicy for NotifiedPolicy {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.allowed.load(Ordering::SeqCst) {
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

#[test]
fn notified_custom_reevaluates_only_on_targeted_or_generic_notification() {
    use std::time::Duration;

    let allowed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let sink = Arc::new(ComponentDef::sink(|value: &u64| {
        std::hint::black_box(*value);
    }));
    let targeted = builder.add("targeted", sink.clone()).unwrap();
    let ordinary = builder.add("ordinary", sink).unwrap();
    builder
        .expose_input("targeted_in", targeted.input::<u64>("input").unwrap(), one())
        .unwrap();
    builder
        .expose_input("ordinary_in", ordinary.input::<u64>("input").unwrap(), one())
        .unwrap();
    let mut options = SessionOptions::default();
    options.readiness.insert(
        "targeted".into(),
        Readiness::Custom(Arc::new(NotifiedPolicy {
            calls: calls.clone(),
            allowed: allowed.clone(),
        })),
    );
    let mut session = builder.build().unwrap().compile().unwrap().start_with_options(options);
    let notifier = session.process_notifier("targeted").unwrap();
    let mut listener = session.wake_listener();
    session.inject_local("targeted_in", 1_u64).unwrap();
    assert_eq!(session.run_until_idle(16).steps, 0);
    let before = calls.load(Ordering::SeqCst);
    assert!(before > 0);

    for index in 0..12_u64 {
        session.inject_local("ordinary_in", index).unwrap();
        assert_eq!(session.run_until_idle(16).steps, 1);
    }
    assert_eq!(calls.load(Ordering::SeqCst), before);

    allowed.store(true, Ordering::SeqCst);
    assert!(notifier.notify());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(session.run_until_idle(16).steps, 1);
    assert!(calls.load(Ordering::SeqCst) > before);
    assert_eq!(session.state(), SessionState::Quiescent);
}

#[test]
fn process_notifier_does_not_resume_waitwake_but_wake_handle_does() {
    use std::time::Duration;

    let resumed = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let iface = Arc::new(Interface::new([PortDef::wire::<i64>("in").unwrap()], []).unwrap());
    let node = builder
        .node("waiter", iface, Arc::new({
            let resumed = resumed.clone();
            move || WaitOnWake {
                stage: 0,
                resumed: resumed.clone(),
            }
        }))
        .unwrap();
    builder
        .expose_input("input", node.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    let notifier = session.process_notifier("waiter").unwrap();
    let wake = session.wake_handle("waiter").unwrap();
    let mut listener = session.wake_listener();

    session.inject("input", Value::of(4_i64)).unwrap();
    assert_eq!(session.run_until_idle(16).steps, 1);
    assert!(notifier.notify());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(session.run_until_idle(16).steps, 0);
    assert_eq!(resumed.load(Ordering::SeqCst), 0);

    assert!(wake.wake());
    assert_eq!(session.run_until_idle(16).steps, 1);
    assert_eq!(resumed.load(Ordering::SeqCst), 1);
}

#[test]
fn generic_notifier_still_drives_opted_in_custom_policy() {
    let allowed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let sink = builder
        .add("custom", Arc::new(ComponentDef::sink(|_: &u64| {})))
        .unwrap();
    builder
        .expose_input("input", sink.input::<u64>("input").unwrap(), one())
        .unwrap();
    let mut options = SessionOptions::default();
    options.readiness.insert(
        "custom".into(),
        Readiness::Custom(Arc::new(NotifiedPolicy {
            calls: calls.clone(),
            allowed: allowed.clone(),
        })),
    );
    let mut session = builder.build().unwrap().compile().unwrap().start_with_options(options);
    session.inject_local("input", 1_u64).unwrap();
    assert_eq!(session.run_until_idle(16).steps, 0);
    allowed.store(true, Ordering::SeqCst);
    assert!(session.notifier().notify());
    assert_eq!(session.run_until_idle(16).steps, 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

struct PanicNotificationContract;
impl ReadinessPolicy for PanicNotificationContract {
    fn select(&self, _: &ReadinessSnapshot) -> Vec<String> {
        vec![]
    }
    fn notification_driven(&self) -> bool {
        panic!("policy registration failed")
    }
}
#[test]
fn notification_contract_panic_fails_session_instead_of_escaping_start() {
    let mut builder = NetworkBuilder::new();
    let sink = builder
        .add("custom", Arc::new(ComponentDef::sink(|_: &u64| {})))
        .unwrap();
    builder
        .expose_input("input", sink.input::<u64>("input").unwrap(), one())
        .unwrap();
    let mut options = SessionOptions::default();
    options.readiness.insert(
        "custom".into(),
        Readiness::Custom(Arc::new(PanicNotificationContract)),
    );
    let session = builder.build().unwrap().compile().unwrap().start_with_options(options);
    assert_eq!(session.state(), SessionState::Failed);
    assert!(
        session
            .failure()
            .unwrap()
            .contains("readiness notification contract panicked")
    );
}
