//! FIFO and atomic per-emission publication against production transport.
use phenix_graph::connection::*;
use phenix_graph::{GraphOrigin, TypeIdentity, Value};
use std::any::TypeId;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn queue(capacity: usize) -> ConnectionQueue {
    ConnectionQueue::new(
        NonZeroUsize::new(capacity).unwrap(),
        Value::of(0_i64).identity(),
    )
}
fn packet(value: i64) -> Packet {
    Packet::new(Value::of(value))
}
fn publish(set: &mut ConnectionSet, value: i64, destinations: &[usize]) {
    let targets: Vec<_> = destinations
        .iter()
        .copied()
        .map(FanoutTarget::identity)
        .collect();
    let mut transaction = set.prepare(&packet(value), &targets).unwrap();
    set.commit(&mut transaction).unwrap();
}

#[test]
fn capacity_one_full_returns_exact_payload_without_overwriting_fifo() {
    let mut queue = queue(1);
    queue.push(packet(1)).unwrap();
    let (error, retained) = queue.push(packet(2)).unwrap_err();
    assert_eq!(error, ConnectionError::Full);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.head().unwrap().value().get::<i64>().unwrap(), 1);
    assert_eq!(queue.pop().unwrap().value().get::<i64>().unwrap(), 1);
    queue.push(retained).unwrap();
    assert_eq!(queue.pop().unwrap().into_value().get::<i64>().unwrap(), 2);
}

#[test]
fn capacity_n_preserves_fifo_under_repeated_full_drain_resume() {
    for capacity in [1, 2, 3, 8, 31] {
        let mut queue = queue(capacity);
        for round in 0..20 {
            for offset in 0..capacity {
                queue
                    .push(packet((round * capacity + offset) as i64))
                    .unwrap();
            }
            assert!(queue.is_full());
            let (error, retained) = queue.push(packet(-1)).unwrap_err();
            assert_eq!(error, ConnectionError::Full);
            assert_eq!(retained.value().get::<i64>().unwrap(), -1);
            for offset in 0..capacity {
                assert_eq!(
                    queue.pop().unwrap().value().get::<i64>().unwrap(),
                    (round * capacity + offset) as i64
                );
            }
            assert!(queue.is_empty());
        }
    }
}

#[test]
fn writer_close_preserves_queued_values_and_eos_requires_drain() {
    let mut queue = queue(2);
    queue.push(packet(1)).unwrap();
    queue.push(packet(2)).unwrap();
    queue.close_writer();
    queue.close_writer();
    assert!(!queue.is_eos());
    assert_eq!(
        queue.push(packet(3)).unwrap_err().0,
        ConnectionError::WriterClosed
    );
    assert_eq!(queue.pop().unwrap().value().get::<i64>().unwrap(), 1);
    assert!(!queue.is_eos());
    assert_eq!(queue.pop().unwrap().value().get::<i64>().unwrap(), 2);
    assert!(queue.is_eos());
}

#[test]
fn open_empty_connection_is_not_eos() {
    let mut queue = queue(1);
    assert!(queue.is_empty());
    assert!(!queue.is_eos());
    assert!(queue.pop().is_none());
    queue.close_writer();
    assert!(queue.is_eos());
}

#[test]
fn early_reader_finish_returns_accepted_undelivered_packets() {
    let mut queue = queue(2);
    queue.push(packet(10)).unwrap();
    queue.push(packet(20)).unwrap();
    let abandoned = queue.close_reader();
    assert_eq!(abandoned.len(), 2);
    assert_eq!(abandoned[0].value().get::<i64>().unwrap(), 10);
    assert_eq!(abandoned[1].value().get::<i64>().unwrap(), 20);
    assert!(queue.reader_closed());
    assert!(queue.close_reader().is_empty());
    assert_eq!(
        queue.push(packet(30)).unwrap_err().0,
        ConnectionError::ReaderClosed
    );
}

#[test]
fn identity_rejects_local_wire_coercion_and_forged_diagnostic_labels() {
    let mut queue = queue(2);
    assert_eq!(
        queue.push(Packet::new(Value::local(1_i64))).unwrap_err().0,
        ConnectionError::WrongType
    );
    let forged = Value::from_wire_parts("not-the-i64-schema".into(), 1_i64.to_le_bytes().to_vec());
    assert_eq!(
        queue.push(Packet::new(forged)).unwrap_err().0,
        ConnectionError::WrongType
    );
    assert!(queue.is_empty());
    queue.push(packet(1)).unwrap();
}

#[test]
fn fanout_waits_atomically_when_one_recipient_is_full() {
    let mut set = ConnectionSet::new(vec![queue(1), queue(1)]);
    publish(&mut set, 1, &[0]);
    let mut transaction = set
        .prepare(
            &packet(2),
            &[FanoutTarget::identity(0), FanoutTarget::identity(1)],
        )
        .unwrap();
    assert_eq!(set.commit(&mut transaction), Err(ConnectionError::Full));
    assert_eq!(transaction.retained_packets(), 2);
    assert_eq!(set.queues()[0].len(), 1);
    assert_eq!(set.queues()[1].len(), 0);
    assert_eq!(set.pop(0).unwrap().value().get::<i64>().unwrap(), 1);
    set.commit(&mut transaction).unwrap();
    assert!(transaction.is_committed());
    assert_eq!(transaction.retained_packets(), 0);
    assert_eq!(set.pop(0).unwrap().value().get::<i64>().unwrap(), 2);
    assert_eq!(set.pop(1).unwrap().value().get::<i64>().unwrap(), 2);
}

#[test]
fn blocked_retries_never_repeat_preparation_callbacks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapter_calls = calls.clone();
    let adapter: Arc<dyn EdgeAdapter> = Arc::new(move |packet: &Value| {
        adapter_calls.fetch_add(1, Ordering::SeqCst);
        Ok(packet.clone())
    });
    let mut set = ConnectionSet::new(vec![queue(1), queue(1)]);
    publish(&mut set, 1, &[0]);
    let targets = [
        FanoutTarget {
            connection: 0,
            preparation: Some(adapter.clone()),
        },
        FanoutTarget {
            connection: 1,
            preparation: Some(adapter),
        },
    ];
    let mut transaction = set.prepare(&packet(2), &targets).unwrap();
    for _ in 0..20 {
        assert_eq!(set.commit(&mut transaction), Err(ConnectionError::Full));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    set.pop(0).unwrap();
    set.commit(&mut transaction).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn partial_progress_across_emissions_does_not_roll_back_prior_emission() {
    let mut set = ConnectionSet::new(vec![queue(1), queue(1)]);
    publish(&mut set, 10, &[0, 1]);
    let mut second = set
        .prepare(
            &packet(20),
            &[FanoutTarget::identity(0), FanoutTarget::identity(1)],
        )
        .unwrap();
    set.pop(0).unwrap();
    assert_eq!(set.commit(&mut second), Err(ConnectionError::Full));
    assert_eq!(set.queues()[0].len(), 0);
    assert_eq!(
        set.queues()[1]
            .head()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        10
    );
    set.pop(1).unwrap();
    set.commit(&mut second).unwrap();
    assert_eq!(set.pop(0).unwrap().value().get::<i64>().unwrap(), 20);
    assert_eq!(set.pop(1).unwrap().value().get::<i64>().unwrap(), 20);
}

#[test]
fn closed_recipient_fails_whole_fanout_even_when_another_is_full() {
    for closed in [0, 1] {
        let full = 1 - closed;
        let mut set = ConnectionSet::new(vec![queue(1), queue(1)]);
        publish(&mut set, 1, &[full]);
        let mut transaction = set
            .prepare(
                &packet(2),
                &[FanoutTarget::identity(0), FanoutTarget::identity(1)],
            )
            .unwrap();
        set.close_reader(closed).unwrap();
        assert_eq!(
            set.commit(&mut transaction),
            Err(ConnectionError::ReaderClosed)
        );
        assert_eq!(set.queues()[full].len(), 1);
        assert_eq!(set.queues()[closed].len(), 0);
        assert_eq!(transaction.retained_packets(), 2);
    }
}

#[test]
fn writer_closed_between_prepare_and_commit_fails_without_partial_delivery() {
    let mut set = ConnectionSet::new(vec![queue(1), queue(1)]);
    let mut transaction = set
        .prepare(
            &packet(2),
            &[FanoutTarget::identity(0), FanoutTarget::identity(1)],
        )
        .unwrap();
    set.close_writer(1).unwrap();
    assert_eq!(
        set.commit(&mut transaction),
        Err(ConnectionError::WriterClosed)
    );
    assert!(set.queues().iter().all(ConnectionQueue::is_empty));
}

#[test]
fn preparation_error_and_panic_cannot_partially_deliver() {
    for panics in [false, true] {
        let failing: Arc<dyn EdgeAdapter> = Arc::new(move |_: &Value| {
            assert!(!panics, "adapter panic");
            Err("adapter rejected value".into())
        });
        let set = ConnectionSet::new(vec![queue(1), queue(1)]);
        let result = set.prepare(
            &packet(2),
            &[
                FanoutTarget::identity(0),
                FanoutTarget {
                    connection: 1,
                    preparation: Some(failing),
                },
            ],
        );
        assert!(matches!(result, Err(PrepareError::Adapter(_))));
        assert!(set.queues().iter().all(ConnectionQueue::is_empty));
    }
}

#[test]
fn duplicate_and_invalid_selection_reject_before_any_preparation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapter_calls = calls.clone();
    let adapter: Arc<dyn EdgeAdapter> = Arc::new(move |packet: &Value| {
        adapter_calls.fetch_add(1, Ordering::SeqCst);
        Ok(packet.clone())
    });
    let set = ConnectionSet::new(vec![queue(1)]);
    for (second, expected) in [
        (0, ConnectionError::DuplicateDestination),
        (1, ConnectionError::InvalidDestination),
    ] {
        let error = set
            .prepare(
                &packet(1),
                &[
                    FanoutTarget {
                        connection: 0,
                        preparation: Some(adapter.clone()),
                    },
                    FanoutTarget::identity(second),
                ],
            )
            .unwrap_err();
        assert_eq!(error, PrepareError::Connection(expected));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn adapter_output_identity_is_validated_before_any_mutation() {
    let set = ConnectionSet::new(vec![queue(1), queue(1)]);
    let invalid: Arc<dyn EdgeAdapter> = Arc::new(|_: &Value| Ok(Value::local(2_i64)));
    let error = set
        .prepare(
            &packet(1),
            &[
                FanoutTarget::identity(0),
                FanoutTarget {
                    connection: 1,
                    preparation: Some(invalid),
                },
            ],
        )
        .unwrap_err();
    assert_eq!(error, PrepareError::Connection(ConnectionError::WrongType));
    assert!(set.queues().iter().all(ConnectionQueue::is_empty));
}

#[test]
fn transaction_is_session_topology_bound_and_cannot_commit_twice() {
    let mut first = ConnectionSet::new(vec![queue(1)]);
    let mut second = ConnectionSet::new(vec![queue(1)]);
    let mut transaction = first
        .prepare(&packet(1), &[FanoutTarget::identity(0)])
        .unwrap();
    assert_eq!(
        second.commit(&mut transaction),
        Err(ConnectionError::ForeignTopology)
    );
    assert!(second.queues()[0].is_empty());
    first.commit(&mut transaction).unwrap();
    assert_eq!(
        first.commit(&mut transaction),
        Err(ConnectionError::AlreadyCommitted)
    );
    assert_eq!(first.queues()[0].len(), 1);
}

#[test]
fn empty_selection_is_a_successful_publication_without_delivery() {
    let mut set = ConnectionSet::new(vec![]);
    let mut transaction = set.prepare(&packet(1), &[]).unwrap();
    assert!(!transaction.is_committed());
    assert_eq!(transaction.retained_packets(), 0);
    set.commit(&mut transaction).unwrap();
    assert!(transaction.is_committed());
    assert_eq!(
        set.commit(&mut transaction),
        Err(ConnectionError::AlreadyCommitted)
    );
}

#[test]
fn local_move_only_fanout_shares_arcs_and_preserves_exact_origin() {
    struct MoveOnly(u64);
    let identity = TypeIdentity::Local(TypeId::of::<MoveOnly>());
    let mut set = ConnectionSet::new(vec![
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), identity.clone()),
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), identity),
    ]);
    let origin = GraphOrigin::default();
    let packet = Packet::new(Value::local(MoveOnly(19))).with_origin(origin.clone());
    let mut transaction = set
        .prepare(
            &packet,
            &[FanoutTarget::identity(0), FanoutTarget::identity(1)],
        )
        .unwrap();
    set.commit(&mut transaction).unwrap();
    let a = set.pop(0).unwrap();
    let b = set.pop(1).unwrap();
    assert_eq!(a.origin.as_ref(), Some(&origin));
    assert_eq!(b.origin.as_ref(), Some(&origin));
    let a = a.value().shared::<MoveOnly>().unwrap();
    let b = b.into_value().into_shared::<MoveOnly>().unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.0, 19);
}

struct DropProbe(Arc<AtomicUsize>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn leased_values_block_all_bare_extraction_but_preserve_unleased_apis() {
    let lease = Arc::new(());
    let local = Value::local(19_i64).with_lease(lease.clone());
    for error in [
        local.shared::<i64>().unwrap_err(),
        local.clone().into_shared::<i64>().unwrap_err(),
        local.clone().into_owned::<i64>().unwrap_err(),
        local.get::<i64>().unwrap_err(),
        Value::of(19_i64)
            .with_lease(lease)
            .get::<i64>()
            .unwrap_err(),
    ] {
        assert!(error.contains("leased value"));
        assert!(error.contains("leased_shared"));
    }
    assert_eq!(local.borrow::<i64>(), Some(&19));
    assert_eq!(*local.leased_shared::<i64>().unwrap(), 19);
    assert_eq!(*Value::local(19_i64).shared::<i64>().unwrap(), 19);
    assert_eq!(*Value::local(19_i64).into_shared::<i64>().unwrap(), 19);
    assert_eq!(Value::local(19_i64).into_owned::<i64>().unwrap(), 19);
    assert_eq!(Value::of(19_i64).get::<i64>().unwrap(), 19);
}

#[test]
fn lease_aware_decoding_and_cloning_keep_ownership_until_last_typed_owner() {
    let drops = Arc::new(AtomicUsize::new(0));
    let packet = Packet::new(Value::of(42_i64)).with_lease(Arc::new(DropProbe(drops.clone())));
    let value = packet.value().clone();
    drop(packet);
    let decoded = value.decoded::<i64>().unwrap();
    let clone = decoded.clone();
    drop(value);
    drop(decoded);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(*clone, 42);
    drop(clone);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn captured_value_clone_outlives_packet_and_real_session() {
    use phenix_graph::network::NetworkBuilder;
    use phenix_graph::process::{Process, ProcessContext, ProcessEmission, Step};
    use phenix_graph::runtime::SessionOptions;
    use phenix_graph::{Interface, PortDef};

    struct Source(bool);
    impl Process for Source {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            if std::mem::replace(&mut self.0, true) {
                Ok(Step::Finished)
            } else {
                Ok(Step::Emit(ProcessEmission::new(
                    "out",
                    Value::local(42_i64),
                )))
            }
        }
    }
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| Source(false)),
        )
        .unwrap();
    builder.start(&source).unwrap();
    builder
        .expose_output("output", source.output::<i64>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut session = compiled.start_with_options(SessionOptions {
        lease: Some(Arc::new(DropProbe(drops.clone()))),
        ..Default::default()
    });
    session.run_until_idle(100);
    let packet = session.take_output("output").unwrap().unwrap();
    let value = packet.value().clone();
    drop(packet);
    drop(session);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(value.borrow::<i64>(), Some(&42));
    assert!(value.shared::<i64>().is_err());
    let typed = value.leased_shared::<i64>().unwrap();
    drop(value);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(*typed, 42);
    drop(typed);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

struct PayloadBeforeLease {
    is_payload: bool,
    events: Arc<std::sync::Mutex<Vec<&'static str>>>,
}
impl Drop for PayloadBeforeLease {
    fn drop(&mut self) {
        self.events
            .lock()
            .unwrap()
            .push(if self.is_payload { "payload" } else { "lease" });
    }
}

#[test]
fn local_value_and_typed_clones_drop_payload_before_all_leases() {
    for typed in [false, true] {
        let events = Arc::new(std::sync::Mutex::new(vec![]));
        let packet = Packet::new(Value::local(PayloadBeforeLease {
            is_payload: true,
            events: events.clone(),
        }))
        .with_lease(Arc::new(PayloadBeforeLease {
            is_payload: false,
            events: events.clone(),
        }));
        let value = packet.value().clone();
        drop(packet.into_value());
        if typed {
            let shared = value.leased_shared::<PayloadBeforeLease>().unwrap();
            let clone = shared.clone();
            drop(value);
            drop(shared);
            assert!(events.lock().unwrap().is_empty());
            drop(clone);
        } else {
            let clone = value.clone();
            drop(value);
            assert!(events.lock().unwrap().is_empty());
            drop(clone);
        }
        assert_eq!(*events.lock().unwrap(), ["payload", "lease"]);
    }
}

#[test]
fn fanout_inherits_source_leases_and_preserves_adapter_output_leases() {
    let source_drops = Arc::new(AtomicUsize::new(0));
    let adapter_drops = Arc::new(AtomicUsize::new(0));
    let adapted_lease = adapter_drops.clone();
    let adapter: Arc<dyn EdgeAdapter> = Arc::new(move |value: &Value| {
        Ok(Value::local(*value.borrow::<i64>().unwrap() + 1)
            .with_lease(Arc::new(DropProbe(adapted_lease.clone()))))
    });
    let identity = TypeIdentity::Local(TypeId::of::<i64>());
    let mut set = ConnectionSet::new(vec![
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), identity.clone()),
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), identity),
    ]);
    let source =
        Packet::new(Value::local(42_i64)).with_lease(Arc::new(DropProbe(source_drops.clone())));
    let mut fanout = set
        .prepare(
            &source,
            &[
                FanoutTarget::identity(0),
                FanoutTarget {
                    connection: 1,
                    preparation: Some(adapter),
                },
            ],
        )
        .unwrap();
    drop(source);
    set.commit(&mut fanout).unwrap();
    let identity = set.pop(0).unwrap().into_value();
    let adapted = set.pop(1).unwrap().into_value();
    drop(set);
    drop(fanout);
    assert_eq!(source_drops.load(Ordering::SeqCst), 0);
    assert_eq!(adapter_drops.load(Ordering::SeqCst), 0);
    assert_eq!(identity.borrow::<i64>(), Some(&42));
    assert_eq!(adapted.borrow::<i64>(), Some(&43));
    assert!(identity.shared::<i64>().is_err());
    assert!(adapted.shared::<i64>().is_err());
    drop(identity);
    assert_eq!(source_drops.load(Ordering::SeqCst), 0);
    drop(adapted);
    assert_eq!(source_drops.load(Ordering::SeqCst), 1);
    assert_eq!(adapter_drops.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_and_blocked_transaction_disposal_drop_payload_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let identity = TypeIdentity::Local(TypeId::of::<DropProbe>());
    let mut set = ConnectionSet::new(vec![ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        identity,
    )]);
    let original = Packet::new(Value::local(DropProbe(drops.clone())));
    let targets = [FanoutTarget::identity(0)];
    let mut first = set.prepare(&original, &targets).unwrap();
    set.commit(&mut first).unwrap();
    let mut second = set.prepare(&original, &targets).unwrap();
    assert_eq!(set.commit(&mut second), Err(ConnectionError::Full));
    drop(original);
    let disposed = set.cancel();
    assert_eq!(disposed.len(), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(set.commit(&mut second), Err(ConnectionError::Cancelled));
    drop(disposed);
    drop(second);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(set.cancel().is_empty());
}

#[test]
fn pure_preparation_can_convert_wire_to_local_without_an_extra_codec() {
    let local = ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        TypeIdentity::Local(TypeId::of::<i64>()),
    );
    let mut set = ConnectionSet::new(vec![local]);
    let adapter: Arc<dyn EdgeAdapter> =
        Arc::new(|packet: &Value| Ok(Value::local(packet.get::<i64>()?)));
    let mut transaction = set
        .prepare(
            &packet(42),
            &[FanoutTarget {
                connection: 0,
                preparation: Some(adapter),
            }],
        )
        .unwrap();
    set.commit(&mut transaction).unwrap();
    assert_eq!(set.pop(0).unwrap().value().borrow::<i64>(), Some(&42));
}

#[test]
fn cancellation_invalidates_empty_publication_and_future_preparation() {
    let mut set = ConnectionSet::new(vec![]);
    let mut pending = set.prepare(&packet(1), &[]).unwrap();
    assert!(set.cancel().is_empty());
    assert_eq!(set.commit(&mut pending), Err(ConnectionError::Cancelled));
    assert!(!pending.is_committed());
    assert_eq!(
        set.prepare(&packet(2), &[]).unwrap_err(),
        PrepareError::Connection(ConnectionError::Cancelled)
    );
}

#[test]
fn known_closed_destinations_reject_before_running_adapters() {
    for reader in [true, false] {
        let calls = Arc::new(AtomicUsize::new(0));
        let adapter_calls = calls.clone();
        let adapter: Arc<dyn EdgeAdapter> = Arc::new(move |value: &Value| {
            adapter_calls.fetch_add(1, Ordering::SeqCst);
            Ok(value.clone())
        });
        let mut set = ConnectionSet::new(vec![queue(1), queue(1)]);
        let expected = if reader {
            set.close_reader(1).unwrap();
            ConnectionError::ReaderClosed
        } else {
            set.close_writer(1).unwrap();
            ConnectionError::WriterClosed
        };
        let error = set
            .prepare(
                &packet(1),
                &[
                    FanoutTarget {
                        connection: 0,
                        preparation: Some(adapter),
                    },
                    FanoutTarget::identity(1),
                ],
            )
            .unwrap_err();
        assert_eq!(error, PrepareError::Connection(expected));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(set.queues().iter().all(ConnectionQueue::is_empty));
    }
}

#[test]
fn failed_preparation_drops_prior_prepared_values_exactly_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let adapter_drops = drops.clone();
    let first: Arc<dyn EdgeAdapter> =
        Arc::new(move |_: &Value| Ok(Value::local(DropProbe(adapter_drops.clone()))));
    let fail: Arc<dyn EdgeAdapter> = Arc::new(|_: &Value| Err("later destination failed".into()));
    let identity = TypeIdentity::Local(TypeId::of::<DropProbe>());
    let set = ConnectionSet::new(vec![
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), identity.clone()),
        ConnectionQueue::new(NonZeroUsize::new(1).unwrap(), identity),
    ]);
    let result = set.prepare(
        &packet(1),
        &[
            FanoutTarget {
                connection: 0,
                preparation: Some(first),
            },
            FanoutTarget {
                connection: 1,
                preparation: Some(fail),
            },
        ],
    );
    assert!(matches!(result, Err(PrepareError::Adapter(_))));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(set.queues().iter().all(ConnectionQueue::is_empty));
}

#[test]
fn randomized_fanout_and_fifo_operations_match_independent_bounded_reference() {
    use std::collections::VecDeque;
    fn next(random: &mut u64) -> usize {
        *random ^= *random << 13;
        *random ^= *random >> 7;
        *random ^= *random << 17;
        *random as usize
    }
    for seed in 1..=64_u64 {
        let mut random = seed;
        let capacities = [1, 2, 3];
        let mut set = ConnectionSet::new(capacities.into_iter().map(queue).collect());
        let mut reference: [VecDeque<i64>; 3] = std::array::from_fn(|_| VecDeque::new());
        let mut closed = [false; 3];
        for operation in 0..512_i64 {
            let choice = next(&mut random);
            if choice.is_multiple_of(3) {
                let index = next(&mut random) % 3;
                let actual = set
                    .pop(index)
                    .map(|packet| packet.value().get::<i64>().unwrap());
                assert_eq!(
                    actual,
                    reference[index].pop_front(),
                    "seed={seed} operation={operation}"
                );
            } else if operation > 480 && choice.is_multiple_of(11) {
                let index = next(&mut random) % 3;
                set.close_writer(index).unwrap();
                closed[index] = true;
            } else {
                let mask = next(&mut random) % 8;
                let selected: Vec<_> = (0..3).filter(|index| mask & (1 << index) != 0).collect();
                let targets: Vec<_> = selected
                    .iter()
                    .copied()
                    .map(FanoutTarget::identity)
                    .collect();
                let closed_destination = selected.iter().any(|index| closed[*index]);
                let full_destination = selected
                    .iter()
                    .any(|index| reference[*index].len() == capacities[*index]);
                let result = set.prepare(&packet(operation), &targets);
                if closed_destination {
                    assert!(matches!(
                        result,
                        Err(PrepareError::Connection(ConnectionError::WriterClosed))
                    ));
                } else {
                    let mut transaction = result.unwrap();
                    let result = set.commit(&mut transaction);
                    if full_destination {
                        assert_eq!(result, Err(ConnectionError::Full));
                    } else {
                        result.unwrap();
                        for index in selected {
                            reference[index].push_back(operation);
                        }
                    }
                }
            }
            for index in 0..3 {
                let actual = &set.queues()[index];
                assert_eq!(
                    actual.len(),
                    reference[index].len(),
                    "seed={seed} operation={operation} queue={index}"
                );
                assert_eq!(
                    actual
                        .head()
                        .map(|packet| packet.value().get::<i64>().unwrap()),
                    reference[index].front().copied()
                );
                assert_eq!(
                    actual.is_eos(),
                    closed[index] && reference[index].is_empty()
                );
            }
        }
    }
}
