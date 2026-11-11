//! Capacity-one split/join with persistent state and a finite feedback loop.
use phenix_graph::{
    Value,
    component::ComponentDef,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{Readiness, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy)]
struct Packet {
    value: i64,
    remaining: u8,
    stop: bool,
}

struct Split {
    right: Option<Packet>,
    emitted: bool,
}
impl Process for Split {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if let Some(packet) = self.right.take() {
            return Ok(Step::Emit(ProcessEmission::new(
                "right",
                Value::local(packet),
            )));
        }
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        let packet = *value.borrow::<Packet>().unwrap();
        self.right = Some(Packet {
            value: packet.value * 10,
            ..packet
        });
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "left",
            Value::local(packet),
        )))
    }
}

struct Join {
    total: i64,
    emitted: bool,
}
impl Process for Join {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(left) = cx.take("left") else {
            return Ok(Step::Finished);
        };
        let right = cx.take("right").ok_or("join requires both branches")?;
        let packet = *left.borrow::<Packet>().unwrap();
        if !packet.stop {
            self.total += packet.value + right.borrow::<Packet>().unwrap().value;
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "output",
            Value::local(Packet {
                value: self.total,
                ..packet
            }),
        )))
    }
}

struct Feedback {
    emitted: bool,
}
impl Process for Feedback {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("input") else {
            return Ok(Step::Finished);
        };
        let mut packet = *value.borrow::<Packet>().unwrap();
        // Explicit in-band shutdown breaks the cycle's otherwise circular EOS dependency.
        if packet.stop {
            return Ok(Step::Finished);
        }
        self.emitted = true;
        if packet.remaining == 0 {
            Ok(Step::Emit(ProcessEmission::new(
                "result",
                Value::local(packet.value),
            )))
        } else {
            packet.remaining -= 1;
            Ok(Step::Emit(ProcessEmission::new(
                "again",
                Value::local(packet),
            )))
        }
    }
}

fn main() {
    let capacity = NonZeroUsize::new(1).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let collected = seen.clone();
    let mut b = NetworkBuilder::new();
    let split = b
        .node(
            "split",
            Arc::new(
                Interface::new(
                    [PortDef::local::<Packet>("input")],
                    [
                        PortDef::local::<Packet>("left"),
                        PortDef::local::<Packet>("right"),
                    ],
                )
                .unwrap(),
            ),
            Arc::new(|| Split {
                right: None,
                emitted: false,
            }),
        )
        .unwrap();
    let join = b
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::local::<Packet>("left"),
                        PortDef::local::<Packet>("right"),
                    ],
                    [PortDef::local::<Packet>("output")],
                )
                .unwrap(),
            ),
            Arc::new(|| Join {
                total: 0,
                emitted: false,
            }),
        )
        .unwrap();
    let feedback = b
        .node(
            "feedback",
            Arc::new(
                Interface::new(
                    [PortDef::local::<Packet>("input")],
                    [
                        PortDef::local::<Packet>("again"),
                        PortDef::local::<i64>("result"),
                    ],
                )
                .unwrap(),
            ),
            Arc::new(|| Feedback { emitted: false }),
        )
        .unwrap();
    let sink = b
        .add(
            "sink",
            Arc::new(ComponentDef::sink(move |n: &i64| {
                println!("{n}");
                collected.lock().unwrap().push(*n);
            })),
        )
        .unwrap();
    for branch in ["left", "right"] {
        b.connect(
            split.output::<Packet>(branch).unwrap(),
            join.input::<Packet>(branch).unwrap(),
            capacity,
        )
        .unwrap();
    }
    b.connect(
        join.output::<Packet>("output").unwrap(),
        feedback.input::<Packet>("input").unwrap(),
        capacity,
    )
    .unwrap();
    b.connect(
        feedback.output::<Packet>("again").unwrap(),
        split.input::<Packet>("input").unwrap(),
        capacity,
    )
    .unwrap();
    b.connect(
        feedback.output::<i64>("result").unwrap(),
        sink.input::<i64>("input").unwrap(),
        capacity,
    )
    .unwrap();
    b.expose_input("input", split.input::<Packet>("input").unwrap(), capacity)
        .unwrap();
    let mut options = SessionOptions::default();
    // Any would reserve just one branch, not a matched split pair.
    options.readiness.insert("join".into(), Readiness::All);
    let mut session = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);
    for value in [1, 2] {
        session
            .inject(
                "input",
                Value::local(Packet {
                    value,
                    remaining: 1,
                    stop: false,
                }),
            )
            .unwrap();
        let report = session.run_until_idle(128);
        assert_eq!(report.state, SessionState::Quiescent);
        assert!(!report.budget_exhausted);
        assert_eq!(session.run_until_idle(8).steps, 0);
    }
    assert_eq!(*seen.lock().unwrap(), [132, 1848]);
    session
        .inject(
            "input",
            Value::local(Packet {
                value: 0,
                remaining: 0,
                stop: true,
            }),
        )
        .unwrap();
    session.close_input("input").unwrap();
    assert_eq!(
        session.run_until_complete(128).state,
        SessionState::Completed
    );
    assert_eq!(*seen.lock().unwrap(), [132, 1848]);
}

#[test]
fn finite_feedback_and_shutdown() {
    main();
}
