//! Adapter definition ownership and contained final snapshot/typed-owner cleanup.
use phenix_graph::{
    CallbackPhase, Failure, FailureKind, GraphOrigin, InputKey, ProcessContext, Value, ValueError,
    connection::{
        AdapterRef, ConnectionError, ConnectionQueue, ConnectionSet, EdgeAdapter, FanoutTarget,
        Packet,
        PacketDestructorPanicked,
    },
};
use std::{
    error::Error,
    fmt,
    num::NonZeroUsize,
    sync::{Arc, Mutex, Weak},
};

type Events = Arc<Mutex<Vec<usize>>>;

#[derive(Debug)]
struct Witness {
    owners: Vec<Weak<()>>,
    events: Events,
    event: usize,
}
impl Witness {
    fn check(&self) {
        assert!(self.owners.iter().all(|owner| owner.upgrade().is_some()));
    }
}
impl Drop for Witness {
    fn drop(&mut self) {
        self.check();
        self.events.lock().unwrap().push(self.event);
    }
}

#[derive(Debug)]
struct Cause {
    witness: Option<Witness>,
    panic: bool,
}
impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.witness.as_ref().unwrap().check();
        f.write_str("adapter concrete cause")
    }
}
impl Error for Cause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.witness.as_ref().unwrap().check();
        None
    }
}
impl Drop for Cause {
    fn drop(&mut self) {
        let witness = self.witness.take().unwrap();
        witness.check();
        if self.panic {
            std::panic::panic_any(witness);
        }
        drop(witness);
    }
}

struct Input(Events);
impl Drop for Input {
    fn drop(&mut self) {
        self.0.lock().unwrap().push(0);
    }
}

fn queue(value: &Value) -> ConnectionQueue {
    ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), value.identity())
}

fn target(connection: usize, adapter: Arc<dyn EdgeAdapter>) -> FanoutTarget {
    FanoutTarget {
        connection,
        preparation: AdapterRef::Owned(adapter),
    }
}

#[test]
fn escaped_adapter_cause_and_secondary_pin_exact_definition_without_input_payload() {
    struct Adapter {
        _code: Arc<()>,
        failure: Mutex<Option<Failure>>,
    }
    impl EdgeAdapter for Adapter {
        fn prepare(&self, _: &Value) -> Result<Value, Failure> {
            Err(self.failure.lock().unwrap().take().unwrap())
        }
    }
    for (secondary, panic) in [(false, false), (true, false), (false, true), (true, true)] {
        let events = Events::default();
        let code = Arc::new(());
        let input = Arc::new(());
        let owners = vec![Arc::downgrade(&code), Arc::downgrade(&input)];
        let cause = |event| Cause {
            witness: Some(Witness {
                owners: owners.clone(),
                events: events.clone(),
                event,
            }),
            panic,
        };
        let failure = Failure::external("adapter primary", cause(1))
            .with_cleanup(Failure::external("adapter secondary", cause(2)));
        let address = failure.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause;
        let secondary_address = failure.cleanup()[0]
            .source()
            .unwrap()
            .downcast_ref::<Cause>()
            .unwrap() as *const Cause;
        let adapter = Arc::new(Adapter {
            _code: code,
            failure: Mutex::new(Some(failure)),
        });
        let definition = Arc::downgrade(&adapter);
        let targets = [target(0, adapter)];
        let packet = Packet::new(Value::local(Input(events.clone())).with_lease(input));
        let set = ConnectionSet::new(vec![queue(&Value::local(()))]);
        let failure = set.prepare(&packet, &targets).unwrap_err().into_failure();
        let escaped = if secondary {
            failure.cleanup()[0].clone()
        } else {
            failure.clone()
        };
        drop(targets);
        drop(packet);
        drop(set);
        drop(failure);
        assert_eq!(events.lock().unwrap()[0], 0, "input payload is not pinned");
        assert!(
            definition.upgrade().is_some(),
            "exact definition stays owned"
        );
        assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
        let source = escaped.source().unwrap();
        let concrete = source.downcast_ref::<Cause>().unwrap();
        assert_eq!(
            concrete as *const Cause,
            if secondary {
                secondary_address
            } else {
                address
            }
        );
        assert!(source.source().is_none());
        assert_eq!(format!("{source}"), "adapter concrete cause");
        let _ = format!("{source:?}");
        assert_eq!(escaped.dispose_catching(), panic);
        assert!(definition.upgrade().is_none());
        assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
        let mut recorded = events.lock().unwrap().clone();
        recorded.sort();
        assert_eq!(recorded, [0, 1, 2]);
    }
}

#[test]
fn adapter_panic_payload_cleanup_and_escaped_failure_keep_callback_and_input_leases() {
    let events = Events::default();
    let code = Arc::new(());
    let input = Arc::new(());
    let owners = vec![Arc::downgrade(&code), Arc::downgrade(&input)];
    let adapter: Arc<dyn EdgeAdapter> = Arc::new({
        let owners = owners.clone();
        let events = events.clone();
        move |_: &Value| {
            let _keep = &code;
            std::panic::panic_any(Witness {
                owners: owners.clone(),
                events: events.clone(),
                event: 1,
            })
        }
    });
    let definition = Arc::downgrade(&adapter);
    let targets = [target(0, adapter)];
    let packet = Packet::new(Value::local(Input(events.clone())).with_lease(input));
    let set = ConnectionSet::new(vec![queue(&Value::local(()))]);
    let failure = set.prepare(&packet, &targets).unwrap_err().into_failure();
    assert_eq!(*events.lock().unwrap(), [1]);
    assert_eq!(failure.kind(), FailureKind::Panic);
    assert_eq!(failure.context().phase, Some(CallbackPhase::Adapter));
    let escaped = failure.clone();
    drop(failure);
    drop(targets);
    drop(packet);
    drop(set);
    assert_eq!(*events.lock().unwrap(), [1, 0]);
    assert!(definition.upgrade().is_some());
    assert!(
        escaped
            .source()
            .unwrap()
            .is::<phenix_graph::process::CallbackPanicked>()
    );
    drop(escaped);
    assert!(definition.upgrade().is_none());
    assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
}

#[test]
fn local_success_pins_callback_definition_through_last_typed_payload_owner() {
    let events = Events::default();
    let code = Arc::new(());
    let input = Arc::new(());
    let owners = vec![Arc::downgrade(&code), Arc::downgrade(&input)];
    let adapter: Arc<dyn EdgeAdapter> = Arc::new({
        let owners = owners.clone();
        let events = events.clone();
        move |_: &Value| {
            let _keep = &code;
            Ok(Value::local(Witness {
                owners: owners.clone(),
                events: events.clone(),
                event: 1,
            }))
        }
    });
    let definition = Arc::downgrade(&adapter);
    let targets = [target(0, adapter)];
    let origin = GraphOrigin::default();
    let packet = Packet::new(Value::local(Input(events.clone())).with_lease(input))
        .with_origin(origin.clone());
    let mut set = ConnectionSet::new(vec![queue(&Value::local(Witness {
        owners: vec![],
        events: Events::default(),
        event: 99,
    }))]);
    let mut prepared = set.prepare(&packet, &targets).unwrap();
    set.commit(&mut prepared).unwrap();
    let output = set.pop(0).unwrap();
    assert_eq!(output.origin, Some(origin));
    assert!(output.value().shared::<Witness>().is_err());
    let typed = output.value().leased_shared::<Witness>().unwrap();
    assert!(std::ptr::eq(
        output.value().borrow::<Witness>().unwrap(),
        &*typed
    ));
    drop(output);
    drop(targets);
    drop(packet);
    drop(set);
    drop(prepared);
    assert_eq!(*events.lock().unwrap(), [0]);
    assert!(definition.upgrade().is_some());
    typed.check();
    assert!(!typed.dispose_catching());
    assert_eq!(*events.lock().unwrap(), [0, 1]);
    assert!(definition.upgrade().is_none());
    assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
}

#[test]
fn portable_success_does_not_pin_callback_definition_or_restrict_wire_access() {
    let code = Arc::new(());
    let weak = Arc::downgrade(&code);
    let adapter: Arc<dyn EdgeAdapter> = Arc::new(move |_: &Value| {
        let _keep = &code;
        Ok(Value::of(42_i64))
    });
    let targets = [target(0, adapter)];
    let mut set = ConnectionSet::new(vec![queue(&Value::of(0_i64))]);
    let mut prepared = set
        .prepare(&Packet::new(Value::local(())), &targets)
        .unwrap();
    drop(targets);
    assert!(weak.upgrade().is_none());
    set.commit(&mut prepared).unwrap();
    let output = set.pop(0).unwrap();
    assert_eq!(output.value().bytes(), 42_i64.to_le_bytes());
    assert_eq!(output.value().get::<i64>().unwrap(), 42);
}

struct PanickingLease(Option<Witness>);
impl Drop for PanickingLease {
    fn drop(&mut self) {
        let witness = self.0.take().unwrap();
        witness.check();
        witness.events.lock().unwrap().push(witness.event - 100);
        std::panic::panic_any(witness);
    }
}

struct Ordinary {
    leases: Vec<Weak<PanickingLease>>,
    witness: Witness,
}
impl Drop for Ordinary {
    fn drop(&mut self) {
        assert!(self.leases.iter().all(|lease| lease.upgrade().is_some()));
        self.witness.check();
    }
}

fn leased_output(
    count: usize,
    owners: &[Weak<()>],
    events: &Events,
) -> (Value, Vec<Weak<PanickingLease>>) {
    let leases: Vec<_> = (0..count)
        .map(|index| {
            Arc::new(PanickingLease(Some(Witness {
                owners: owners.to_vec(),
                events: events.clone(),
                event: 300 + index,
            })))
        })
        .collect();
    let weak = leases.iter().map(Arc::downgrade).collect::<Vec<_>>();
    let mut value = Value::local(Ordinary {
        leases: weak.clone(),
        witness: Witness {
            owners: owners.to_vec(),
            events: events.clone(),
            event: 100,
        },
    });
    for lease in leases {
        value = value.with_lease(lease);
    }
    (value, weak)
}

#[test]
fn wrong_type_and_partial_fanout_contain_every_final_snapshot_lease_release() {
    for partial in [false, true] {
        for count in [1, 3] {
            let code = Arc::new(());
            let input = Arc::new(());
            let owners = vec![Arc::downgrade(&code), Arc::downgrade(&input)];
            let events = Events::default();
            let (value, leases) = leased_output(count, &owners, &events);
            let identity = value.identity();
            let adapter: Arc<dyn EdgeAdapter> = Arc::new({
                let value = Mutex::new(Some(value));
                move |_: &Value| {
                    let _keep = &code;
                    Ok(value.lock().unwrap().take().unwrap())
                }
            });
            let definition = Arc::downgrade(&adapter);
            let mut targets = vec![target(0, adapter)];
            let primary = Failure::external(
                "later adapter primary",
                Cause {
                    witness: Some(Witness {
                        owners: owners.clone(),
                        events: events.clone(),
                        event: 1,
                    }),
                    panic: false,
                },
            );
            let address =
                primary.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause;
            let mut queues = vec![ConnectionQueue::new(
                NonZeroUsize::new(1).unwrap(),
                if partial {
                    identity.clone()
                } else {
                    Value::local(()).identity()
                },
            )];
            if partial {
                let primary = Mutex::new(Some(primary));
                targets.push(target(
                    1,
                    Arc::new(move |_: &Value| Err(primary.lock().unwrap().take().unwrap())),
                ));
                queues.push(ConnectionQueue::new(
                    NonZeroUsize::new(1).unwrap(),
                    identity,
                ));
            } else {
                drop(primary);
                events.lock().unwrap().clear();
            }
            let set = ConnectionSet::new(queues);
            let packet = Packet::new(Value::local(Input(events.clone())).with_lease(input));
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                set.prepare(&packet, &targets)
            }))
            .expect("preparation must return Err, not unwind")
            .unwrap_err()
            .into_failure();
            if partial {
                assert_eq!(failure.summary(), "later adapter primary");
                assert_eq!(
                    failure.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause,
                    address
                );
            } else {
                assert_eq!(
                    failure.source().unwrap().downcast_ref::<ConnectionError>(),
                    Some(&ConnectionError::WrongType)
                );
            }
            assert_eq!(failure.cleanup().len(), count);
            for cleanup in failure.cleanup() {
                assert_eq!(cleanup.kind(), FailureKind::Cleanup);
                assert_eq!(
                    cleanup
                        .source()
                        .unwrap()
                        .downcast_ref::<PacketDestructorPanicked>(),
                    Some(&PacketDestructorPanicked { connection: 0 })
                );
            }
            assert!(set.queues().iter().all(|queue| queue.is_empty()
                && queue.capacity() == 1
                && !queue.reader_closed()
                && !queue.writer_closed()
                && !queue.is_eos()));
            assert!(leases.iter().all(|lease| lease.upgrade().is_none()));
            let mut expected = vec![100];
            for index in (0..count).rev() {
                expected.extend([200 + index, 300 + index]);
            }
            assert_eq!(*events.lock().unwrap(), expected);
            let cleanup = failure.cleanup()[0].clone();
            drop(targets);
            drop(packet);
            drop(set);
            drop(failure);
            assert!(definition.upgrade().is_some());
            assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
            drop(cleanup);
            assert!(definition.upgrade().is_none());
            assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
        }
    }
}

#[test]
fn failed_source_cleanup_contains_final_leases_of_invalidated_successful_value() {
    struct Source {
        original: bool,
        witness: Option<Witness>,
    }
    impl Clone for Source {
        fn clone(&self) -> Self {
            Self {
                original: false,
                witness: None,
            }
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            if self.original {
                std::panic::panic_any(self.witness.take().unwrap());
            }
        }
    }
    for count in [1, 3] {
        let code = Arc::new(());
        let owners = vec![Arc::downgrade(&code)];
        let events = Events::default();
        let source = Value::local(Source {
            original: true,
            witness: Some(Witness {
                owners: owners.clone(),
                events: events.clone(),
                event: 50,
            }),
        })
        .with_lease(code);
        let mut context = ProcessContext::new([("in".into(), source)].into(), vec![]);
        let (output, leases) = leased_output(count, &owners, &events);
        let failure = InputKey::<Source>::new("in")
            .map_shared_input(&mut context, "", |_, _| Ok(output))
            .unwrap_err();
        assert!(context.input("in").is_none());
        assert!(matches!(
            failure.source().unwrap().downcast_ref::<ValueError>(),
            Some(ValueError::DestructorPanicked { .. })
        ));
        assert_eq!(failure.cleanup().len(), count);
        for cleanup in failure.cleanup() {
            assert_eq!(cleanup.kind(), FailureKind::Cleanup);
            assert!(matches!(
                cleanup.source().unwrap().downcast_ref::<ValueError>(),
                Some(ValueError::DestructorPanicked { .. })
            ));
        }
        let mut expected = vec![50, 100];
        for index in (0..count).rev() {
            expected.extend([200 + index, 300 + index]);
        }
        assert_eq!(*events.lock().unwrap(), expected);
        assert!(leases.iter().all(|lease| lease.upgrade().is_none()));
        drop(context);
        assert!(owners[0].upgrade().is_some());
        drop(failure);
        assert!(owners[0].upgrade().is_none());
    }
}

#[test]
fn leased_failure_reports_actual_contained_cause_panic_without_cloning_or_detaching() {
    for shared in [false, true] {
        let events = Events::default();
        let code = Arc::new(());
        let envelope = Arc::new(());
        let owners = vec![Arc::downgrade(&code), Arc::downgrade(&envelope)];
        let failure = Failure::external(
            "exact failure",
            Cause {
                witness: Some(Witness {
                    owners: owners.clone(),
                    events: events.clone(),
                    event: 1,
                }),
                panic: true,
            },
        )
        .with_lease(code);
        let address = failure.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause;
        let source = Value::local(failure).with_lease(envelope);
        let managed = source.leased_shared::<Failure>().unwrap();
        assert!(std::ptr::eq(source.borrow::<Failure>().unwrap(), &*managed));
        assert_eq!(
            managed.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause,
            address
        );
        let other = shared.then(|| managed.clone());
        assert!(
            !source.dispose_catching(),
            "shared payload cleanup is deferred"
        );
        assert!(events.lock().unwrap().is_empty());
        assert_eq!(managed.dispose_catching(), !shared);
        if let Some(other) = other {
            assert!(events.lock().unwrap().is_empty());
            assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
            assert_eq!(
                other.source().unwrap().downcast_ref::<Cause>().unwrap() as *const Cause,
                address
            );
            assert!(other.dispose_catching());
        }
        assert_eq!(*events.lock().unwrap(), [1]);
        assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
    }
}

#[test]
fn unrelated_leased_payloads_keep_the_same_managed_cleanup_contract() {
    for panics in [false, true] {
        struct Payload(Option<Witness>, bool);
        impl Drop for Payload {
            fn drop(&mut self) {
                let witness = self.0.take().unwrap();
                if self.1 {
                    std::panic::panic_any(witness);
                }
                drop(witness);
            }
        }
        let events = Events::default();
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let source = Value::local(Payload(
            Some(Witness {
                owners: vec![weak.clone()],
                events: events.clone(),
                event: 1,
            }),
            panics,
        ))
        .with_lease(lease);
        let managed = source.leased_shared::<Payload>().unwrap();
        drop(source);
        assert_eq!(managed.dispose_catching(), panics);
        assert_eq!(*events.lock().unwrap(), [1]);
        assert!(weak.upgrade().is_none());
    }
}
