//! Packet-driven dirty scheduling must preserve bounded capture progress,
//! EOS propagation and externally delivered wakeups.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
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

    session.inject_local("active_in", 1_i64).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 2);
    session.inject_local("active_in", 2_i64).unwrap();
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
    session.inject_local("input", 42_i64).unwrap();
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(resumed.load(Ordering::SeqCst), 0);
    assert_eq!(session.state(), SessionState::Waiting);
    assert!(session.wake_handle("waker").unwrap().wake());
    assert_eq!(session.run_until_idle(32).steps, 1);
    assert_eq!(resumed.load(Ordering::SeqCst), 1);
}
