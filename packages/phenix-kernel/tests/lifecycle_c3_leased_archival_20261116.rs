mod support;

use phenix_graph::{
    Value,
    compose::{Edit, Footprint, NetworkPatch},
    network::NetworkError,
    process::{HandoffError, LocalProcessDispatcher},
    runtime::{RequestError, RequestStatus},
};
use phenix_kernel::{BuildOptions, Contribution, Key, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Revision {
    increment: i64,
    drops: Arc<AtomicUsize>,
}

impl Drop for Revision {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl PluginInstance for Revision {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let increment = self.increment;
        Contribution::new(
            "worker",
            NetworkPatch::new([
                Edit::Add {
                    path: "worker".into(),
                    definition: support::definition(false, true, increment, "test/archival"),
                },
                Edit::ExposeInput {
                    name: "input".into(),
                    targets: vec![support::path("worker", "run")],
                    capacity: support::capacity(),
                },
                Edit::ExposeOutput {
                    name: "output".into(),
                    from: support::path("worker", "value"),
                },
            ]),
        )
        .register(into)
    }
}

#[test]
fn archival_requests_cloned_sessions_and_jobs_keep_authority_only_in_their_owner() {
    for reload in [false, true] {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("native", |plugin: Revision| Ok(plugin))
            .unwrap();
        let mut host = KernelHost::new(catalog.instance());
        let application = Arc::new(());
        let application_lease = Arc::downgrade(&application);
        let options = || {
            let mut options = BuildOptions::default();
            options.execution.lease = Some(application.clone());
            support::grant(
                &mut options,
                Key::new("native", "worker"),
                [
                    Footprint::Component("worker".into()),
                    Footprint::InputExport("input".into()),
                    Footprint::OutputExport("output".into()),
                ],
            );
            options
        };
        let stage = host
            .plugins()
            .prepare_load(
                &key,
                Revision {
                    increment: 10,
                    drops: drops.clone(),
                },
            )
            .unwrap();
        let candidate = host
            .prepare(
                vec![stage],
                options().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "old",
                |_| Ok(()),
            )
            .unwrap();
        let old = host.publish(candidate).unwrap();
        let compiled_before = old.compiled().clone();
        let (request, job) = old
            .with_session(|session| {
                let request = session.begin_request(Some("output")).unwrap();
                session
                    .inject_request_local(request, "input", 1_i64)
                    .unwrap();
                session.close_request(request).unwrap();
                (request, session.next_job().unwrap().1)
            })
            .unwrap();
        let ticket = job.ticket();
        let old_session = job.metadata().unwrap().session;

        let stages = if reload {
            vec![
                host.plugins()
                    .prepare_reload(
                        &key,
                        Revision {
                            increment: 100,
                            drops: drops.clone(),
                        },
                    )
                    .unwrap(),
            ]
        } else {
            vec![]
        };
        let candidate = host
            .prepare(
                stages,
                options().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "new",
                |_| Ok(()),
            )
            .unwrap();
        let current = host.publish(candidate).unwrap();
        drop(application);
        assert_eq!((old.epoch(), current.epoch()), (1, 2));
        assert!(old.drain(1).unwrap().is_empty());
        let compiled_after = old.compiled().clone();
        {
            let bound = compiled_before
                .bind("worker", &support::contract("test/archival"))
                .unwrap();
            let input = bound.input::<i64>("run").unwrap();
            let output = bound.output::<i64>("value").unwrap();
            assert!(input.resolve(&compiled_after).is_ok());
            assert!(output.resolve(&compiled_after).is_ok());
            assert!(matches!(
                input.resolve(current.compiled()),
                Err(NetworkError::ForeignScope)
            ));
            assert!(matches!(
                output.resolve(current.compiled()),
                Err(NetworkError::ForeignScope)
            ));
        }
        let mut detached = compiled_before.start();
        let detached_request = detached.begin_request(Some("output")).unwrap();
        assert_eq!(detached_request.sequence(), request.sequence());
        assert_eq!(detached.request_status(request), Err(RequestError::Foreign));

        let completion = job.run();
        let completion = current
            .with_session(|session| {
                let current_request = session.begin_request(Some("output")).unwrap();
                assert_eq!(current_request.sequence(), request.sequence());
                assert_eq!(session.request_status(request), Err(RequestError::Foreign));
                let rejected = session
                    .inject_request(request, "input", Value::local(99_i64))
                    .unwrap_err();
                assert_eq!(rejected.reason, RequestError::Foreign);
                assert_eq!(rejected.value.borrow::<i64>(), Some(&99));
                let (error, returned) = session.complete_job(completion).unwrap_err();
                assert_eq!(error, HandoffError::ForeignCompletion);
                assert_eq!(returned.ticket(), ticket);
                session
                    .inject_request_local(current_request, "input", 2_i64)
                    .unwrap();
                session.close_request(current_request).unwrap();
                returned
            })
            .unwrap();
        old.with_session(|session| session.complete_job(*completion).unwrap())
            .unwrap();
        old.run_steps(8).unwrap();
        let packet = old
            .with_session(|session| {
                let packet = session
                    .take_request_output(request, "output")
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    session.request_status(request),
                    Ok(RequestStatus::Settled { captures: 1 })
                );
                session.release_request(request).unwrap();
                assert_eq!(session.request_status(request), Err(RequestError::Stale));
                packet
            })
            .unwrap();
        assert_eq!(packet.value().borrow::<i64>(), Some(&11));
        assert_eq!(packet.metadata().unwrap().session(), old_session);
        current.run_steps(8).unwrap();
        let current_packet = current
            .with_session(|session| session.take_output("output").unwrap().unwrap())
            .unwrap();
        assert_eq!(
            current_packet.value().borrow::<i64>(),
            Some(&(if reload { 102 } else { 12 }))
        );
        assert_ne!(current_packet.metadata().unwrap().session(), old_session);

        // Starting from either clone after retirement is an explicit archival capability.
        detached
            .inject_request_local(detached_request, "input", 3_i64)
            .unwrap();
        detached.close_request(detached_request).unwrap();
        let (_, retained_job) = detached.next_job().unwrap();
        let mut started_after = compiled_after.start();
        started_after.inject_local("input", 4_i64).unwrap();
        started_after.run_until_idle(8);
        let detached_packet = started_after.take_output("output").unwrap().unwrap();
        assert_eq!(detached_packet.value().borrow::<i64>(), Some(&14));
        old.inject("input", Value::local(5_i64)).unwrap();
        old.run_steps(8).unwrap();
        let explicit_packet = old
            .with_session(|session| session.take_output("output").unwrap().unwrap())
            .unwrap();
        assert_eq!(explicit_packet.value().borrow::<i64>(), Some(&15));

        drop((packet, current_packet, detached_packet, explicit_packet));
        drop((
            compiled_before,
            compiled_after,
            detached,
            started_after,
            old,
            current,
            host,
        ));
        assert_eq!(drops.load(Ordering::SeqCst), usize::from(reload));
        assert!(application_lease.upgrade().is_some());
        let completion = retained_job.run();
        assert_eq!(drops.load(Ordering::SeqCst), usize::from(reload));
        drop(completion);
        assert_eq!(drops.load(Ordering::SeqCst), if reload { 2 } else { 1 });
        assert!(application_lease.upgrade().is_none());
    }
}
