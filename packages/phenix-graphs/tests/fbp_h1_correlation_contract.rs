//! H1 bounded request-correlation contract, exercised through the public
//! compiled network, scoped admission, cancellation and capture APIs.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{
        Readiness, RequestFailurePolicy, RequestQueuePolicy, RequestStatus, SessionOptions,
        SessionState,
    },
};
use std::{num::NonZeroUsize, sync::Arc};

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(8).unwrap()
}

#[derive(Default)]
enum Phase {
    #[default]
    Start,
    WaitForTail(i64),
    AfterEmission,
}

struct Join {
    staged: bool,
    phase: Phase,
}

impl Process for Join {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        match std::mem::take(&mut self.phase) {
            Phase::Start => {
                if cx.is_eos("left") || cx.is_eos("right") {
                    return Ok(Step::Finished);
                }
                let left = cx.take("left").expect("correlated left packet");
                let right = cx.take("right").expect("correlated right packet");
                let sum = *left.borrow::<i64>().unwrap() + *right.borrow::<i64>().unwrap();
                if self.staged {
                    self.phase = Phase::WaitForTail(sum);
                    Ok(Step::WaitInput("right".into()))
                } else {
                    self.phase = Phase::AfterEmission;
                    Ok(Step::Emit(ProcessEmission::new("out", Value::local(sum))))
                }
            }
            Phase::WaitForTail(sum) => {
                let right = cx.take("right").expect("authenticated staged tail");
                self.phase = Phase::AfterEmission;
                Ok(Step::Emit(ProcessEmission::new(
                    "out",
                    Value::local(sum + *right.borrow::<i64>().unwrap()),
                )))
            }
            Phase::AfterEmission => Ok(Step::Complete),
        }
    }
}

fn network(staged: bool) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let join = builder
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("left"), PortDef::local::<i64>("right")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(move || Join {
                staged,
                phase: Phase::Start,
            }),
        )
        .unwrap();
    builder
        .expose_input("left", join.input::<i64>("left").unwrap(), capacity())
        .unwrap();
    builder
        .expose_input("right", join.input::<i64>("right").unwrap(), capacity())
        .unwrap();
    builder
        .expose_output("out", join.output::<i64>("out").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

fn options(queue_policy: RequestQueuePolicy) -> SessionOptions {
    let mut options = SessionOptions {
        max_requests: NonZeroUsize::new(2).unwrap(),
        capture_capacity: capacity(),
        request_queue_policy: queue_policy,
        ..Default::default()
    };
    options.readiness.insert("join".into(), Readiness::CorrelatedAll);
    options
}

fn captured(
    session: &mut phenix_graph::runtime::NetworkSession,
    id: phenix_graph::runtime::RequestId,
) -> Vec<i64> {
    let mut values = Vec::new();
    while let Some(packet) = session.take_request_output(id, "out").unwrap() {
        assert_eq!(packet.metadata().unwrap().request(), Some(id));
        values.push(*packet.value().borrow::<i64>().unwrap());
    }
    values
}

#[test]
fn crossed_multiplicity_preserves_request_local_fifo_and_settles_both_scopes() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session = network(false).start_with_options(options(policy));
        let a = session.begin_request(None).unwrap();
        let b = session.begin_request(None).unwrap();

        // Left A,A,B,B crosses right B,B,A,A. Every admission is bounded:
        // capacity 8, two admitted scopes, no hidden holding queue.
        for (id, port, value) in [
            (a, "left", 10_i64),
            (a, "left", 11),
            (b, "right", 2),
            (b, "right", 3),
            (b, "left", 20),
            (b, "left", 21),
            (a, "right", 1),
            (a, "right", 4),
        ] {
            session.inject_request_local(id, port, value).unwrap();
        }
        session.close_request(a).unwrap();
        session.close_request(b).unwrap();
        // Global EOS must not overtake already accepted crossed packets.
        // Closing both writers still permits the two correlated pairs each.
        session.close_input("left").unwrap();
        session.close_input("right").unwrap();
        let report = session.run_until_idle(64);
        assert!(!report.budget_exhausted, "{policy:?}");
        assert_eq!(captured(&mut session, a), [11, 15], "{policy:?}");
        assert_eq!(captured(&mut session, b), [22, 24], "{policy:?}");
        for request in [a, b] {
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Settled { captures: 2 }
            );
            session.release_request(request).unwrap();
        }
        assert!(session.failure().is_none(), "{policy:?}");
    }
}

#[test]
fn correlated_staged_wait_skips_only_foreign_heads_for_its_current_owner() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session = network(true).start_with_options(options(policy));
        let a = session.begin_request(None).unwrap();
        let b = session.begin_request(None).unwrap();
        // The foreign right head (B) precedes both of A's right packets.
        // CorrelatedAll atomically reserves A's initial pair, then A's
        // WaitInput("right") must also take A's next packet, not B's head.
        session.inject_request_local(b, "right", 200_i64).unwrap();
        session.inject_request_local(a, "left", 10_i64).unwrap();
        session.inject_request_local(a, "right", 1_i64).unwrap();
        session.inject_request_local(a, "right", 2_i64).unwrap();
        session.close_request(a).unwrap();

        let first = session.run_until_idle(32);
        assert!(!first.budget_exhausted, "{policy:?}");
        assert_eq!(captured(&mut session, a), [13], "{policy:?}");
        assert_eq!(
            session.request_status(a).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        assert_eq!(session.request_work(b).unwrap().queued, 1);
        session.release_request(a).unwrap();

        session.inject_request_local(b, "left", 20_i64).unwrap();
        session.inject_request_local(b, "right", 201_i64).unwrap();
        session.close_request(b).unwrap();
        let second = session.run_until_idle(32);
        assert!(!second.budget_exhausted, "{policy:?}");
        assert_eq!(captured(&mut session, b), [421], "{policy:?}");
        assert_eq!(
            session.request_status(b).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        session.release_request(b).unwrap();
        assert!(session.failure().is_none(), "{policy:?}");
    }
}

#[test]
fn unscoped_head_is_not_reordered_as_a_correlated_request() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session = network(false).start_with_options(options(policy));
        let a = session.begin_request(None).unwrap();
        // An unscoped head cannot be taken as A's right packet. A may still
        // bypass it because A has explicitly opted into correlated selection.
        session.inject("right", Value::local(900_i64)).unwrap();
        session.inject_request_local(a, "left", 10_i64).unwrap();
        session.inject_request_local(a, "right", 1_i64).unwrap();
        session.close_request(a).unwrap();
        let report = session.run_until_idle(32);
        assert!(!report.budget_exhausted);
        assert_eq!(captured(&mut session, a), [11]);
        session.release_request(a).unwrap();

        // The unscoped right packet must still be available to its own
        // head-only global join, with no request authority attached.
        session.inject("left", Value::local(90_i64)).unwrap();
        let report = session.run_until_idle(32);
        assert!(!report.budget_exhausted);
        let packet = session.take_output("out").unwrap().unwrap();
        assert_eq!(packet.metadata().unwrap().request(), None);
        assert_eq!(packet.value().borrow::<i64>(), Some(&990));
        assert!(session.take_output("out").unwrap().is_none());
        assert!(session.failure().is_none(), "{policy:?}");
    }
}

#[test]
fn cancelled_staged_owner_releases_peer_without_losing_its_packets() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut options = options(policy);
        options.request_failure_policy = RequestFailurePolicy::RestartAffectedProcess;
        let mut session = network(true).start_with_options(options);
        let a = session.begin_request(None).unwrap();
        let b = session.begin_request(None).unwrap();

        session.inject_request_local(a, "left", 10_i64).unwrap();
        session.inject_request_local(a, "right", 1_i64).unwrap();
        assert_eq!(session.run_until_idle(1).steps, 1);
        session.inject_request_local(b, "left", 20_i64).unwrap();
        session.inject_request_local(b, "right", 2_i64).unwrap();
        session.inject_request_local(b, "right", 3_i64).unwrap();
        session.close_request(b).unwrap();
        assert_eq!(session.run_until_idle(8).steps, 0);
        assert_eq!(session.request_status(b).unwrap(), RequestStatus::Pending);

        session.cancel_request(a).unwrap();
        assert_eq!(session.request_status(a).unwrap(), RequestStatus::Cancelled);
        session.release_request(a).unwrap();
        let report = session.run_until_idle(32);
        assert!(!report.budget_exhausted);
        assert_eq!(captured(&mut session, b), [25], "{policy:?}");
        assert_eq!(
            session.request_status(b).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        session.release_request(b).unwrap();
        session.close_input("left").unwrap();
        session.close_input("right").unwrap();
        session.run_until_idle(16);
        assert_ne!(session.state(), SessionState::Failed);
        assert!(session.failure().is_none(), "{policy:?}");
    }
}
