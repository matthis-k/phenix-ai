mod support;

use phenix_graph::{
    InputKey, OutputKey,
    network::{NetworkBuilder, NetworkError},
    process::LocalProcessDispatcher,
    runtime::{RequestError, RequestFailurePolicy, RequestQueuePolicy, SessionState},
};
use phenix_kernel::{Key, host::LifecycleError};
use phenix_plugin::PluginCatalog;
use phenix_storage_graph::derived::*;
use std::sync::Arc;
use support::*;

#[test]
fn prepared_routes_adopt_only_the_exact_published_owner_and_keep_archival_authority() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (plugin, spec) = sum(&f, true);
        let config = || {
            let mut config = options(&plugin);
            config.execution.request_queue_policy = queue;
            config.execution.capture_capacity = capacity();
            config
        };
        let mut catalog = PluginCatalog::new();
        let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
        let mut host = DerivedHost::new(catalog.instance());
        let mut prepared = None;
        let candidate = host
            .prepare(
                vec![host.plugins().prepare_load(&kind, plugin.clone()).unwrap()],
                config(),
                Arc::new(LocalProcessDispatcher),
                "first",
                |binding| {
                    prepared = Some(spec.bind_preparation(binding)?);
                    Ok(())
                },
            )
            .unwrap();
        let first = host.publish(candidate).unwrap();
        let prepared = prepared.unwrap();
        first.with_kernel(|kernel| {
            assert_eq!(*prepared.bind_kernel(kernel).unwrap().query().unwrap(), 8);
        });

        // Even a binding which escaped a rejected candidate has no authority
        // over the still-active owner. Rejection must not adopt names afresh.
        let mut rejected = None;
        let failed = host.prepare(
            vec![],
            config(),
            Arc::new(LocalProcessDispatcher),
            "rejected",
            |binding| {
                rejected = Some(spec.bind_preparation(binding)?);
                Err("reject after resolution".into())
            },
        );
        assert!(matches!(failed, Err(LifecycleError::Binding(_))));
        assert_eq!(host.active().unwrap().epoch(), first.epoch());
        let rejected = rejected.unwrap();
        first.with_kernel(|kernel| {
            assert!(matches!(
                rejected.bind_kernel(kernel),
                Err(Error::Network(error)) if *error == NetworkError::ForeignScope
            ));
            assert_eq!(*prepared.bind_kernel(kernel).unwrap().query().unwrap(), 8);
        });

        // The exact same QuerySpec and final definitions are reusable, but the
        // independently compiled route bundle must have a distinct owner.
        let mut successor = None;
        let candidate = host
            .prepare(
                vec![],
                config(),
                Arc::new(LocalProcessDispatcher),
                "successor",
                |binding| {
                    let exported = binding
                        .export::<QuerySpec<i64>>(&Key::new("domain", "derive"))
                        .unwrap();
                    successor = Some(exported.bind_preparation(binding)?);
                    Ok(())
                },
            )
            .unwrap();
        let second = host.publish(candidate).unwrap();
        let successor = successor.unwrap();
        second.with_kernel(|kernel| {
            for foreign in [&prepared, &rejected] {
                assert!(matches!(
                    foreign.bind_kernel(kernel),
                    Err(Error::Network(error)) if *error == NetworkError::ForeignScope
                ));
            }
            assert_eq!(*successor.bind_kernel(kernel).unwrap().query().unwrap(), 8);
        });
        first.with_kernel(|kernel| {
            assert!(matches!(
                successor.bind_kernel(kernel),
                Err(Error::Network(error)) if *error == NetworkError::ForeignScope
            ));
        });
        assert!(first.drain(1).unwrap().is_empty());
        drop(host);
        f.put(&f.b, 9);
        first.with_kernel(|kernel| {
            assert_eq!(*prepared.bind_kernel(kernel).unwrap().query().unwrap(), 12);
        });
        second.with_kernel(|kernel| {
            assert_eq!(*successor.bind_kernel(kernel).unwrap().query().unwrap(), 12);
        });
    }
}

#[test]
fn capacity_one_queries_drain_older_heterogeneous_aliases_and_retry_full_ingress() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
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
        let ingress = runtime
            .kernel()
            .compiled()
            .bind_ingress("values.refresh", InputKey::new("run"))
            .unwrap();
        runtime
            .kernel()
            .with_session(|session| {
                session
                    .inject_bound(&ingress, RefreshCause::Resynchronize)
                    .unwrap();
                session.run_until_idle(128);
                // Readiness can reserve one activation before dispatch. Fill
                // that slot and the capacity-one canonical ingress without
                // driving either. SharedFifo's demand must take the Full retry.
                let full = (0..4).any(|_| {
                    session
                        .inject_bound(&ingress, RefreshCause::Resynchronize)
                        .is_err_and(|error| {
                            error.reason == phenix_graph::connection::ConnectionError::Full
                        })
                });
                assert!(full);
            })
            .unwrap();
        f.put(&f.a, 7);
        let query = runtime.bind_query(&spec).unwrap();
        assert_eq!(*query.query().unwrap(), 7, "{queue:?}");
        f.put(&f.a, 11);
        assert_eq!(*query.query().unwrap(), 11, "{queue:?}");
        runtime
            .kernel()
            .with_session(|session| {
                for name in runtime.kernel().compiled().outputs().keys() {
                    let capture = runtime.kernel().compiled().capture(name).unwrap();
                    assert!(
                        session.take_bound_output(&capture).unwrap().is_none(),
                        "{name}"
                    );
                }
            })
            .unwrap();
    }
}

#[test]
fn bound_capture_validation_keeps_alias_and_released_request_identity() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    let mut graph = NetworkBuilder::new();
    let number = graph.derive("number", (&f.a,), |a: Number| a.0).unwrap();
    let spec = graph.query_spec("query", &number).unwrap();
    let tap = graph.component("query").unwrap();
    graph
        .expose_output("alias", tap.output::<i64>("value").unwrap())
        .unwrap();
    let mut runtime = local(authored(graph, spec.clone(), None));
    runtime.start().unwrap();
    let compiled = runtime.kernel().compiled();
    let ingress = compiled
        .bind_ingress(number.refresh_route(), InputKey::new("run"))
        .unwrap();
    let selected = compiled
        .bind_capture("query", OutputKey::<i64>::new("value"))
        .unwrap();
    let alias = compiled
        .bind_capture("alias", OutputKey::<i64>::new("value"))
        .unwrap();
    runtime
        .kernel()
        .with_session(|session| {
            let request = session.begin_bound_request(None).unwrap();
            session
                .inject_bound_request(request, &ingress, RefreshCause::Demand)
                .unwrap();
            session.close_request(request).unwrap();
            session.run_until_idle(128);
            let packet = session
                .take_bound_request_output(request, &selected)
                .unwrap()
                .unwrap();
            session
                .validate_bound_capture(request, &selected, &packet)
                .unwrap();
            assert!(
                session
                    .validate_bound_capture(request, &alias, &packet)
                    .is_err()
            );
            for name in compiled.outputs().keys() {
                let capture = compiled.capture(name).unwrap();
                while session
                    .take_bound_request_output(request, &capture)
                    .unwrap()
                    .is_some()
                {}
            }
            session.release_request(request).unwrap();
            assert_eq!(
                session.validate_bound_capture(request, &selected, &packet),
                Err(RequestError::Stale)
            );
        })
        .unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 3);
}

#[test]
fn same_shaped_redirected_tap_root_and_error_captures_fail_both_resolvers() {
    for redirected in ["query", "number.sources.snapshot", "number.sources.errors"] {
        let f = Fixture::new(false);
        let mut child = NetworkBuilder::new();
        let number = child.derive("number", (&f.a,), |a: Number| a.0).unwrap();
        let spec = child.query_spec("query", &number).unwrap();
        let decoy = child.derive("decoy", (&f.a,), |a: Number| a.0).unwrap();
        child.query_spec("decoy.query", &decoy).unwrap();
        let mut graph = NetworkBuilder::new();
        let child = graph.module("child", child.build().unwrap()).unwrap();
        graph
            .expose_input(
                number.refresh_route(),
                child.input::<RefreshCause>(number.refresh_route()).unwrap(),
                capacity(),
            )
            .unwrap();
        for (name, decoy) in [
            ("query", "decoy.query"),
            ("number.sources.snapshot", "decoy.sources.snapshot"),
            ("number.sources.errors", "decoy.sources.errors"),
        ] {
            graph
                .expose_output_dynamic(
                    name,
                    &child
                        .output_endpoint(if name == redirected { decoy } else { name })
                        .unwrap(),
                )
                .unwrap();
        }
        let plugin = authored(graph, spec.clone(), None);
        let runtime = local(plugin.clone());
        assert!(
            matches!(runtime.bind_query(&spec), Err(Error::InvalidPublication)),
            "{redirected}"
        );
        let config = options(&plugin);
        let mut catalog = PluginCatalog::new();
        let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
        let host = DerivedHost::new(catalog.instance());
        let failed = host.prepare(
            vec![host.plugins().prepare_load(&kind, plugin).unwrap()],
            config,
            Arc::new(LocalProcessDispatcher),
            "redirected",
            |binding| {
                assert!(
                    matches!(
                        spec.bind_preparation(binding),
                        Err(Error::InvalidPublication)
                    ),
                    "{redirected}"
                );
                spec.validate_binding(binding).map_err(Into::into)
            },
        );
        assert!(matches!(failed, Err(LifecycleError::Binding(_))));
        assert!(host.active().is_none());
    }
}

#[test]
fn original_known_lease_survives_until_bindings_values_and_errors_all_drop() {
    for last in 0..3 {
        let f = Fixture::new(false);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (plugin, spec) = sum(&f, false);
        let mut config = options(&plugin);
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        config.execution.lease = Some(lease);
        let mut catalog = PluginCatalog::new();
        let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
        let mut host = DerivedHost::new(catalog.instance());
        let mut prepared = None;
        let candidate = host
            .prepare(
                vec![host.plugins().prepare_load(&kind, plugin).unwrap()],
                config,
                Arc::new(LocalProcessDispatcher),
                "leased",
                |binding| {
                    prepared = Some(spec.bind_preparation(binding)?);
                    Ok(())
                },
            )
            .unwrap();
        let generation = host.publish(candidate).unwrap();
        let prepared = prepared.unwrap();
        let value =
            generation.with_kernel(|kernel| prepared.bind_kernel(kernel).unwrap().query().unwrap());
        f.session.delete(f.a.source().address(), "value").unwrap();
        let error = generation
            .with_kernel(|kernel| prepared.bind_kernel(kernel).unwrap().query().unwrap_err());
        drop(generation);
        drop(host);
        drop(spec);
        drop(f);
        // Each independently escaped object retains the original known owner,
        // not the currently active deployment or another object's lifetime.
        let mut retained: [Option<Box<dyn std::any::Any>>; 3] = [
            Some(Box::new(prepared.clone())),
            Some(Box::new(value)),
            Some(Box::new(error)),
        ];
        drop(prepared);
        for (index, owner) in retained.iter_mut().enumerate() {
            if index != last {
                drop(owner.take());
                assert!(weak.upgrade().is_some());
            }
        }
        drop(retained[last].take());
        assert!(weak.upgrade().is_none(), "last owner {last}");
    }
}

#[test]
fn low_level_bound_queries_preserve_both_failure_policies_and_local_source_errors() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for policy in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            for process_failure in [false, true] {
                let f = Fixture::new(false);
                f.put(&f.a, 3);
                let mut graph = NetworkBuilder::new();
                let first = std::sync::atomic::AtomicBool::new(process_failure);
                let number = graph
                    .derive("number", (&f.a,), move |a: Number| {
                        assert!(
                            !first.swap(false, std::sync::atomic::Ordering::SeqCst),
                            "first process fails"
                        );
                        a.0
                    })
                    .unwrap();
                let spec = graph.query_spec("query", &number).unwrap();
                let plugin = authored(graph, spec.clone(), None);
                let mut config = options(&plugin);
                config.execution.request_queue_policy = queue;
                config.execution.request_failure_policy = policy;
                config.execution.capture_capacity = capacity();
                let mut catalog = PluginCatalog::new();
                let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
                let mut plugins = catalog.instance();
                plugins.load(&kind, plugin).unwrap();
                // The supported DerivedRuntime fixes RestartAffectedProcess.
                // Low-level binding also preserves an explicitly chosen policy.
                let mut kernel = phenix_kernel::Kernel::build_with_options(
                    "failure-policy",
                    plugins.snapshots(),
                    Arc::new(LocalProcessDispatcher),
                    config,
                )
                .unwrap();
                kernel.start().unwrap();
                let query = spec.bind_kernel(&kernel).unwrap();
                if !process_failure {
                    assert_eq!(*query.query().unwrap(), 3);
                    f.session.delete(f.a.source().address(), "value").unwrap();
                }
                let error = query.query().unwrap_err();
                if process_failure {
                    assert!(
                        failures(&error)
                            .iter()
                            .any(|failure| failure.kind() == phenix_graph::FailureKind::Panic)
                    );
                } else {
                    assert!(failures(&error).iter().any(|failure| {
                        matches!(
                            std::error::Error::source(failure)
                                .and_then(|source| source.downcast_ref::<phenix_storage::Error>()),
                            Some(phenix_storage::Error::MissingInput { .. })
                        )
                    }));
                }
                let terminal = process_failure && policy == RequestFailurePolicy::FailSession;
                kernel
                    .with_session(|session| {
                        assert_eq!(session.state() == SessionState::Failed, terminal)
                    })
                    .unwrap();
                if !terminal {
                    f.put(&f.a, 7);
                    assert_eq!(*query.query().unwrap(), 7);
                }
            }
        }
    }
}
