use phenix_graph::{
    ComponentDef,
    network::{CompiledNetwork, NetworkBuilder},
    process::{ActivationJob, DispatchResult, HandoffError, ProcessDispatcher},
    runtime::{
        RequestError, RequestFailurePolicy, RequestQueuePolicy, RequestStatus, SessionOptions,
        SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn network(effects: &Arc<AtomicUsize>) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    for name in ["a", "b"] {
        let effects = effects.clone();
        let node = builder
            .add(
                name,
                Arc::new(ComponentDef::sink(move |_: &i64| {
                    effects.fetch_add(1, Ordering::SeqCst);
                })),
            )
            .unwrap();
        builder
            .expose_input(
                name,
                node.input::<i64>("input").unwrap(),
                NonZeroUsize::new(2).unwrap(),
            )
            .unwrap();
    }
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn timeout_recovery_and_late_returns_cover_both_queue_and_failure_policies() {
    for request_queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for request_failure_policy in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let effects = Arc::new(AtomicUsize::new(0));
            let mut session = network(&effects).start_with_options(SessionOptions {
                request_queue_policy,
                request_failure_policy,
                max_requests: NonZeroUsize::new(2).unwrap(),
                deferred_timeout: Some(Duration::from_secs(60)),
                ..Default::default()
            });
            let expired = session.begin_request(None).unwrap();
            let survivor = session.begin_request(None).unwrap();
            session.inject_request_local(expired, "a", 1_i64).unwrap();
            session.close_request(expired).unwrap();
            let (index, job) = session.next_job().unwrap();
            session.inject_request_local(survivor, "b", 2_i64).unwrap();
            session.close_request(survivor).unwrap();
            let now = Instant::now() + Duration::from_secs(120);
            let overdue = session.overdue_jobs(now);
            assert_eq!(overdue.len(), 1);
            assert_eq!(
                (overdue[0].process, overdue[0].ticket),
                (index, job.ticket())
            );
            assert_eq!(session.request_status(expired), Ok(RequestStatus::Pending));
            assert_eq!(session.request_work(expired).unwrap().dispatched, 1);
            assert_eq!(effects.load(Ordering::SeqCst), 0);
            assert_eq!(session.supervise_deferred(now), overdue);
            assert_eq!(session.request_status(expired), Ok(RequestStatus::Failed));
            assert_eq!(
                session.request_cancelled_jobs(expired).unwrap(),
                &[job.ticket()]
            );
            assert!(session.overdue_jobs(now).is_empty());
            let diagnostic = session
                .request_failure(expired)
                .map(|failure| failure.map(phenix_graph::Failure::summary))
                .unwrap()
                .unwrap()
                .to_owned();
            let ticket = job.ticket();
            let (error, returned) = session.complete_job(job.run()).unwrap_err();
            assert_eq!(returned.ticket(), ticket);
            assert_eq!(
                effects.load(Ordering::SeqCst),
                1,
                "timeout does not preempt the worker"
            );
            drop(returned);
            if request_failure_policy == RequestFailurePolicy::FailSession {
                assert_eq!(error, HandoffError::StaleCompletion);
                assert_eq!(session.state(), SessionState::Failed);
                assert_eq!(session.request_status(survivor), Ok(RequestStatus::Failed));
                session.cancel();
                assert_eq!(
                    session.failure().map(phenix_graph::Failure::summary),
                    Some(diagnostic.as_str())
                );
                assert_eq!(session.run_until_idle(8).steps, 0);
            } else {
                assert_eq!(error, HandoffError::ForeignCompletion);
                assert!(
                    session
                        .failure()
                        .map(phenix_graph::Failure::summary)
                        .is_none()
                );
                assert_eq!(session.request_status(survivor), Ok(RequestStatus::Pending));
                session.release_request(expired).unwrap();
                assert_eq!(session.request_status(expired), Err(RequestError::Stale));
                let reused = session.begin_request(None).unwrap();
                assert!(reused.sequence() > survivor.sequence());
                session.inject_request_local(reused, "a", 3_i64).unwrap();
                session.close_request(reused).unwrap();
                let report = session.run_until_idle(8);
                assert_eq!(report.next_deferred_timeout, None);
                assert_eq!(effects.load(Ordering::SeqCst), 3);
                for request in [survivor, reused] {
                    assert_eq!(
                        session.request_status(request),
                        Ok(RequestStatus::Settled { captures: 0 })
                    );
                    session.release_request(request).unwrap();
                }
            }
        }
    }
}

#[test]
fn multiple_deadlines_survive_foreign_returns_and_clear_only_the_validated_ticket() {
    let compiled = network(&Arc::new(AtomicUsize::new(0)));
    let options = SessionOptions {
        deferred_timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    };
    let mut session = compiled.start_with_options(options.clone());
    let mut foreign = compiled.start_with_options(options);
    session.inject_local("a", 1_i64).unwrap();
    session.inject_local("b", 2_i64).unwrap();
    foreign.inject_local("a", 3_i64).unwrap();
    let before = Instant::now();
    let (a, first) = session.next_job().unwrap();
    let after_first = Instant::now();
    let (b, second) = session.next_job().unwrap();
    let after_second = Instant::now();
    let (_, other) = foreign.next_job().unwrap();
    let future = after_second + Duration::from_secs(120);
    let overdue = session.overdue_jobs(future);
    assert_eq!(overdue.len(), 2);
    assert_eq!(
        overdue.iter().map(|job| job.ticket).collect::<Vec<_>>(),
        [first.ticket(), second.ticket()]
    );
    let (error, other) = session.reject(a, other).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    let (error, other) = session.complete(a, other.run()).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(session.overdue_jobs(future), overdue);
    foreign.complete_job(*other).unwrap();
    // Bound the first deadline by the instants around its handoff.
    assert_eq!(session.overdue_jobs(before).len(), 0);
    let first_due = after_first + Duration::from_secs(60);
    assert!(
        session
            .overdue_jobs(first_due)
            .iter()
            .any(|job| job.ticket == first.ticket())
    );
    let report_at = Instant::now();
    let remaining = session.run_until_idle(0).next_deferred_timeout.unwrap();
    let report_end = Instant::now();
    assert!(
        remaining <= (after_first + Duration::from_secs(60)).saturating_duration_since(report_at)
    );
    assert!(remaining >= (before + Duration::from_secs(60)).saturating_duration_since(report_end));
    session.complete(a, first.run()).unwrap();
    let remaining = session.overdue_jobs(future);
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        (remaining[0].process, remaining[0].ticket),
        (b, second.ticket())
    );
    let report_at = Instant::now();
    let remaining = session.run_until_idle(0).next_deferred_timeout.unwrap();
    let report_end = Instant::now();
    assert!(
        remaining <= (after_second + Duration::from_secs(60)).saturating_duration_since(report_at)
    );
    assert!(
        remaining >= (after_first + Duration::from_secs(60)).saturating_duration_since(report_end)
    );
    session.reject(b, second).unwrap();
    assert_eq!(session.run_until_idle(0).next_deferred_timeout, None);
}

#[test]
fn deferred_deadline_starts_after_synchronous_dispatch_returns_not_when_it_enters() {
    const TIMEOUT: Duration = Duration::from_millis(200);
    struct SlowHandoff {
        held: Mutex<Option<ActivationJob>>,
        window: Mutex<Option<(Instant, Instant)>>,
    }
    impl ProcessDispatcher for SlowHandoff {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            let entered = Instant::now();
            std::thread::sleep(TIMEOUT + Duration::from_millis(50));
            *self.held.lock().unwrap() = Some(job);
            *self.window.lock().unwrap() = Some((entered, Instant::now()));
            DispatchResult::Deferred
        }
    }
    let effects = Arc::new(AtomicUsize::new(0));
    let dispatcher = Arc::new(SlowHandoff {
        held: Mutex::new(None),
        window: Mutex::new(None),
    });
    let mut session = network(&effects).start_with_options(SessionOptions {
        dispatcher: dispatcher.clone(),
        deferred_timeout: Some(TIMEOUT),
        ..Default::default()
    });
    session.inject_local("a", 1_i64).unwrap();
    let report = session.run_until_idle(1);
    let (entered, returned) = dispatcher.window.lock().unwrap().unwrap();
    assert!(returned.duration_since(entered) >= TIMEOUT);
    assert_eq!(report.state, SessionState::Waiting);
    assert!(
        report
            .next_deferred_timeout
            .is_some_and(|remaining| remaining > Duration::ZERO)
    );
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    let job = dispatcher.held.lock().unwrap().take().unwrap();
    let overdue = session.overdue_jobs(returned + TIMEOUT + Duration::from_secs(1));
    assert_eq!(overdue.len(), 1);
    assert_eq!(overdue[0].ticket, job.ticket());
    // Inspection and an otherwise abandoned session do not apply the deadline.
    assert_eq!(session.state(), SessionState::Waiting);
    assert_eq!(
        session.supervise_deferred(returned + TIMEOUT + Duration::from_secs(1)),
        overdue
    );
    let (error, returned) = session.complete_job(job.run()).unwrap_err();
    assert_eq!(error, HandoffError::StaleCompletion);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    drop(returned);
}

#[test]
fn explicit_cancellation_before_supervision_wins_even_when_job_is_already_overdue() {
    for request_queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session =
            network(&Arc::new(AtomicUsize::new(0))).start_with_options(SessionOptions {
                request_queue_policy,
                request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
                deferred_timeout: Some(Duration::ZERO),
                ..Default::default()
            });
        let request = session.begin_request(None).unwrap();
        session.inject_request_local(request, "a", 1_i64).unwrap();
        session.close_request(request).unwrap();
        let (_, job) = session.next_job().unwrap();
        assert_eq!(session.overdue_jobs(Instant::now()).len(), 1);
        assert_eq!(session.cancel_request(request).unwrap(), [job.ticket()]);
        assert_eq!(
            session.request_status(request),
            Ok(RequestStatus::Cancelled)
        );
        assert!(session.supervise_deferred(Instant::now()).is_empty());
        let (error, returned) = session.complete_job(job.run()).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        drop(returned);
        assert_eq!(
            session.request_status(request),
            Ok(RequestStatus::Cancelled)
        );
        assert_eq!(
            session
                .request_failure(request)
                .map(|failure| failure.map(phenix_graph::Failure::summary)),
            Ok(None)
        );
    }
}
