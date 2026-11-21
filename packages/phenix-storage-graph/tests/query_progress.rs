mod support;

use phenix_graph::{
    InputKey,
    network::NetworkBuilder,
    process::{ActivationJob, DispatchResult, LocalProcessDispatcher, ProcessDispatcher},
    runtime::{RequestError, RequestQueuePolicy, RequestStatus},
};
use phenix_storage_graph::derived::*;
use std::sync::{Arc, Mutex};
use support::*;

#[test]
fn draining_blocked_observations_is_progress_even_without_process_steps() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for fill_ingress in [false, true] {
            let f = Fixture::new(false);
            f.put(&f.a, 3);
            let mut graph = NetworkBuilder::new();
            let group = graph.sources("values", (&f.a,)).unwrap();
            let number = graph
                .derive_from("number", &group, |a: Number| a.0)
                .unwrap();
            let text = graph
                .derive_from("text", &group, |a: Number| a.0.to_string())
                .unwrap();
            graph.expose_output("text", text.output()).unwrap();
            graph.expose_output("text.alias", text.output()).unwrap();
            graph
                .expose_output("number.alias", number.output())
                .unwrap();
            let spec = graph.query_spec("query", &number).unwrap();
            let plugin = authored(graph, spec.clone(), None);
            let mut config = options(&plugin);
            config.execution.request_queue_policy = queue;
            config.execution.capture_capacity = capacity();
            let mut runtime = configured(plugin, Arc::new(LocalProcessDispatcher), config);
            runtime.start().unwrap();
            let compiled = runtime.kernel().compiled();
            let ingress = compiled
                .bind_ingress("values.refresh", InputKey::new("run"))
                .unwrap();
            let captures: Vec<_> = compiled
                .outputs()
                .keys()
                .map(|name| compiled.capture(name).unwrap())
                .collect();
            runtime
                .kernel()
                .with_session(|session| {
                    // Two observations before driving leave a full capture and a
                    // retained publication, rather than merely a queued new input.
                    for _ in 0..2 {
                        session
                            .inject_bound(&ingress, RefreshCause::Resynchronize)
                            .unwrap();
                    }
                    assert!(session.run_until_idle(256).steps > 0);
                    assert_eq!(session.run_until_idle(256).steps, 0);
                    if fill_ingress {
                        let full = (0..4).any(|_| {
                            session
                                .inject_bound(&ingress, RefreshCause::Resynchronize)
                                .is_err_and(|error| {
                                    error.reason == phenix_graph::connection::ConnectionError::Full
                                })
                        });
                        assert!(full);
                    }
                })
                .unwrap();
            f.put(&f.a, 7);
            let query = runtime.bind_query(&spec).unwrap();
            assert_eq!(
                *query.query_with_budget(256).unwrap(),
                7,
                "{queue:?}, fill_ingress={fill_ingress}"
            );
            runtime
                .kernel()
                .with_session(|session| {
                    for capture in &captures {
                        assert!(
                            session.take_bound_output(capture).unwrap().is_none(),
                            "{}",
                            capture.name()
                        );
                    }
                })
                .unwrap();
        }
    }
}

#[test]
fn capture_relief_does_not_replenish_an_exhausted_step_budget() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for budget in [0, 1] {
            let f = Fixture::new(false);
            f.put(&f.a, 3);
            f.put(&f.b, 5);
            let (plugin, spec) = sum(&f, false);
            let mut config = options(&plugin);
            config.execution.request_queue_policy = queue;
            config.execution.capture_capacity = capacity();
            let mut runtime = configured(plugin, Arc::new(LocalProcessDispatcher), config);
            runtime.start().unwrap();
            let ingress = runtime
                .kernel()
                .compiled()
                .bind_ingress("sum.sources.refresh", InputKey::new("run"))
                .unwrap();
            runtime
                .kernel()
                .with_session(|session| {
                    for _ in 0..2 {
                        session
                            .inject_bound(&ingress, RefreshCause::Resynchronize)
                            .unwrap();
                    }
                    session.run_until_idle(256);
                    assert_eq!(session.run_until_idle(256).steps, 0);
                })
                .unwrap();
            let query = runtime.bind_query(&spec).unwrap();
            assert!(
                matches!(query.query_with_budget(budget), Err(Error::Unsettled)),
                "{queue:?}, budget={budget}"
            );
            // The exhausted attempt did not damage the owner or grant itself
            // a fresh budget just because it relieved old capture pressure.
            f.put(&f.a, 7);
            assert_eq!(*query.query_with_budget(256).unwrap(), 12);
        }
    }
}

#[derive(Default)]
struct HoldJob(Mutex<Option<ActivationJob>>);
impl ProcessDispatcher for HoldJob {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        let mut retained = self.0.lock().unwrap();
        assert!(
            retained.is_none(),
            "stalled query must not dispatch more work"
        );
        *retained = Some(job);
        DispatchResult::Deferred
    }
}

#[test]
fn no_progress_stops_with_unused_budget_and_zero_budget_never_dispatches() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for budget in [0, 256] {
            let f = Fixture::new(false);
            f.put(&f.a, 3);
            f.put(&f.b, 5);
            let (plugin, spec) = sum(&f, false);
            let mut config = options(&plugin);
            config.execution.request_queue_policy = queue;
            config.execution.capture_capacity = capacity();
            let dispatcher = Arc::new(HoldJob::default());
            let mut runtime = configured(plugin, dispatcher.clone(), config);
            runtime.start().unwrap();
            let error = runtime
                .bind_query(&spec)
                .unwrap()
                .query_with_budget(budget)
                .unwrap_err();
            let job = dispatcher.0.lock().unwrap().take();
            if budget == 0 {
                assert!(matches!(error, Error::Unsettled));
                assert!(job.is_none());
            } else {
                let job = job.unwrap();
                let request = job.metadata().unwrap().request.unwrap();
                let Error::RequestInterrupted {
                    cause,
                    cancelled_jobs,
                    disposal,
                } = error
                else {
                    panic!("lost bounded no-progress cause or job inventory: {error:?}");
                };
                assert!(matches!(*cause, Error::Unsettled));
                assert_eq!(cancelled_jobs, [job.ticket()]);
                assert_eq!(disposal.handed_jobs, 1);
                runtime
                    .kernel()
                    .with_session(|session| {
                        assert_eq!(session.request_status(request), Err(RequestError::Stale));
                        assert!(session.complete_job(job.run()).is_err());
                    })
                    .unwrap();
            }
        }
    }
}

#[derive(Default)]
struct HoldPeer(Mutex<Option<ActivationJob>>, std::sync::atomic::AtomicBool);
impl ProcessDispatcher for HoldPeer {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        if job
            .metadata()
            .is_some_and(|metadata| metadata.path == "held")
            && !self.1.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            *self.0.lock().unwrap() = Some(job);
            return DispatchResult::Deferred;
        }
        DispatchResult::Completed(job.run())
    }
}

#[test]
fn retained_other_lane_blocks_query_without_donating_captures_or_job_authority() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    let mut graph = NetworkBuilder::new();
    let group = graph.sources("values", (&f.a,)).unwrap();
    let number = graph
        .derive_from("number", &group, |a: Number| a.0)
        .unwrap();
    let held = graph
        .derive_from("held", &group, |a: Number| a.0.to_string())
        .unwrap();
    graph.expose_output("held", held.output()).unwrap();
    graph
        .expose_output("number.alias", number.output())
        .unwrap();
    let spec = graph.query_spec("query", &number).unwrap();
    let plugin = authored(graph, spec.clone(), None);
    let mut config = options(&plugin);
    config.execution.request_queue_policy = RequestQueuePolicy::BoundedLanes;
    config.execution.capture_capacity = capacity();
    let dispatcher = Arc::new(HoldPeer::default());
    let mut runtime = configured(plugin, dispatcher.clone(), config);
    runtime.start().unwrap();
    let compiled = runtime.kernel().compiled();
    let ingress = compiled
        .bind_ingress("values.refresh", InputKey::new("run"))
        .unwrap();
    let captures: Vec<_> = compiled
        .outputs()
        .keys()
        .map(|name| compiled.capture(name).unwrap())
        .collect();
    let (old, before, disposal) = runtime
        .kernel()
        .with_session(|session| {
            let old = session.begin_bound_request(None).unwrap();
            for _ in 0..2 {
                session
                    .inject_bound_request(old, &ingress, RefreshCause::Demand)
                    .unwrap();
            }
            session.close_request(old).unwrap();
            session.run_until_idle(256);
            let work = session.request_work(old).unwrap();
            assert!(work.publication_pending > 0);
            assert!(work.capture_backpressure > 0);
            assert_eq!(work.dispatched, 1);
            assert!(session.request_failure(old).unwrap().is_none());
            (old, work, session.request_disposal(old).unwrap())
        })
        .unwrap();
    let ticket = dispatcher.0.lock().unwrap().as_ref().unwrap().ticket();
    f.put(&f.a, 7);
    let query = runtime.bind_query(&spec).unwrap();
    // H7: shared process instances can remain occupied by another lane's
    // retained publication. A5 does not grant permission to steal that lane.
    assert!(matches!(
        query.query_with_budget(256),
        Err(Error::Unsettled)
    ));
    runtime
        .kernel()
        .with_session(|session| {
            assert_eq!(session.request_status(old).unwrap(), RequestStatus::Pending);
            assert_eq!(session.request_work(old).unwrap(), before);
            assert_eq!(session.request_disposal(old).unwrap(), disposal);
            assert!(session.request_failure(old).unwrap().is_none());
            assert!(session.request_cancelled_jobs(old).unwrap().is_empty());
            assert_eq!(
                dispatcher.0.lock().unwrap().as_ref().unwrap().ticket(),
                ticket
            );
            let job = dispatcher.0.lock().unwrap().take().unwrap();
            assert_eq!(job.metadata().unwrap().request, Some(old));
            session.complete_job(job.run()).unwrap();
        })
        .unwrap();
    // No deferred worker remains: retained scoped captures alone still hold
    // the shared process. Only the old owner may relieve that pressure.
    assert!(matches!(
        query.query_with_budget(256),
        Err(Error::Unsettled)
    ));
    runtime
        .kernel()
        .with_session(|session| {
            assert!(session.request_work(old).unwrap().capture_backpressure > 0);
            assert_eq!(session.request_work(old).unwrap().dispatched, 0);
            assert!(session.request_failure(old).unwrap().is_none());
            assert!(session.request_cancelled_jobs(old).unwrap().is_empty());
            let mut counts = vec![0; captures.len()];
            for _ in 0..32 {
                session.run_until_idle(256);
                for (capture, count) in captures.iter().zip(&mut counts) {
                    while let Some(packet) =
                        session.take_bound_request_output(old, capture).unwrap()
                    {
                        session
                            .validate_bound_capture(old, capture, &packet)
                            .unwrap();
                        if capture.name() == "query" {
                            assert_eq!(packet.value().borrow::<i64>(), Some(&3));
                        }
                        *count += 1;
                    }
                }
                if matches!(
                    session.request_status(old).unwrap(),
                    RequestStatus::Settled { .. }
                ) {
                    break;
                }
            }
            for (capture, count) in captures.iter().zip(counts) {
                assert_eq!(
                    count,
                    if capture.name() == "values.errors" {
                        0
                    } else {
                        2
                    },
                    "{}",
                    capture.name()
                );
            }
            session.release_request(old).unwrap();
            assert_eq!(session.request_status(old), Err(RequestError::Stale));
        })
        .unwrap();
    assert_eq!(*query.query_with_budget(256).unwrap(), 7);
}

#[test]
fn shared_fifo_demand_preserves_another_live_requests_captures() {
    let fixture = Fixture::new(false);
    fixture.put(&fixture.a, 3);
    let mut graph = NetworkBuilder::new();
    let group = graph.sources("values", (&fixture.a,)).unwrap();
    let number = graph
        .derive_from("number", &group, |a: Number| a.0)
        .unwrap();
    let text = graph
        .derive_from("text", &group, |a: Number| a.0.to_string())
        .unwrap();
    graph.expose_output("text", text.output()).unwrap();
    graph
        .expose_output("number.alias", number.output())
        .unwrap();
    let spec = graph.query_spec("query", &number).unwrap();
    let plugin = authored(graph, spec.clone(), None);
    let mut configuration = options(&plugin);
    configuration.execution.request_queue_policy = RequestQueuePolicy::SharedFifo;
    configuration.execution.capture_capacity = capacity();
    let mut runtime = configured(plugin, Arc::new(LocalProcessDispatcher), configuration);
    runtime.start().unwrap();

    let compiled = runtime.kernel().compiled();
    let ingress = compiled
        .bind_ingress("values.refresh", InputKey::new("run"))
        .unwrap();
    let captures: Vec<_> = compiled
        .outputs()
        .keys()
        .map(|name| compiled.capture(name).unwrap())
        .collect();
    let old = runtime
        .kernel()
        .with_session(|session| {
            let old = session.begin_bound_request(None).unwrap();
            session
                .inject_bound_request(old, &ingress, RefreshCause::Demand)
                .unwrap();
            session.close_request(old).unwrap();
            session.run_until_idle(256);
            assert!(session.request_work(old).unwrap().queued > 0);
            old
        })
        .unwrap();

    fixture.put(&fixture.a, 7);
    let query = runtime.bind_query(&spec).unwrap();
    assert!(matches!(query.query_with_budget(256), Err(Error::Unsettled)));
    runtime
        .kernel()
        .with_session(|session| {
            // The demand may not relieve another live owner's capture pressure.
            assert_eq!(session.request_status(old).unwrap(), RequestStatus::Pending);
            let mut total = 0;
            let mut saw_query = false;
            for _ in 0..32 {
                session.run_until_idle(256);
                for capture in &captures {
                    while let Some(packet) =
                        session.take_bound_request_output(old, capture).unwrap()
                    {
                        session.validate_bound_capture(old, capture, &packet).unwrap();
                        assert_eq!(packet.metadata().unwrap().request(), Some(old));
                        if capture.name() == "query" {
                            assert_eq!(packet.value().borrow::<i64>(), Some(&3));
                            saw_query = true;
                        }
                        total += 1;
                    }
                }
                if matches!(
                    session.request_status(old).unwrap(),
                    RequestStatus::Settled { .. }
                ) {
                    break;
                }
            }
            assert!(total > 0);
            assert!(saw_query);
            assert!(matches!(
                session.request_status(old),
                Ok(RequestStatus::Settled { .. })
            ));
            session.release_request(old).unwrap();
        })
        .unwrap();
    assert_eq!(*query.query_with_budget(256).unwrap(), 7);
}
