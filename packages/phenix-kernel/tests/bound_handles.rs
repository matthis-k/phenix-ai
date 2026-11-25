mod support;

use phenix_graph::{
    BoundDeclaration, CaptureHandle, ComponentDef, IngressHandle, InputKey, NetworkError,
    OutputKey, Value,
    compose::{Edit, Footprint, NetworkPatch},
    connection::ConnectionError,
    process::LocalProcessDispatcher,
    runtime::{RequestError, RequestFailurePolicy, RequestQueuePolicy},
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, Contribution, InputEvent, Kernel, Key,
    Producer, SessionAccessError,
    host::{Binding, KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
struct Counts {
    activation: Arc<AtomicUsize>,
    poll: Arc<AtomicUsize>,
    read: Arc<AtomicUsize>,
}
struct Idle(Counts);
impl Producer for Idle {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        self.0.poll.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}
struct Fixture {
    counts: Counts,
    definition: Arc<ComponentDef>,
}
impl Fixture {
    fn new(counts: &Counts) -> Self {
        let read = counts.read.clone();
        Self {
            counts: counts.clone(),
            definition: support::function(move |value| {
                read.fetch_add(1, Ordering::SeqCst);
                value + 1
            }),
        }
    }
}
impl PluginInstance for Fixture {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let counts = self.counts.clone();
        Contribution::new(
            "main",
            NetworkPatch::new([
                Edit::Add {
                    path: "worker".into(),
                    definition: self.definition.clone(),
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
                Edit::ExposeOutput {
                    name: "alias".into(),
                    from: support::path("worker", "value"),
                },
            ]),
        )
        .prepare(move |_| {
            let counts = counts.clone();
            let mut artifacts = CandidateArtifacts::default();
            artifacts.activate(move || {
                counts.activation.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(Idle(counts.clone())))
            })?;
            Ok(artifacts)
        })
        .register(into)
    }
}
fn options(queue: RequestQueuePolicy, failure: RequestFailurePolicy) -> BuildOptions {
    let mut options = BuildOptions::default();
    options.execution.request_queue_policy = queue;
    options.execution.request_failure_policy = failure;
    support::grant(
        &mut options,
        Key::new("fixture", "main"),
        [
            Footprint::Component("worker".into()),
            Footprint::InputExport("input".into()),
            Footprint::OutputExport("output".into()),
            Footprint::OutputExport("alias".into()),
        ],
    );
    options
}
struct Handles {
    ingress: IngressHandle<i64>,
    capture: CaptureHandle,
    alias: CaptureHandle,
    declaration: BoundDeclaration,
}
fn bind(binding: &Binding<'_>) -> Handles {
    let declaration = binding.declaration("worker").unwrap();
    let key = declaration.definition_key().unwrap();
    assert_eq!(binding.bind_definition(key).unwrap().path(), "worker");
    assert_eq!(binding.declarations(key).len(), 1);
    assert!(binding.declaration("").unwrap().definition_key().is_none());
    let ingress = binding
        .bind_ingress("input", InputKey::new("selector"))
        .unwrap();
    let capture = binding
        .bind_capture("output", OutputKey::<i64>::new("selector"))
        .unwrap();
    assert!(
        ingress
            .matches_input(&declaration.input::<i64>("run").unwrap())
            .unwrap()
    );
    assert!(
        capture
            .matches_output(&declaration.output::<i64>("value").unwrap())
            .unwrap()
    );
    assert!(binding.source(&declaration).is_err());
    Handles {
        ingress,
        capture,
        alias: binding.capture("alias").unwrap(),
        declaration,
    }
}

#[test]
fn host_final_binding_failure_archival_successor_and_alias_authentication() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for failure in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let counts = Counts::default();
            let mut catalog = PluginCatalog::new();
            let key = catalog
                .register("fixture", |fixture: Fixture| Ok(fixture))
                .unwrap();
            let mut host = KernelHost::new(catalog.instance());
            let stage = host
                .plugins()
                .prepare_load(&key, Fixture::new(&counts))
                .unwrap();
            let mut handles = None;
            let candidate = host
                .prepare(
                    vec![stage],
                    options(queue, failure),
                    Arc::new(LocalProcessDispatcher),
                    "first",
                    |binding| {
                        handles = Some(bind(binding));
                        assert_eq!(counts.activation.load(Ordering::SeqCst), 0);
                        assert_eq!(counts.poll.load(Ordering::SeqCst), 0);
                        assert_eq!(counts.read.load(Ordering::SeqCst), 0);
                        Ok(())
                    },
                )
                .unwrap();
            let old = host.publish(candidate).unwrap();
            let handles = handles.unwrap();
            assert_eq!(counts.activation.load(Ordering::SeqCst), 1);
            assert_eq!(counts.poll.load(Ordering::SeqCst), 0);
            assert_eq!(counts.read.load(Ordering::SeqCst), 0);

            let mut escaped = None;
            for panic in [false, true] {
                let rejected = host.prepare(
                    vec![],
                    options(queue, failure),
                    Arc::new(LocalProcessDispatcher),
                    "failed",
                    |binding| {
                        escaped = Some(bind(binding));
                        if panic {
                            panic!("binding failure");
                        }
                        Err(phenix_graph::Failure::application_message("binding rejection").into())
                    },
                );
                assert!(matches!(rejected, Err(LifecycleError::Binding(_))));
                assert_eq!(counts.activation.load(Ordering::SeqCst), 1);
                assert_eq!(counts.poll.load(Ordering::SeqCst), 0);
                assert_eq!(counts.read.load(Ordering::SeqCst), 0);
                let error = old
                    .inject_bound(&escaped.as_ref().unwrap().ingress, 17)
                    .unwrap_err();
                assert_eq!(error.reason, ConnectionError::ForeignTopology);
                assert_eq!(error.value.borrow::<i64>(), Some(&17));
            }
            let escaped = escaped.unwrap();
            let mut next_handles = None;
            let candidate = host
                .prepare(
                    vec![],
                    options(queue, failure),
                    Arc::new(LocalProcessDispatcher),
                    "successor",
                    |binding| {
                        next_handles = Some(bind(binding));
                        Ok(())
                    },
                )
                .unwrap();
            let next = host.publish(candidate).unwrap();
            let next_handles = next_handles.unwrap();
            assert!(old.drain(8).unwrap().is_empty());
            assert_eq!(counts.poll.load(Ordering::SeqCst), 0);
            for foreign in [&handles, &escaped] {
                next.with_session(|_| {
                    // Foreign must win over recursive session access for every surface.
                    assert_eq!(
                        next.inject_bound(&foreign.ingress, 4).unwrap_err().reason,
                        ConnectionError::ForeignTopology
                    );
                    assert_eq!(
                        next.begin_bound_request(&foreign.capture).unwrap(),
                        Err(RequestError::Foreign)
                    );
                    assert_eq!(
                        next.close_bound_input(&foreign.ingress).unwrap(),
                        Err(ConnectionError::ForeignTopology)
                    );
                    assert_eq!(
                        next.take_bound_output(&foreign.capture)
                            .unwrap()
                            .unwrap_err(),
                        ConnectionError::ForeignTopology
                    );
                    assert_eq!(
                        next.bound_output_closed(&foreign.capture).unwrap(),
                        Err(ConnectionError::ForeignTopology)
                    );
                    assert!(matches!(
                        next.bound_wake_handle(&foreign.declaration).unwrap(),
                        Err(NetworkError::ForeignScope)
                    ));
                    assert!(matches!(
                        next.bound_process_notifier(&foreign.declaration).unwrap(),
                        Err(NetworkError::ForeignScope)
                    ));
                })
                .unwrap();
            }
            let request = old.begin_bound_request(None).unwrap().unwrap();
            let new_request = next
                .begin_bound_request(&next_handles.capture)
                .unwrap()
                .unwrap();
            let rejected = next
                .inject_bound_request(request, &next_handles.ingress, 9)
                .unwrap_err();
            assert_eq!(rejected.reason, RequestError::Foreign);
            assert_eq!(rejected.value.borrow::<i64>(), Some(&9));
            for foreign in [&handles, &escaped] {
                next.with_session(|_| {
                    assert_eq!(
                        next.inject_bound_request(new_request, &foreign.ingress, 5)
                            .unwrap_err()
                            .reason,
                        RequestError::Foreign
                    );
                    assert_eq!(
                        next.take_bound_request_output(new_request, &foreign.capture)
                            .unwrap()
                            .unwrap_err(),
                        RequestError::Foreign
                    );
                })
                .unwrap();
            }
            // Retained old authority remains executable after real publication (C3).
            old.inject_bound_request(request, &handles.ingress, 10)
                .unwrap();
            old.with_session(|session| session.close_request(request).unwrap())
                .unwrap();
            old.run_steps(32).unwrap();
            let packet = old
                .take_bound_request_output(request, &handles.capture)
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(packet.value().borrow::<i64>(), Some(&11));
            old.validate_bound_capture(request, &handles.capture, &packet)
                .unwrap()
                .unwrap();
            assert_eq!(
                old.validate_bound_capture(request, &handles.alias, &packet)
                    .unwrap(),
                Err(RequestError::InvalidCapture)
            );
            assert_eq!(
                next.validate_bound_capture(new_request, &handles.capture, &packet)
                    .unwrap(),
                Err(RequestError::Foreign)
            );
            let alias = old
                .take_bound_request_output(request, &handles.alias)
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(
                alias.metadata().unwrap().process(),
                packet.metadata().unwrap().process()
            );
            assert_eq!(
                alias.metadata().unwrap().output(),
                packet.metadata().unwrap().output()
            );
            old.validate_bound_capture(request, &handles.alias, &alias)
                .unwrap()
                .unwrap();
            old.with_session(|session| session.release_request(request).unwrap())
                .unwrap();
            assert_eq!(
                old.inject_bound_request(request, &handles.ingress, 6)
                    .unwrap_err()
                    .reason,
                RequestError::Stale
            );
            old.close_bound_input(&handles.ingress).unwrap().unwrap();
            old.run_steps(32).unwrap();
            assert!(old.bound_output_closed(&handles.capture).unwrap().unwrap());
            next.inject_bound_request_value(
                new_request,
                &next_handles.ingress,
                Value::local(20_i64),
            )
            .unwrap();
            next.with_session(|session| session.close_request(new_request).unwrap())
                .unwrap();
            next.run_steps(32).unwrap();
            assert_eq!(
                next.take_bound_request_output(new_request, &next_handles.capture)
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .value()
                    .borrow::<i64>(),
                Some(&21)
            );
        }
    }
}

#[test]
fn preview_endpoints_never_become_final_authority() {
    let escaped = Arc::new(Mutex::new(None));
    struct Preview(Arc<Mutex<Option<phenix_graph::BoundInput<i64>>>>);
    impl PluginInstance for Preview {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(phenix_kernel::ContractDefinition {
                id: "contract".into(),
                contract: support::contract("preview@1"),
            })?;
            let escaped = self.0.clone();
            Contribution::new(
                "main",
                support::patch("provider", false, true, 1, "preview@1"),
            )
            .offer("offer", "provider", Key::new("preview", "contract"))
            .import("self", Key::new("preview", "contract"))
            .prepare(move |context| {
                let endpoint = context.bind_import("self")?.input::<i64>("run")?;
                let patch = context.expose_bound_input(
                    "input",
                    std::slice::from_ref(&endpoint),
                    support::capacity(),
                )?;
                *escaped.lock().unwrap() = Some(endpoint);
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new([
                        Edit::Add {
                            path: "provider".into(),
                            definition: support::definition(false, true, 1, "preview@1"),
                        },
                        patch,
                    ])),
                    ..Default::default()
                })
            })
            .register(into)
        }
    }
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("preview", "main"),
        [
            Footprint::Component("provider".into()),
            Footprint::InputExport("input".into()),
        ],
    );
    let kernel = Kernel::build_with_options(
        "preview",
        [support::snapshot("preview", Preview(escaped.clone()))],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    let preview = escaped.lock().unwrap().take().unwrap();
    assert_eq!(
        preview.validate(kernel.compiled()),
        Err(NetworkError::ForeignScope)
    );
    let final_ingress = kernel
        .compiled()
        .bind_ingress("input", InputKey::new("input"))
        .unwrap();
    assert_eq!(
        final_ingress.matches_input(&preview),
        Err(NetworkError::ForeignScope)
    );
    assert_eq!(
        kernel.inject_bound(&final_ingress, 1).unwrap_err().reason,
        ConnectionError::Cancelled
    );
    assert_eq!(
        kernel.close_bound_input(&final_ingress),
        Err(SessionAccessError::NotStarted)
    );
}

#[test]
fn final_bound_source_and_wakes_keep_actual_session_request_authority() {
    use phenix_graph::{
        Interface, PortDef,
        process::{Process, ProcessContext, ProcessEmission, Step},
    };
    struct Source(bool);
    impl Process for Source {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
            if std::mem::replace(&mut self.0, true) {
                self.0 = false;
                Ok(Step::Complete)
            } else {
                Ok(Step::Emit(ProcessEmission::new(
                    "value",
                    Value::local(42_i64),
                )))
            }
        }
    }
    struct SourcePlugin;
    impl PluginInstance for SourcePlugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            Contribution::new(
                "main",
                NetworkPatch::new([
                    Edit::Add {
                        path: "source".into(),
                        definition: Arc::new(ComponentDef::primitive(
                            Arc::new(Interface::new([], [PortDef::local::<i64>("value")]).unwrap()),
                            Arc::new(|| Source(false)),
                        )),
                    },
                    Edit::AddSource("source".into()),
                    Edit::ExposeOutput {
                        name: "output".into(),
                        from: support::path("source", "value"),
                    },
                ]),
            )
            .register(into)
        }
    }
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for failure in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let mut catalog = PluginCatalog::new();
            let key = catalog
                .register("source", |_: ()| Ok(SourcePlugin))
                .unwrap();
            let mut host = KernelHost::new(catalog.instance());
            let stage = host.plugins().prepare_load(&key, ()).unwrap();
            let mut options = BuildOptions::default();
            options.execution.auto_start_sources = false;
            options.execution.request_queue_policy = queue;
            options.execution.request_failure_policy = failure;
            support::grant(
                &mut options,
                Key::new("source", "main"),
                [
                    Footprint::Component("source".into()),
                    Footprint::Source("source".into()),
                    Footprint::OutputExport("output".into()),
                ],
            );
            let mut handles = None;
            let candidate = host
                .prepare(
                    vec![stage],
                    options,
                    Arc::new(LocalProcessDispatcher),
                    "source",
                    |binding| {
                        let declaration = binding.declaration("source")?;
                        handles = Some((
                            binding.source(&declaration)?,
                            binding.capture("output")?,
                            declaration,
                        ));
                        Ok(())
                    },
                )
                .unwrap();
            let generation = host.publish(candidate).unwrap();
            let (source, capture, declaration) = handles.unwrap();
            let wake = generation
                .bound_wake_handle(&declaration)
                .unwrap()
                .unwrap()
                .unwrap();
            let notifier = generation
                .bound_process_notifier(&declaration)
                .unwrap()
                .unwrap()
                .unwrap();
            notifier.notify();
            assert!(wake.wake());
            let request = generation.begin_bound_request(&capture).unwrap().unwrap();
            let mut second =
                generation
                    .compiled()
                    .start_with_options(phenix_graph::runtime::SessionOptions {
                        auto_start_sources: false,
                        request_queue_policy: queue,
                        request_failure_policy: failure,
                        ..Default::default()
                    });
            let other_request = second.begin_bound_request(&capture).unwrap();
            assert_eq!(
                generation
                    .start_bound_request_source(other_request, &source)
                    .unwrap(),
                Err(RequestError::Foreign)
            );
            assert_eq!(
                second.start_bound_request_source(request, &source),
                Err(RequestError::Foreign)
            );
            generation
                .start_bound_request_source(request, &source)
                .unwrap()
                .unwrap();
            assert_eq!(
                generation
                    .start_bound_request_source(request, &source)
                    .unwrap(),
                Err(RequestError::Busy)
            );
            generation
                .with_session(|session| session.close_request(request).unwrap())
                .unwrap();
            assert_eq!(
                generation
                    .start_bound_request_source(request, &source)
                    .unwrap(),
                Err(RequestError::Sealed)
            );
            generation.run_steps(32).unwrap();
            let packet = generation
                .take_bound_request_output(request, &capture)
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(packet.value().borrow::<i64>(), Some(&42));
            generation
                .validate_bound_capture(request, &capture, &packet)
                .unwrap()
                .unwrap();
            generation
                .with_session(|session| session.release_request(request).unwrap())
                .unwrap();
            assert_eq!(
                generation
                    .start_bound_request_source(request, &source)
                    .unwrap(),
                Err(RequestError::Stale)
            );
            // The topology handle is shared, but each session allocates its own request authority.
            second
                .start_bound_request_source(other_request, &source)
                .unwrap();
            second.close_request(other_request).unwrap();
            second.run_until_idle(32);
            assert!(
                second
                    .take_bound_request_output(other_request, &capture)
                    .unwrap()
                    .is_some()
            );
            let foreign = Kernel::build("foreign", [], Arc::new(LocalProcessDispatcher)).unwrap();
            assert_eq!(
                foreign
                    .start_bound_request_source(request, &source)
                    .unwrap(),
                Err(RequestError::Foreign)
            );
        }
    }
}
