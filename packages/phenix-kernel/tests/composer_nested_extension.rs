mod support;

use phenix_graph::{
    ComponentDef, Value,
    compose::{ComposeError, Edit, Footprint, NetworkPatch},
    network::{NetworkBuilder, NetworkError},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, ContractDefinition, Contribution, Key, ResolutionError,
    ResolvedImport,
    host::{Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

const OUTER: &str = "test/outer@1";
const LEAF: &str = "test/leaf@1";
const LEAF_PATH: &str = "service/inner/leaf";

struct Provider {
    expose_inner: bool,
    expose_leaf: bool,
}

impl PluginInstance for Provider {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        for (id, semantic) in [("outer", OUTER), ("leaf", LEAF)] {
            registrar.add(ContractDefinition {
                id: id.into(),
                contract: support::contract(semantic),
            })?;
        }
        let mut inner = NetworkBuilder::new();
        let leaf = inner
            .add("leaf", support::definition(false, true, 1, LEAF))
            .unwrap();
        let sibling = inner
            .add("sibling", support::function(|value| value * 10))
            .unwrap();
        inner
            .connect(
                leaf.output::<i64>("value").unwrap(),
                sibling.input::<i64>("run").unwrap(),
                support::capacity(),
            )
            .unwrap();
        inner
            .expose_input(
                "run",
                leaf.input::<i64>("run").unwrap(),
                support::capacity(),
            )
            .unwrap();
        inner
            .expose_output("value", sibling.output::<i64>("value").unwrap())
            .unwrap();
        inner
            .require_contract("leaf", &support::contract(LEAF))
            .unwrap();
        if self.expose_leaf {
            inner.allow_replace(&leaf).unwrap();
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
        if self.expose_inner {
            outer.allow_replace(&child).unwrap();
        }
        outer.claim(&support::contract(OUTER)).unwrap();
        Contribution::new(
            "provide",
            NetworkPatch::new([Edit::Add {
                path: "service".into(),
                definition: Arc::new(ComponentDef::composite(outer.build().unwrap())),
            }]),
        )
        .offer("outer", "service", Key::new(registrar.owner(), "outer"))
        .register(registrar)
    }
}

struct Consumer;

impl PluginInstance for Consumer {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("consume", NetworkPatch::new([]))
            .import("outer", Key::new("provider", "outer"))
            .prepare(|context| {
                assert!(context.inspect_network().is_err());
                assert!(context.import("leaf").is_err());
                assert!(context.bind_import("leaf").is_err());
                let bound = context.bind_import("outer")?;
                assert_eq!(bound.contract().id(), OUTER);
                let input = bound.input::<i64>("run")?;
                let output = bound.output::<i64>("value")?;
                assert!(bound.input::<i64>("private").is_err());
                assert!(bound.output::<String>("value").is_err());
                let mut artifacts = CandidateArtifacts::default();
                artifacts.export(context.import("outer")?)?;
                // Only authenticated outer endpoints are used; no provider internals.
                artifacts.patch = Some(NetworkPatch::new([
                    context.expose_bound_input("request", &[input], support::capacity())?,
                    context.expose_bound_output("reply", &output)?,
                ]));
                Ok(artifacts)
            })
            .register(registrar)
    }
}

struct Extension(Option<Arc<ComponentDef>>);

impl PluginInstance for Extension {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::new(
            "extend",
            NetworkPatch::new(self.0.iter().map(|definition| Edit::Replace {
                path: LEAF_PATH.into(),
                definition: definition.clone(),
            })),
        )
        .after([Key::new("provider", "provide")])
        .register(registrar)
    }
}

fn options(leaf_grant: Option<&str>) -> BuildOptions {
    let mut options = BuildOptions::default();
    let selection = (Key::new("consumer", "outer"), Key::new("provider", "outer"));
    options
        .imports
        .selections
        .insert(selection.0.clone(), selection.1.clone());
    options.imports.allow = Some([selection].into());
    support::grant(
        &mut options,
        Key::new("provider", "provide"),
        [Footprint::Component("service".into())],
    );
    support::grant(
        &mut options,
        Key::new("consumer", "consume"),
        [
            Footprint::InputExport("request".into()),
            Footprint::OutputExport("reply".into()),
        ],
    );
    if let Some(path) = leaf_grant {
        support::grant(
            &mut options,
            Key::new("extension", "extend"),
            [Footprint::Component(path.into())],
        );
    }
    options
}

fn execute(generation: &Generation, input: i64) -> i64 {
    generation.inject("request", Value::local(input)).unwrap();
    let report = generation.run_steps(100).unwrap();
    assert!(!report.budget_exhausted);
    generation
        .with_session(|session| {
            assert!(
                session
                    .failure()
                    .map(phenix_graph::Failure::summary)
                    .is_none(),
                "{:?}",
                session.failure().map(phenix_graph::Failure::summary)
            );
            *session
                .take_output("reply")
                .unwrap()
                .expect("outer reply")
                .value()
                .borrow::<i64>()
                .unwrap()
        })
        .unwrap()
}

#[test]
fn three_plugins_replace_a_nested_leaf_atomically_and_retain_old_generations() {
    let mut catalog = PluginCatalog::new();
    let provider = catalog.register("provider", |p: Provider| Ok(p)).unwrap();
    let consumer = catalog.register("consumer", |_: ()| Ok(Consumer)).unwrap();
    let extension = catalog.register("extension", |p: Extension| Ok(p)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let stages = vec![
        host.plugins().prepare_load(&consumer, ()).unwrap(),
        host.plugins()
            .prepare_load(&extension, Extension(None))
            .unwrap(),
        host.plugins()
            .prepare_load(
                &provider,
                Provider {
                    expose_inner: true,
                    expose_leaf: true,
                },
            )
            .unwrap(),
    ];
    let candidate = host
        .prepare(
            stages,
            options(None).with_dispatcher(Arc::new(LocalProcessDispatcher)),
            "nested",
            |_| Ok(()),
        )
        .unwrap();
    assert!(host.active().is_none());
    assert_eq!(host.plugins().snapshots().count(), 0);
    let old = host.publish(candidate).unwrap();
    assert_eq!(execute(&old, 2), 30);
    let provider_snapshot = host.plugins().snapshot(&provider).unwrap();
    let old_extension = host.plugins().snapshot(&extension).unwrap();
    let old_outer = old.compiled().inspect("service").unwrap();
    let old_leaf = old.compiled().inspect(LEAF_PATH).unwrap();
    let old_sibling = old.compiled().inspect("service/inner/sibling").unwrap();
    assert_eq!(old_leaf.origin, old_sibling.origin);

    let stage = host
        .plugins()
        .prepare_reload(
            &extension,
            Extension(Some(support::definition(false, true, 7, LEAF))),
        )
        .unwrap();
    let replacement = stage.candidate().unwrap();
    let candidate = host
        .prepare(
            vec![stage],
            options(Some(LEAF_PATH)).with_dispatcher(Arc::new(LocalProcessDispatcher)),
            "extended",
            |binding| {
                let imported = binding
                    .export::<ResolvedImport>(&Key::new("consumer", "consume"))
                    .unwrap();
                assert_eq!(imported.offer, Key::new("provider", "outer"));
                assert_eq!(imported.producer, Key::new("provider", "provide"));
                Ok(())
            },
        )
        .unwrap();
    assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
    assert_eq!(
        host.plugins().snapshot(&extension).unwrap().revision(),
        old_extension.revision()
    );
    assert_eq!(execute(&old, 3), 40);
    let active = host.publish(candidate).unwrap();
    assert_eq!(active.epoch(), old.epoch() + 1);
    assert_eq!(
        host.plugins().snapshot(&extension).unwrap().revision(),
        replacement.revision()
    );
    assert_eq!(execute(&active, 2), 90);
    assert_eq!(execute(&old, 2), 30);

    let outer = active.compiled().inspect("service").unwrap();
    let inner = active.compiled().inspect("service/inner").unwrap();
    let leaf = active.compiled().inspect(LEAF_PATH).unwrap();
    let sibling = active.compiled().inspect("service/inner/sibling").unwrap();
    let provider_origin = outer.origin.as_ref().unwrap();
    assert!(
        provider_origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
            provider_snapshot.revision_identity().clone(),
        ))
    );
    assert_eq!(outer.declared_origin.as_ref(), Some(provider_origin));
    assert_eq!(outer.interface, old_outer.interface);
    assert_eq!(outer.claims, old_outer.claims);
    assert_eq!(inner.origin, outer.origin);
    assert_eq!(sibling.origin, outer.origin);
    assert!(inner.declared_origin.is_none());
    assert!(sibling.declared_origin.is_none());
    assert_eq!(sibling.definition_key, old_sibling.definition_key);
    assert_eq!(sibling.interface, old_sibling.interface);
    assert_eq!(sibling.claims, old_sibling.claims);
    let leaf_origin = leaf.origin.as_ref().unwrap();
    assert_eq!(leaf.declared_origin.as_ref(), Some(leaf_origin));
    assert_ne!(leaf.definition_key, old_leaf.definition_key);
    assert!(
        leaf_origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
            replacement.revision_identity().clone(),
        ))
    );
    assert!(
        !leaf_origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
            provider_snapshot.revision_identity().clone(),
        ))
    );
    assert!(
        !leaf_origin.belongs_to(&phenix_graph::GraphOrigin::from_root(
            old_extension.revision_identity().clone(),
        ))
    );
    active
        .compiled()
        .check_implementation("service", &support::contract(OUTER))
        .unwrap();
    active
        .compiled()
        .check_implementation(LEAF_PATH, &support::contract(LEAF))
        .unwrap();
    assert_eq!(
        old.compiled().inspect(LEAF_PATH).unwrap().origin,
        old_leaf.origin
    );

    let revisions: Vec<_> = host
        .plugins()
        .snapshots()
        .map(|snapshot| (snapshot.name().to_owned(), snapshot.revision()))
        .collect();
    let claimed = support::definition(false, true, 100, LEAF);
    for (case, expose_inner, expose_leaf, definition, grant) in [
        ("no grant", true, true, claimed.clone(), None),
        (
            "ancestor grant",
            true,
            true,
            claimed.clone(),
            Some("service/inner"),
        ),
        (
            "sibling grant",
            true,
            true,
            claimed.clone(),
            Some("service/inner/sibling"),
        ),
        (
            "private intermediate",
            false,
            true,
            claimed.clone(),
            Some(LEAF_PATH),
        ),
        ("private leaf", true, false, claimed, Some(LEAF_PATH)),
        (
            "incompatible interface",
            true,
            true,
            support::sink(|_| {}),
            Some(LEAF_PATH),
        ),
        (
            "frozen contract",
            true,
            true,
            support::definition(false, false, 100, LEAF),
            Some(LEAF_PATH),
        ),
    ] {
        // Both plugin revisions are staged, and neither may commit on failure.
        let stages = vec![
            host.plugins()
                .prepare_reload(
                    &provider,
                    Provider {
                        expose_inner,
                        expose_leaf,
                    },
                )
                .unwrap(),
            host.plugins()
                .prepare_reload(&extension, Extension(Some(definition)))
                .unwrap(),
        ];
        let error = host
            .prepare(
                stages,
                options(grant).with_dispatcher(Arc::new(LocalProcessDispatcher)),
                case,
                |_| Ok(()),
            )
            .err()
            .expect("invalid nested replacement must fail");
        let LifecycleError::Build(error) = error else {
            panic!("{case}: {error}");
        };
        match (case, error.as_ref()) {
            (
                "no grant" | "ancestor grant" | "sibling grant",
                ResolutionError::Composition {
                    contribution,
                    cause,
                },
            ) => {
                assert_eq!(*contribution, Key::new("extension", "extend"));
                assert!(
                    matches!(cause.downcast_ref::<ComposeError>(), Some(ComposeError::Unauthorized { footprint: Footprint::Component(path), .. }) if path == LEAF_PATH),
                    "{case}: {error}"
                );
            }
            (
                "private intermediate" | "private leaf",
                ResolutionError::Composition {
                    contribution,
                    cause,
                },
            ) => {
                assert_eq!(*contribution, Key::new("extension", "extend"));
                assert!(
                    matches!(
                        cause.downcast_ref::<ComposeError>(),
                        Some(ComposeError::PrivateEditScope(_))
                    ),
                    "{case}: {error}"
                );
            }
            (
                "incompatible interface",
                ResolutionError::Compilation {
                    cause: NetworkError::Interface(error),
                },
            ) => {
                assert_eq!(error.path.as_ref(), LEAF_PATH);
                assert_eq!(
                    error.phase,
                    phenix_graph::interface::InterfacePhase::Authoring
                );
                assert_eq!(error.endpoint.as_deref(), Some("output value"));
                assert_eq!(
                    *error.reason,
                    phenix_graph::interface::InterfaceReason::MissingPort
                );
                assert!(error.expected.is_some());
                assert!(error.actual.is_none());
            }
            (
                "frozen contract",
                ResolutionError::Compilation {
                    cause: NetworkError::Interface(_),
                },
            ) => {}
            _ => panic!("{case}: unexpected failure: {error}"),
        }
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()), "{case}");
        assert_eq!(
            host.plugins()
                .snapshots()
                .map(|snapshot| (snapshot.name().to_owned(), snapshot.revision()))
                .collect::<Vec<_>>(),
            revisions,
            "{case}"
        );
        assert_eq!(execute(&active, 4), 110, "{case}");
        assert_eq!(execute(&old, 4), 50, "{case}");
    }
}
