mod support;

use phenix_graph::{
    BoundOutput, ComponentDef, NetworkError, Value,
    compose::{ComposeError, ConnectionKey, Edit, Footprint, NetworkPatch},
    interface::{InterfacePhase, InterfaceReason},
    network::{ConnectionDef, NetworkBuilder},
    process::LocalProcessDispatcher,
    runtime::RequestQueuePolicy,
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, ContractDefinition, Contribution, Key, ResolutionError,
    host::{Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::{Arc, Mutex};

const STAGE: &str = "checked-stage@1";
const INNER_CONTRACT: &str = "checked-inner@1";
const INNER: &str = "service/inner";
const LEAF: &str = "service/inner/leaf";
const SIDE: &str = "service/inner/side";
const MONITOR: &str = "service/monitor";

struct Provider {
    freeze_inner: bool,
}
impl PluginInstance for Provider {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(ContractDefinition {
            id: "stage".into(),
            contract: support::contract(STAGE),
        })?;
        let mut inner = NetworkBuilder::new();
        let leaf = inner
            .add("leaf", support::definition(false, true, 1, STAGE))
            .unwrap();
        inner
            .expose_input(
                "run",
                leaf.input::<i64>("run").unwrap(),
                support::capacity(),
            )
            .unwrap();
        inner
            .expose_output("value", leaf.output::<i64>("value").unwrap())
            .unwrap();
        inner
            .require_contract("leaf", &support::contract(STAGE))
            .unwrap();
        inner.allow_extensions().unwrap();
        if self.freeze_inner {
            registrar.add(ContractDefinition {
                id: "inner".into(),
                contract: support::contract(INNER_CONTRACT),
            })?;
            inner.claim(&support::contract(INNER_CONTRACT)).unwrap();
        }
        let mut outer = NetworkBuilder::new();
        let child = outer
            .add(
                "inner",
                Arc::new(ComponentDef::composite(inner.build().unwrap())),
            )
            .unwrap();
        outer
            .expose_input(
                "run",
                child.input::<i64>("run").unwrap(),
                support::capacity(),
            )
            .unwrap();
        outer
            .expose_output("value", child.output::<i64>("value").unwrap())
            .unwrap();
        outer.allow_extensions().unwrap();
        if self.freeze_inner {
            outer
                .require_contract("inner", &support::contract(INNER_CONTRACT))
                .unwrap();
        }
        Contribution::new(
            "provide",
            NetworkPatch::new([
                Edit::Add {
                    path: "service".into(),
                    definition: Arc::new(ComponentDef::composite(outer.build().unwrap())),
                },
                Edit::ExposeInput {
                    name: "request".into(),
                    targets: vec![support::path("service", "run")],
                    capacity: support::capacity(),
                },
                Edit::ExposeOutput {
                    name: "reply".into(),
                    from: support::path("service", "value"),
                },
            ]),
        )
        .offer("leaf", LEAF, Key::new(registrar.owner(), "stage"))
        .register(registrar)
    }
}

struct Extension {
    enabled: bool,
    wrong_parent: bool,
    seen: Arc<Mutex<Vec<i64>>>,
    preview: Arc<Mutex<Option<BoundOutput<i64>>>>,
}
impl PluginInstance for Extension {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        if !self.enabled {
            return Contribution::new("extend", NetworkPatch::new([]))
                .after([Key::new("provider", "provide")])
                .register(registrar);
        }
        let definition = support::definition(false, true, 7, STAGE);
        let add = Edit::Add {
            path: SIDE.into(),
            definition,
        };
        let seen = self.seen.clone();
        let preview = self.preview.clone();
        let wrong_parent = self.wrong_parent;
        // The existing self-import preview authenticates the static Add. The
        // prepared payload must still include that actual Add in final replay.
        Contribution::new("extend", NetworkPatch::new([add.clone()]))
            .offer("side", SIDE, Key::new("provider", "stage"))
            .import("leaf", Key::new("provider", "stage"))
            .import("side", Key::new("provider", "stage"))
            .prepare(move |context| {
                assert!(context.inspect_network().is_err());
                let leaf = context.bind_import("leaf")?;
                let side = context.bind_import("side")?;
                let input = side.input::<i64>("run")?;
                let output = side.output::<i64>("value")?;
                *preview.lock().unwrap() = Some(output.clone());
                let observed = seen.clone();
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new([
                        add.clone(),
                        context.connect_bound_output(
                            &leaf.output::<i64>("value")?,
                            support::path(SIDE, "run"),
                            support::capacity(),
                        )?,
                        context.expose_nested_bound_input(
                            INNER,
                            "alternate",
                            &[input],
                            support::capacity(),
                        )?,
                        context.expose_nested_bound_output(
                            if wrong_parent {
                                "service/inner2"
                            } else {
                                INNER
                            },
                            "telemetry",
                            &output,
                        )?,
                        Edit::ExposeNestedInput {
                            parent: "service".into(),
                            name: "alternate".into(),
                            targets: vec![support::path(INNER, "alternate")],
                            capacity: support::capacity(),
                        },
                        Edit::ExposeInput {
                            name: "alternate".into(),
                            targets: vec![support::path("service", "alternate")],
                            capacity: support::capacity(),
                        },
                        Edit::Add {
                            path: MONITOR.into(),
                            definition: support::sink(move |value| {
                                observed.lock().unwrap().push(value)
                            }),
                        },
                        Edit::Connect(ConnectionDef {
                            from: support::path(INNER, "telemetry"),
                            to: support::path(MONITOR, "run"),
                            capacity: support::capacity(),
                        }),
                    ])),
                    ..Default::default()
                })
            })
            .register(registrar)
    }
}

fn options(queue: RequestQueuePolicy, enabled: bool) -> BuildOptions {
    let mut options = BuildOptions::default();
    options.execution.request_queue_policy = queue;
    let selections = [
        (Key::new("extension", "leaf"), Key::new("provider", "leaf")),
        (Key::new("extension", "side"), Key::new("extension", "side")),
    ];
    if enabled {
        options.imports.selections.extend(selections.clone());
        options.imports.allow = Some(selections.into());
    }
    support::grant(
        &mut options,
        Key::new("provider", "provide"),
        [
            Footprint::Component("service".into()),
            Footprint::InputExport("request".into()),
            Footprint::OutputExport("reply".into()),
        ],
    );
    support::grant(
        &mut options,
        Key::new("extension", "extend"),
        [
            Footprint::Component(SIDE.into()),
            Footprint::Component(MONITOR.into()),
            Footprint::Connection(ConnectionKey::new(
                support::path(LEAF, "value"),
                support::path(SIDE, "run"),
            )),
            Footprint::NestedInputExport {
                parent: INNER.into(),
                name: "alternate".into(),
            },
            Footprint::NestedOutputExport {
                parent: INNER.into(),
                name: "telemetry".into(),
            },
            Footprint::NestedInputExport {
                parent: "service".into(),
                name: "alternate".into(),
            },
            Footprint::InputExport("alternate".into()),
            Footprint::Connection(ConnectionKey::new(
                support::path(INNER, "telemetry"),
                support::path(MONITOR, "run"),
            )),
        ],
    );
    options
}

fn execute(generation: &Generation, value: i64) {
    let capture = generation.compiled().capture("reply").unwrap();
    let request = generation.begin_bound_request(&capture).unwrap().unwrap();
    generation
        .with_session(|session| {
            session
                .inject_request(request, "request", Value::local(value))
                .unwrap();
            session.close_request(request).unwrap();
        })
        .unwrap();
    assert!(!generation.run_steps(128).unwrap().budget_exhausted);
    let packet = generation
        .take_bound_request_output(request, &capture)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(packet.value().borrow::<i64>(), Some(&(value + 1)));
    generation
        .validate_bound_capture(request, &capture, &packet)
        .unwrap()
        .unwrap();
    generation
        .with_session(|session| {
            assert!(session.failure().is_none());
            session.release_request(request).unwrap();
        })
        .unwrap();
}

#[test]
fn prepared_private_add_connect_and_checked_exports_reach_enclosing_monitor_atomically() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let preview = Arc::new(Mutex::new(None));
        let extension = |enabled, wrong_parent| Extension {
            enabled,
            wrong_parent,
            seen: seen.clone(),
            preview: preview.clone(),
        };
        let mut catalog = PluginCatalog::new();
        let provider = catalog
            .register("provider", |freeze_inner| Ok(Provider { freeze_inner }))
            .unwrap();
        let extender = catalog.register("extension", |p: Extension| Ok(p)).unwrap();
        let mut host = KernelHost::new(catalog.instance());
        let candidate = host
            .prepare(
                vec![
                    host.plugins()
                        .prepare_load(&extender, extension(false, false))
                        .unwrap(),
                    host.plugins().prepare_load(&provider, false).unwrap(),
                ],
                options(queue, false).with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "base",
                |_| Ok(()),
            )
            .unwrap();
        let old = host.publish(candidate).unwrap();
        execute(&old, 2);
        assert!(seen.lock().unwrap().is_empty());
        let old_outer = old.compiled().inspect("service").unwrap();
        let old_leaf = old.compiled().inspect(LEAF).unwrap();
        let old_revision = host.plugins().snapshot(&extender).unwrap().revision();

        // Failed checked preparation, replay authority and final binding all
        // preserve the active generation and the staged plugin revision.
        for case in ["projection", "grant", "binding"] {
            let stage = host
                .plugins()
                .prepare_reload(&extender, extension(true, case == "projection"))
                .unwrap();
            let mut opts = options(queue, true);
            if case == "grant" {
                opts.composition
                    .get_mut(&Key::new("extension", "extend"))
                    .unwrap()
                    .grants
                    .remove(&Footprint::NestedOutputExport {
                        parent: INNER.into(),
                        name: "telemetry".into(),
                    });
            }
            let result = host.prepare(
                vec![stage],
                opts.with_dispatcher(Arc::new(LocalProcessDispatcher)),
                case,
                |binding| {
                    assert!(binding.declaration(MONITOR).is_ok());
                    Err("final binding refused".into())
                },
            );
            match (case, result) {
                ("projection", Err(LifecycleError::Build(error))) => {
                    let ResolutionError::Composition {
                        contribution,
                        cause,
                    } = error.as_ref()
                    else {
                        panic!("{error}");
                    };
                    assert_eq!(*contribution, Key::new("extension", "extend"));
                    assert_eq!(
                        cause.source().unwrap().downcast_ref::<NetworkError>(),
                        Some(&NetworkError::InvalidProjection("service/inner2".into()))
                    );
                }
                ("grant", Err(LifecycleError::Build(error))) => {
                    let ResolutionError::Composition {
                        contribution,
                        cause,
                    } = error.as_ref()
                    else {
                        panic!("{error}");
                    };
                    assert_eq!(*contribution, Key::new("extension", "extend"));
                    assert!(
                        matches!(cause.downcast_ref::<ComposeError>(), Some(ComposeError::Unauthorized {
                        footprint: Footprint::NestedOutputExport { parent, name }, ..
                    }) if parent == INNER && name == "telemetry")
                    );
                }
                ("binding", Err(LifecycleError::Binding(_))) => {}
                _ => panic!("unexpected {case} outcome"),
            }
            assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
            assert_eq!(
                host.plugins().snapshot(&extender).unwrap().revision(),
                old_revision
            );
            let rejected_preview = preview.lock().unwrap().take().unwrap();
            assert_eq!(
                rejected_preview.validate(old.compiled()),
                Err(NetworkError::ForeignScope)
            );
            drop(rejected_preview);
            assert!(old.compiled().inspect(SIDE).is_err());
            execute(&old, 3);
            assert!(seen.lock().unwrap().is_empty());
        }

        let stage = host
            .plugins()
            .prepare_reload(&extender, extension(true, false))
            .unwrap();
        let next_snapshot = stage.candidate().unwrap();
        let candidate = host
            .prepare(
                vec![stage],
                options(queue, true).with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "extended",
                |binding| {
                    assert!(binding.declaration(MONITOR).is_ok());
                    // Release the escaped preview after checking it, so its
                    // preparation lease cannot cycle through the fixture plugin.
                    let preview = preview.lock().unwrap().take().unwrap();
                    assert_eq!(
                        binding.capture("reply").unwrap().matches_output(&preview),
                        Err(NetworkError::ForeignScope)
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
        assert_eq!(
            host.plugins().snapshot(&extender).unwrap().revision(),
            old_revision
        );
        assert!(seen.lock().unwrap().is_empty());
        let active = host.publish(candidate).unwrap();
        assert_eq!(
            host.plugins().snapshot(&extender).unwrap().revision(),
            next_snapshot.revision()
        );
        execute(&active, 5);
        assert_eq!(*seen.lock().unwrap(), vec![13]);
        execute(&old, 5);
        assert_eq!(*seen.lock().unwrap(), vec![13]);

        // This route bypasses the original leaf, exercising both nested input
        // projections and the side's nested output into the enclosing monitor.
        let request = active.begin_bound_request(None).unwrap().unwrap();
        active
            .with_session(|session| {
                session
                    .inject_request(request, "alternate", Value::local(5_i64))
                    .unwrap();
                session.close_request(request).unwrap();
            })
            .unwrap();
        assert!(!active.run_steps(128).unwrap().budget_exhausted);
        assert_eq!(*seen.lock().unwrap(), vec![13, 12]);
        active
            .with_session(|session| {
                assert!(session.failure().is_none());
                assert!(
                    session
                        .take_request_output(request, "reply")
                        .unwrap()
                        .is_none()
                );
                session.release_request(request).unwrap();
            })
            .unwrap();

        let outer = active.compiled().inspect("service").unwrap();
        let inner = active.compiled().inspect(INNER).unwrap();
        let leaf = active.compiled().inspect(LEAF).unwrap();
        assert_eq!(
            outer.interface.input("run"),
            old_outer.interface.input("run")
        );
        assert_eq!(outer.interface.outputs(), old_outer.interface.outputs());
        assert_eq!(
            outer.interface.inputs().len(),
            old_outer.interface.inputs().len() + 1
        );
        assert!(outer.interface.input("alternate").is_some());
        assert_eq!(outer.claims, old_outer.claims);
        let provider_snapshot = host.plugins().snapshot(&provider).unwrap();
        // Each Kernel build issues a fresh contribution child origin, retaining
        // the unchanged provider revision's ancestry rather than origin equality.
        assert!(
            outer
                .origin
                .as_ref()
                .unwrap()
                .belongs_to(&phenix_graph::GraphOrigin::from_root(
                    provider_snapshot.revision_identity().clone(),
                ))
        );
        assert!(
            old_outer
                .origin
                .as_ref()
                .unwrap()
                .belongs_to(&phenix_graph::GraphOrigin::from_root(
                    provider_snapshot.revision_identity().clone(),
                ))
        );
        assert_eq!(leaf.definition_key, old_leaf.definition_key);
        assert_eq!(leaf.origin, outer.origin);
        assert_eq!(leaf.claims, old_leaf.claims);
        assert_eq!(inner.origin, outer.origin);
        assert!(inner.interface.input("alternate").is_some());
        assert!(inner.interface.output("telemetry").is_some());
        assert!(outer.interface.output("telemetry").is_none());
        for path in [SIDE, MONITOR] {
            let added = active.compiled().inspect(path).unwrap();
            let origin = added.origin.as_ref().unwrap();
            assert!(
                origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
                    next_snapshot.revision_identity().clone(),
                ))
            );
            assert_eq!(added.declared_origin.as_ref(), Some(origin));
            assert!(
                !origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
                    provider_snapshot.revision_identity().clone(),
                ))
            );
        }
        active
            .compiled()
            .check_implementation(LEAF, &support::contract(STAGE))
            .unwrap();
        active
            .compiled()
            .check_implementation(SIDE, &support::contract(STAGE))
            .unwrap();

        let stage = host.plugins().prepare_unload(&extender).unwrap();
        let candidate = host
            .prepare(
                vec![stage],
                options(queue, false).with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "removed",
                |_| Ok(()),
            )
            .unwrap();
        let removed = host.publish(candidate).unwrap();
        assert_eq!(
            removed.compiled().inspect("service").unwrap().interface,
            old_outer.interface
        );
        assert!(removed.compiled().inspect(SIDE).is_err());
        assert!(removed.compiled().inspect(MONITOR).is_err());
        assert!(
            removed
                .compiled()
                .inspect(INNER)
                .unwrap()
                .interface
                .output("telemetry")
                .is_none()
        );
        execute(&removed, 7);
        assert_eq!(*seen.lock().unwrap(), vec![13, 12]);
        execute(&active, 7);
        assert_eq!(*seen.lock().unwrap(), vec![13, 12, 15]);
    }
}

#[test]
fn nested_export_cannot_grow_an_exact_claimed_and_required_inner_interface() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let preview = Arc::new(Mutex::new(None));
        let extension = |enabled| Extension {
            enabled,
            wrong_parent: false,
            seen: seen.clone(),
            preview: preview.clone(),
        };
        let mut catalog = PluginCatalog::new();
        let provider = catalog
            .register("provider", |freeze_inner| Ok(Provider { freeze_inner }))
            .unwrap();
        let extender = catalog.register("extension", |p: Extension| Ok(p)).unwrap();
        let mut host = KernelHost::new(catalog.instance());
        let candidate = host
            .prepare(
                vec![
                    host.plugins().prepare_load(&provider, true).unwrap(),
                    host.plugins()
                        .prepare_load(&extender, extension(false))
                        .unwrap(),
                ],
                options(queue, false).with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "frozen",
                |_| Ok(()),
            )
            .unwrap();
        let active = host.publish(candidate).unwrap();
        active
            .compiled()
            .check_implementation(INNER, &support::contract(INNER_CONTRACT))
            .unwrap();
        let inner = active.compiled().inspect(INNER).unwrap();
        let revisions: Vec<_> = host
            .plugins()
            .snapshots()
            .map(|snapshot| (snapshot.name().to_owned(), snapshot.revision()))
            .collect();
        let stage = host
            .plugins()
            .prepare_reload(&extender, extension(true))
            .unwrap();
        let error = host
            .prepare(
                vec![stage],
                options(queue, true).with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "cannot grow",
                |_| panic!("exact inner contract must fail before final binding"),
            )
            .err()
            .expect("nested export must preserve exact inner contract");
        let LifecycleError::Build(error) = error else {
            panic!("{error}");
        };
        let ResolutionError::Compilation {
            cause: NetworkError::Interface(cause),
        } = error.as_ref()
        else {
            panic!("{error}");
        };
        assert_eq!(cause.reason.as_ref(), &InterfaceReason::ExtraPort);
        assert_eq!(cause.phase, InterfacePhase::Compiled);
        assert_eq!(cause.contract.as_deref(), Some(INNER_CONTRACT));
        assert_eq!(cause.endpoint.as_deref(), Some("input alternate"));
        let rejected_preview = preview.lock().unwrap().take().unwrap();
        assert_eq!(
            rejected_preview.validate(active.compiled()),
            Err(NetworkError::ForeignScope)
        );
        drop(rejected_preview);
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
        assert_eq!(
            host.plugins()
                .snapshots()
                .map(|snapshot| (snapshot.name().to_owned(), snapshot.revision()))
                .collect::<Vec<_>>(),
            revisions
        );
        let unchanged = active.compiled().inspect(INNER).unwrap();
        assert_eq!(unchanged.interface, inner.interface);
        assert_eq!(unchanged.claims, inner.claims);
        assert!(unchanged.interface.input("alternate").is_none());
        assert!(unchanged.interface.output("telemetry").is_none());
        assert!(active.compiled().inspect(SIDE).is_err());
        assert!(active.compiled().inspect(MONITOR).is_err());
        execute(&active, 5);
        assert!(seen.lock().unwrap().is_empty());
    }
}
