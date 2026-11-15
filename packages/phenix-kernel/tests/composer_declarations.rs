mod support;

use phenix_graph::{
    Value,
    compose::{ComposeError, Edit, Footprint, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, ContractDefinition, Contribution, InputEvent,
    Kernel, Key, Producer, ResolutionError,
};
use phenix_plugin::{CallbackPhase, Code, Failure, PluginCatalog, PluginInstance, Registrar};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

struct Declarations<F>(F);
impl<F: Fn(&mut Registrar) -> Result<(), Failure> + Send + Sync> PluginInstance
    for Declarations<F>
{
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        (self.0)(registrar)
    }
}

#[test]
fn caught_collisions_leave_every_fresh_bundle_name_reusable() {
    for collision in ["composer", "offer", "import"] {
        let contract = Key::new("contracts", "service");
        let mut registrar = Registrar::new("actual-owner").unwrap();
        Contribution::new("taken", NetworkPatch::new([]))
            .offer("taken", "unit", contract.clone())
            .import("taken", contract.clone())
            .register(&mut registrar)
            .unwrap();

        let mut failed = Contribution::new(
            if collision == "composer" {
                "taken"
            } else {
                "fresh"
            },
            NetworkPatch::new([]),
        )
        .offer("fresh", "unit", contract.clone())
        .import("fresh", contract.clone());
        if collision == "offer" {
            failed = failed.offer("taken", "unit", contract.clone());
        }
        if collision == "import" {
            failed = failed.import("taken", contract.clone());
        }
        assert!(failed.register(&mut registrar).is_err());

        Contribution::new("fresh", NetworkPatch::new([]))
            .offer("fresh", "unit", contract.clone())
            .import("fresh", contract)
            .register(&mut registrar)
            .unwrap();
        let inventory = registrar.finish();
        assert_eq!(
            inventory.keys().collect::<Vec<_>>(),
            [
                ("actual-owner", "kernel.contribution", "fresh"),
                ("actual-owner", "kernel.contribution", "taken"),
                ("actual-owner", "network.offer", "fresh"),
                ("actual-owner", "network.offer", "taken"),
                ("actual-owner", "network.requirement", "fresh"),
                ("actual-owner", "network.requirement", "taken"),
            ],
            "{collision} collision left a partial bundle",
        );
    }
}

#[test]
fn duplicate_offer_and_import_ids_within_a_composer_fail_atomically() {
    for duplicate in ["offer", "import"] {
        let contract = Key::new("contracts", "service");
        let mut registrar = Registrar::new("plugin").unwrap();
        let mut failed = Contribution::new("service", NetworkPatch::new([]))
            .offer("primary", "unit", contract.clone())
            .import("dependency", contract.clone());
        failed = match duplicate {
            "offer" => failed.offer("primary", "other-unit", contract.clone()),
            "import" => failed.import("dependency", Key::new("contracts", "other")),
            _ => unreachable!(),
        };
        assert!(failed.register(&mut registrar).is_err());
        Contribution::new("service", NetworkPatch::new([]))
            .offer("primary", "unit", contract.clone())
            .import("dependency", contract)
            .register(&mut registrar)
            .unwrap();
        assert_eq!(registrar.finish().keys().count(), 3);
    }
}

#[test]
fn invalid_composer_offer_and_import_ids_reserve_nothing() {
    for invalid in [
        String::new(),
        "bad/id".into(),
        "nonascii-\u{e9}".into(),
        "x".repeat(257),
    ] {
        for kind in ["composer", "offer", "import"] {
            let contract = Key::new("contracts", "service");
            let mut registrar = Registrar::new("plugin").unwrap();
            assert!(
                Contribution::new(
                    if kind == "composer" {
                        &invalid
                    } else {
                        "service"
                    },
                    NetworkPatch::new([]),
                )
                .offer(
                    if kind == "offer" { &invalid } else { "primary" },
                    "unit",
                    contract.clone()
                )
                .import(
                    if kind == "import" {
                        &invalid
                    } else {
                        "dependency"
                    },
                    contract.clone()
                )
                .register(&mut registrar)
                .is_err(),
                "accepted invalid {kind} id: {invalid:?}",
            );
            Contribution::new("service", NetworkPatch::new([]))
                .offer("primary", "unit", contract.clone())
                .import("dependency", contract)
                .register(&mut registrar)
                .unwrap();
            assert_eq!(registrar.finish().keys().count(), 3);
        }
    }
}

#[test]
fn multiple_offers_and_self_imports_derive_the_actual_composer_identity() {
    let patch = NetworkPatch::new([
        Edit::Add {
            path: "unit-one".into(),
            definition: support::definition(false, true, 1, "test/service@1"),
        },
        Edit::Add {
            path: "unit-two".into(),
            definition: support::definition(false, true, 10, "test/service@1"),
        },
    ]);
    let authored = patch.clone();
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("actual-owner", "service"),
        patch.footprints(),
    );
    for name in ["service", "second"] {
        support::grant(
            &mut options,
            Key::new("actual-owner", "service"),
            [
                Footprint::InputExport(name.into()),
                Footprint::OutputExport(name.into()),
            ],
        );
    }
    for (requirement, offer) in [("service", "service"), ("second", "secondary")] {
        options.imports.selections.insert(
            Key::new("actual-owner", requirement),
            Key::new("actual-owner", offer),
        );
    }
    let snapshot = support::snapshot(
        "actual-owner",
        Declarations(move |r: &mut Registrar| {
            let contract = Key::new(r.owner(), "service");
            r.add(ContractDefinition {
                id: "service".into(),
                contract: support::contract("test/service@1"),
            })?;
            let prepared = authored.clone();
            Contribution::new("service", authored.clone())
                .offer("service", "unit-one", contract.clone())
                .offer("secondary", "unit-two", contract.clone())
                .import("service", contract.clone())
                .import("second", contract)
                .prepare(move |context| {
                    let first = context.import("service")?;
                    let second = context.import("second")?;
                    assert_eq!(first.producer, Key::new("actual-owner", "service"));
                    assert_eq!(second.producer, first.producer);
                    assert_eq!(first.offer, Key::new("actual-owner", "service"));
                    assert_eq!(second.offer, Key::new("actual-owner", "secondary"));
                    let mut edits = prepared.edits().to_vec();
                    for (id, expected_unit) in [("service", "unit-one"), ("second", "unit-two")] {
                        let bound = context.bind_import(id)?;
                        assert_eq!(bound.path(), expected_unit);
                        edits.push(Edit::ExposeInput {
                            name: id.into(),
                            targets: vec![support::path(bound.path(), "run")],
                            capacity: support::capacity(),
                        });
                        edits.push(Edit::ExposeOutput {
                            name: id.into(),
                            from: support::path(bound.path(), "value"),
                        });
                    }
                    assert!(context.import("undeclared").is_err());
                    assert!(context.inspect_network().is_err());
                    let mut artifacts = CandidateArtifacts {
                        patch: Some(NetworkPatch::new(edits)),
                        ..Default::default()
                    };
                    artifacts.export((first.producer, second.offer))?;
                    Ok(artifacts)
                })
                .register(r)
        }),
    );
    // Composer, contract, offer and import IDs retain their distinct namespaces.
    assert_eq!(snapshot.keys().count(), 6);
    let ambiguous = Kernel::build(
        "unselected",
        [snapshot.clone()],
        Arc::new(LocalProcessDispatcher),
    )
    .err()
    .expect("two compatible offers must be ambiguous by default");
    assert!(
        matches!(ambiguous, ResolutionError::Ambiguous { offers, .. }
        if offers == [Key::new("actual-owner", "secondary"), Key::new("actual-owner", "service")])
    );
    let mut kernel = Kernel::build_with_options(
        "self-imports",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    assert_eq!(
        kernel.export::<(Key, Key)>(&Key::new("actual-owner", "service")),
        Some(&(
            Key::new("actual-owner", "service"),
            Key::new("actual-owner", "secondary")
        )),
    );
    kernel.start().unwrap();
    for (id, expected) in [("service", 8), ("second", 17)] {
        support::run(&kernel, id, 7);
        let actual = kernel
            .with_session(|session| {
                *session
                    .take_output(id)
                    .unwrap()
                    .unwrap()
                    .value()
                    .borrow::<i64>()
                    .unwrap()
            })
            .unwrap();
        assert_eq!(actual, expected, "host-selected import {id}");
    }
}

#[test]
fn reserved_kind_spoofs_collide_atomically_and_noncolliding_spoofs_fail_before_preparation() {
    struct ComposerSpoof(&'static str);
    struct OfferSpoof(&'static str);
    impl phenix_plugin::Contribution for ComposerSpoof {
        const KIND: &'static str = "kernel.contribution";
        fn id(&self) -> &str {
            self.0
        }
    }
    impl phenix_plugin::Contribution for OfferSpoof {
        const KIND: &'static str = "network.offer";
        fn id(&self) -> &str {
            self.0
        }
    }
    fn spoof(r: &mut Registrar, composer: bool, id: &'static str) -> Result<(), Failure> {
        if composer {
            r.add(ComposerSpoof(id))
        } else {
            r.add(OfferSpoof(id))
        }
    }

    for composer in [true, false] {
        let contract = Key::new("contracts", "service");
        let mut registrar = Registrar::new("plugin").unwrap();
        spoof(&mut registrar, composer, "taken").unwrap();
        assert!(
            Contribution::new(
                if composer { "taken" } else { "fresh" },
                NetworkPatch::new([])
            )
            .offer("taken", "unit", contract.clone())
            .import("fresh", contract.clone())
            .register(&mut registrar)
            .is_err()
        );
        Contribution::new("fresh", NetworkPatch::new([]))
            .offer("fresh", "unit", contract.clone())
            .import("fresh", contract)
            .register(&mut registrar)
            .unwrap();
        assert_eq!(registrar.finish().keys().count(), 4);

        let prepared = Arc::new(AtomicBool::new(false));
        let observed = prepared.clone();
        let snapshot = support::snapshot(
            "plugin",
            Declarations(move |r: &mut Registrar| {
                let observed = observed.clone();
                Contribution::new("real", NetworkPatch::new([]))
                    .prepare(move |_| {
                        observed.store(true, Ordering::SeqCst);
                        Ok(CandidateArtifacts::default())
                    })
                    .register(r)?;
                spoof(r, composer, "spoof")
            }),
        );
        let error = Kernel::build("spoof", [snapshot], Arc::new(LocalProcessDispatcher))
            .err()
            .expect("reserved kind must retain its concrete payload type");
        assert!(
            matches!(error, ResolutionError::Invalid(ref message)
            if message.contains("contribution type mismatch") && message.contains("spoof")),
            "{error}"
        );
        assert!(!prepared.load(Ordering::SeqCst));
    }
}

#[test]
fn self_import_preview_and_prepared_replacement_require_their_exact_write_grants() {
    for grant_preview in [false, true] {
        let patch = support::patch("unit", false, true, 1, "test/service@1");
        let authored = patch.clone();
        let mut options = BuildOptions::default();
        support::grant(
            &mut options,
            Key::new("plugin", "sibling"),
            [Footprint::Component("protected".into())],
        );
        support::grant(
            &mut options,
            Key::new("plugin", "self"),
            if grant_preview {
                patch.footprints()
            } else {
                [Footprint::Component("unrelated".into())].into()
            },
        );
        let prepared = Arc::new(AtomicBool::new(false));
        let observed = prepared.clone();
        let snapshot = support::snapshot(
            "plugin",
            Declarations(move |r: &mut Registrar| {
                let contract = Key::new(r.owner(), "definition");
                r.add(ContractDefinition {
                    id: "definition".into(),
                    contract: support::contract("test/service@1"),
                })?;
                Contribution::new(
                    "sibling",
                    support::patch("protected", false, true, 10, "test/service@1"),
                )
                .register(r)?;
                let observed = observed.clone();
                let replacement = authored.clone();
                Contribution::new("self", authored.clone())
                    .after([Key::new(r.owner(), "sibling")])
                    .offer("offer", "unit", contract.clone())
                    .import("self", contract)
                    .prepare(move |context| {
                        observed.store(true, Ordering::SeqCst);
                        context.bind_import("self")?;
                        assert!(context.inspect_network().is_err());
                        Ok(CandidateArtifacts {
                            patch: Some(NetworkPatch::new(
                                replacement.edits().iter().cloned().chain([Edit::Replace {
                                    path: "protected".into(),
                                    definition: support::definition(
                                        false,
                                        true,
                                        100,
                                        "test/service@1",
                                    ),
                                }]),
                            )),
                            ..Default::default()
                        })
                    })
                    .register(r)
            }),
        );
        let error = Kernel::build_with_options(
            "authority",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options,
        )
        .err()
        .expect("imports must not confer write authority");
        match error {
            ResolutionError::Composition {
                contribution,
                cause,
            } => {
                assert_eq!(contribution, Key::new("plugin", "self"));
                assert!(
                    matches!(cause.downcast_ref::<ComposeError>(), Some(ComposeError::Unauthorized { footprint, .. })
                    if *footprint == Footprint::Component(if grant_preview { "protected" } else { "unit" }.into())),
                    "{cause}"
                );
            }
            other => panic!("expected exact-grant rejection, got {other}"),
        }
        assert_eq!(prepared.load(Ordering::SeqCst), grant_preview);
    }
}

#[test]
fn repeated_after_and_route_append_and_prepare_replaces() {
    struct Source(VecDeque<String>);
    impl Producer for Source {
        fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
            Ok(self.0.pop_front().map(|input| InputEvent {
                input,
                value: Value::local(1_i64),
            }))
        }
    }
    struct Replaced(Arc<AtomicBool>);
    impl Drop for Replaced {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let patch = NetworkPatch::new([
        Edit::Add {
            path: "left".into(),
            definition: support::sink(|_| {}),
        },
        Edit::Add {
            path: "right".into(),
            definition: support::sink(|_| {}),
        },
        Edit::ExposeInput {
            name: "first".into(),
            targets: vec![support::path("left", "run")],
            capacity: support::capacity(),
        },
        Edit::ExposeInput {
            name: "second".into(),
            targets: vec![support::path("right", "run")],
            capacity: support::capacity(),
        },
    ]);
    let authored = patch.clone();
    let mut options = BuildOptions::default();
    support::grant(&mut options, Key::new("plugin", "a"), patch.footprints());
    let log = Arc::new(Mutex::new(Vec::new()));
    let recorded = log.clone();
    let replaced = Arc::new(AtomicBool::new(false));
    let observed = replaced.clone();
    let snapshot = support::snapshot(
        "plugin",
        Declarations(move |r: &mut Registrar| {
            let old = Replaced(observed.clone());
            let prepared = recorded.clone();
            let main = Contribution::new("a", authored.clone())
                .after([Key::new(r.owner(), "z")])
                .after([Key::new(r.owner(), "b")])
                .route(["first".into()])
                .route(["second".into()])
                .prepare(move |_| {
                    let _ = &old;
                    panic!("replaced callback must never execute")
                })
                .prepare(move |_| {
                    prepared.lock().unwrap().push("a");
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.activate(|| {
                        Ok(Box::new(Source(VecDeque::from([
                            "first".into(),
                            "second".into(),
                        ]))))
                    })?;
                    Ok(artifacts)
                });
            assert!(observed.load(Ordering::SeqCst));
            main.register(r)?;
            for id in ["b", "z"] {
                let prepared = recorded.clone();
                Contribution::new(id, NetworkPatch::new([]))
                    .prepare(move |_| {
                        prepared.lock().unwrap().push(id);
                        Ok(CandidateArtifacts::default())
                    })
                    .register(r)?;
            }
            Ok(())
        }),
    );
    assert!(replaced.load(Ordering::SeqCst));
    let mut kernel = Kernel::build_with_options(
        "append-and-replace",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    assert_eq!(*log.lock().unwrap(), ["b", "z", "a"]);
    kernel.start().unwrap();
    let events = kernel.drain(2).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.result.is_ok()));
}

#[test]
fn failed_bundles_drop_their_callbacks_before_the_plugin_instance() {
    struct Provider {
        alive: Arc<AtomicBool>,
        log: Arc<Mutex<Vec<&'static str>>>,
        duplicate_within_bundle: bool,
    }
    struct CallbackDrop {
        alive: Arc<AtomicBool>,
        log: Arc<Mutex<Vec<&'static str>>>,
    }
    impl Drop for CallbackDrop {
        fn drop(&mut self) {
            assert!(
                self.alive.load(Ordering::SeqCst),
                "provider already destroyed"
            );
            self.log.lock().unwrap().push("callback");
        }
    }
    impl PluginInstance for Provider {
        fn contribute(&self, r: &mut Registrar) -> Result<(), Failure> {
            let contract = Key::new("contracts", "service");
            Contribution::new("installed", NetworkPatch::new([]))
                .import("taken", contract.clone())
                .register(r)?;
            let captured = CallbackDrop {
                alive: self.alive.clone(),
                log: self.log.clone(),
            };
            let mut failed = Contribution::new("fresh", NetworkPatch::new([]))
                .prepare(move |_| {
                    let _ = &captured;
                    Ok(CandidateArtifacts::default())
                })
                .offer("fresh", "unit", contract.clone())
                .import("fresh", contract.clone())
                .import("taken", contract.clone());
            if self.duplicate_within_bundle {
                failed = failed.import("taken", contract.clone());
            }
            assert!(failed.register(r).is_err());
            assert_eq!(*self.log.lock().unwrap(), ["callback"]);
            Contribution::new("fresh", NetworkPatch::new([]))
                .offer("fresh", "unit", contract.clone())
                .import("fresh", contract)
                .register(r)
        }
    }
    impl Drop for Provider {
        fn drop(&mut self) {
            self.alive.store(false, Ordering::SeqCst);
            self.log.lock().unwrap().push("provider");
        }
    }
    for duplicate_within_bundle in [false, true] {
        let alive = Arc::new(AtomicBool::new(true));
        let log = Arc::new(Mutex::new(Vec::new()));
        let snapshot = support::snapshot(
            "plugin",
            Provider {
                alive: alive.clone(),
                log: log.clone(),
                duplicate_within_bundle,
            },
        );
        assert!(alive.load(Ordering::SeqCst));
        assert_eq!(snapshot.keys().count(), 5);
        drop(snapshot);
        assert!(!alive.load(Ordering::SeqCst));
        assert_eq!(*log.lock().unwrap(), ["callback", "provider"]);
    }
}

#[test]
fn propagated_bundle_failure_drops_patch_factory_captures_before_implementation_and_provider() {
    struct Mark {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
        implementation_alive: Arc<AtomicBool>,
        provider_alive: Arc<AtomicBool>,
    }
    impl Drop for Mark {
        fn drop(&mut self) {
            match self.name {
                "definition" => {
                    assert!(self.implementation_alive.load(Ordering::SeqCst));
                    assert!(self.provider_alive.load(Ordering::SeqCst));
                }
                "implementation" => {
                    assert!(self.provider_alive.load(Ordering::SeqCst));
                    self.implementation_alive.store(false, Ordering::SeqCst);
                }
                "provider" => {
                    assert!(!self.implementation_alive.load(Ordering::SeqCst));
                    self.provider_alive.store(false, Ordering::SeqCst);
                }
                _ => unreachable!(),
            }
            self.log.lock().unwrap().push(self.name);
        }
    }
    struct Plugin(Mark);
    impl PluginInstance for Plugin {
        fn contribute(&self, r: &mut Registrar) -> Result<(), Failure> {
            let contract = Key::new("contracts", "service");
            Contribution::new("installed", NetworkPatch::new([]))
                .offer("taken", "unused", contract.clone())
                .register(r)?;
            let captured = Mark {
                name: "definition",
                log: self.0.log.clone(),
                implementation_alive: self.0.implementation_alive.clone(),
                provider_alive: self.0.provider_alive.clone(),
            };
            // The process factory owns this capture through the patch definition.
            let definition = support::function(move |_| {
                let _ = &captured;
                panic!("failed declaration must never execute")
            });
            Contribution::new(
                "failed",
                NetworkPatch::new([Edit::Add {
                    path: "unit".into(),
                    definition,
                }]),
            )
            .offer("taken", "unit", contract)
            .register(r)
        }
    }

    let log = Arc::new(Mutex::new(Vec::new()));
    let implementation_alive = Arc::new(AtomicBool::new(true));
    let provider_alive = Arc::new(AtomicBool::new(true));
    let provider = Mark {
        name: "provider",
        log: log.clone(),
        implementation_alive: implementation_alive.clone(),
        provider_alive: provider_alive.clone(),
    };
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("plugin", move |()| {
            let _ = &provider;
            Ok(Plugin(Mark {
                name: "implementation",
                log: provider.log.clone(),
                implementation_alive: provider.implementation_alive.clone(),
                provider_alive: provider.provider_alive.clone(),
            }))
        })
        .unwrap();
    let mut plugins = catalog.instance();
    let failure = plugins
        .load(&key, ())
        .err()
        .expect("bundle error must propagate from contribute");
    assert_eq!(failure.code, Code::DescriptorInvalid);
    assert_eq!(
        failure.context.as_ref().unwrap().phase,
        CallbackPhase::Contribute
    );
    assert!(plugins.snapshots().next().is_none());
    assert_eq!(*log.lock().unwrap(), ["definition"]);
    drop((plugins, catalog));
    assert!(implementation_alive.load(Ordering::SeqCst));
    assert!(provider_alive.load(Ordering::SeqCst));
    drop(failure);
    assert_eq!(
        *log.lock().unwrap(),
        ["definition", "implementation", "provider"]
    );
    assert!(!implementation_alive.load(Ordering::SeqCst));
    assert!(!provider_alive.load(Ordering::SeqCst));
}
