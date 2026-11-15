mod support;
use phenix_graph::{
    compose::{Edit, Footprint, NetworkPatch},
    contract::Contract,
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, ContractDefinition, Contribution, ImportPolicy, Kernel, Key,
    ResolutionError,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, PluginSnapshot, Registrar};
use std::collections::BTreeSet;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn contract(id: &str) -> Contract {
    support::contract(id)
}

#[derive(Clone)]
struct Provider {
    path: String,
    module: bool,
    increment: i64,
    semantic: &'static str,
    requires: Vec<Key>,
    advertised_path: Option<String>,
    actual_composer: &'static str,
    compositions: Arc<AtomicUsize>,
}

impl Provider {
    fn new(path: &str, module: bool, increment: i64, compositions: &Arc<AtomicUsize>) -> Self {
        Self {
            path: path.into(),
            module,
            increment,
            semantic: "service@1",
            requires: vec![],
            advertised_path: None,
            actual_composer: "main",
            compositions: compositions.clone(),
        }
    }
}

impl PluginInstance for Provider {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let owner = into.owner().to_owned();
        into.add(ContractDefinition {
            id: "definition".into(),
            contract: contract(self.semantic),
        })?;
        let config = self.clone();
        let mut contribution = Contribution::new(
            self.actual_composer,
            support::patch(&self.path, self.module, true, self.increment, self.semantic),
        )
        .after(self.requires.clone())
        .prepare(move |_| {
            config.compositions.fetch_add(1, Ordering::SeqCst);
            Ok(CandidateArtifacts::default())
        });
        if self.actual_composer == "main" {
            contribution = contribution.offer(
                "offer",
                self.advertised_path.as_ref().unwrap_or(&self.path).clone(),
                Key::new(&owner, "definition"),
            );
        }
        contribution.register(into)?;
        if self.actual_composer != "main" {
            Contribution::new("main", NetworkPatch::new([]))
                .after([Key::new(&owner, self.actual_composer)])
                .offer("offer", self.path.clone(), Key::new(&owner, "definition"))
                .register(into)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Consumer {
    observed: Arc<Mutex<Vec<i64>>>,
    compositions: Arc<AtomicUsize>,
}

impl PluginInstance for Consumer {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let config = self.clone();
        Contribution::new("main", NetworkPatch::new([]))
            .import("service", Key::new("z-provider", "definition"))
            .prepare(move |context| {
                assert!(context.import("service").is_ok());
                assert!(context.import("undeclared").is_err());
                assert!(context.bind_import("undeclared").is_err());
                assert!(context.inspect_network().is_err());
                config.compositions.fetch_add(1, Ordering::SeqCst);
                let bound = context.bind_import("service")?;
                assert!(bound.input::<i64>("private").is_err());
                assert!(bound.output::<String>("value").is_err());
                bound.input::<i64>("run")?;
                bound.output::<i64>("value")?;
                let mut artifacts = CandidateArtifacts::default();
                artifacts.export(config.observed.clone())?;
                artifacts.patch = Some(NetworkPatch::new([
                    Edit::ExposeInput {
                        name: "request".into(),
                        targets: vec![support::path(bound.path(), "run")],
                        capacity: support::capacity(),
                    },
                    Edit::ExposeOutput {
                        name: "result".into(),
                        from: support::path(bound.path(), "value"),
                    },
                ]));
                Ok(artifacts)
            })
            .register(into)
    }
}

fn requirement() -> Key {
    Key::new("a-consumer", "service")
}
fn offer(owner: &str) -> Key {
    Key::new(owner, "offer")
}
fn selected(owner: &str) -> ImportPolicy {
    let mut policy = ImportPolicy::default();
    policy.selections.insert(requirement(), offer(owner));
    policy
}
fn options(writes: &[(&str, &str, &str)], consumer: bool) -> BuildOptions {
    let mut options = BuildOptions::default();
    for (owner, composer, path) in writes {
        support::grant(
            &mut options,
            Key::new(*owner, *composer),
            [Footprint::Component((*path).into())],
        );
    }
    if consumer {
        support::grant(
            &mut options,
            Key::new("a-consumer", "main"),
            [
                Footprint::InputExport("request".into()),
                Footprint::OutputExport("result".into()),
            ],
        );
    }
    options
}
fn build(
    snapshots: Vec<PluginSnapshot>,
    policy: &ImportPolicy,
    granted: &BuildOptions,
) -> Result<Kernel, ResolutionError> {
    let options = BuildOptions {
        composition: granted.composition.clone(),
        imports: policy.clone(),
        ..Default::default()
    };
    Kernel::build_with_options(
        "imports",
        snapshots,
        Arc::new(LocalProcessDispatcher),
        options,
    )
}

fn execute(kernel: &Kernel, value: i64) {
    support::run(kernel, "request", value);
    let value = kernel
        .with_session(|session| {
            session
                .take_output("result")
                .unwrap()
                .unwrap()
                .value()
                .borrow::<i64>()
                .copied()
                .unwrap()
        })
        .unwrap();
    kernel
        .export::<Arc<Mutex<Vec<i64>>>>(&Key::new("a-consumer", "main"))
        .unwrap()
        .lock()
        .unwrap()
        .push(value);
}

#[test]
fn every_policy_table_rejects_unknown_requirement_and_offer_keys() {
    let granted = options(&[("z-provider", "main", "service")], true);
    let count = Arc::new(AtomicUsize::new(0));
    let mut catalog = PluginCatalog::new();
    let z = catalog.register("z-provider", |p: Provider| Ok(p)).unwrap();
    let a = catalog.register("a-consumer", |p: Consumer| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(&z, Provider::new("service", false, 1, &count))
        .unwrap();
    plugins
        .load(
            &a,
            Consumer {
                observed: Arc::default(),
                compositions: count.clone(),
            },
        )
        .unwrap();
    for table in 0..3 {
        for unknown_requirement in [false, true] {
            let requirement = if unknown_requirement {
                Key::new("stale", "requirement")
            } else {
                requirement()
            };
            let offer = if unknown_requirement {
                offer("z-provider")
            } else {
                offer("stale")
            };
            let mut policy = ImportPolicy::default();
            match table {
                0 => {
                    policy.selections.insert(requirement, offer);
                }
                1 => {
                    policy.allow = Some(BTreeSet::from([(requirement, offer)]));
                }
                _ => {
                    policy.deny.insert((requirement, offer));
                }
            }
            let error = build(plugins.snapshots().collect(), &policy, &granted)
                .err()
                .expect("stale policy must fail closed");
            assert!(error.to_string().contains("unknown"), "{error}");
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn node_and_module_providers_switch_and_reload_without_consumer_paths() {
    for module in [false, true] {
        let granted = options(
            &[
                ("z-provider", "main", "first"),
                ("y-provider", "main", "second"),
            ],
            true,
        );
        let mut catalog = PluginCatalog::new();
        let z = catalog.register("z-provider", |p: Provider| Ok(p)).unwrap();
        let y = catalog.register("y-provider", |p: Provider| Ok(p)).unwrap();
        let a = catalog.register("a-consumer", |p: Consumer| Ok(p)).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new(Mutex::new(vec![]));
        let mut plugins = catalog.instance();
        plugins
            .load(
                &a,
                Consumer {
                    observed: observed.clone(),
                    compositions: count.clone(),
                },
            )
            .unwrap();
        plugins
            .load(&z, Provider::new("first", module, 10, &count))
            .unwrap();
        plugins
            .load(&y, Provider::new("second", !module, 20, &count))
            .unwrap();
        let snapshots: Vec<_> = plugins.snapshots().collect();
        let error = build(snapshots.clone(), &ImportPolicy::default(), &granted)
            .err()
            .unwrap();
        assert!(
            matches!(error, ResolutionError::Ambiguous { requirement: key, offers }
            if key == requirement() && offers == vec![offer("y-provider"), offer("z-provider")])
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let mut old = build(snapshots.clone(), &selected("z-provider"), &granted).unwrap();
        let mut switched = build(
            snapshots.into_iter().rev().collect(),
            &selected("y-provider"),
            &granted,
        )
        .unwrap();
        old.start().unwrap();
        switched.start().unwrap();
        execute(&old, 1);
        execute(&switched, 1);
        let granted = options(
            &[
                ("z-provider", "main", "renamed"),
                ("y-provider", "main", "second"),
            ],
            true,
        );
        let staged = plugins
            .prepare_reload(&z, Provider::new("renamed", module, 30, &count))
            .unwrap();
        let candidate = staged.candidate().unwrap();
        let mut new = build(
            plugins
                .snapshots()
                .filter(|s| s.name() != "z-provider")
                .chain([candidate])
                .collect(),
            &selected("z-provider"),
            &granted,
        )
        .unwrap();
        plugins.commit(staged).unwrap();
        new.start().unwrap();
        execute(&new, 1);
        execute(&old, 2);
        assert_eq!(*observed.lock().unwrap(), [11, 21, 31, 12]);
    }
}

#[test]
fn eligibility_semantics_cycles_and_exact_ownership_fail_closed() {
    let granted = options(
        &[
            ("z-provider", "main", "first"),
            ("y-provider", "main", "second"),
        ],
        true,
    );
    let mut catalog = PluginCatalog::new();
    let z = catalog.register("z-provider", |p: Provider| Ok(p)).unwrap();
    let y = catalog.register("y-provider", |p: Provider| Ok(p)).unwrap();
    let a = catalog.register("a-consumer", |p: Consumer| Ok(p)).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let mut plugins = catalog.instance();
    plugins
        .load(
            &a,
            Consumer {
                observed: Arc::default(),
                compositions: count.clone(),
            },
        )
        .unwrap();
    plugins
        .load(&z, Provider::new("first", false, 10, &count))
        .unwrap();
    let mut unrelated = Provider::new("second", true, 20, &count);
    unrelated.semantic = "unrelated@1";
    plugins.load(&y, unrelated).unwrap();
    assert!(matches!(
        build(
            plugins.snapshots().collect(),
            &selected("y-provider"),
            &granted
        ),
        Err(ResolutionError::Ineligible { .. })
    ));
    let pair = (requirement(), offer("z-provider"));
    let mut policy = ImportPolicy {
        allow: Some(BTreeSet::new()),
        ..Default::default()
    };
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy, &granted),
        Err(ResolutionError::Missing { .. })
    ));
    policy.allow.as_mut().unwrap().insert(pair.clone());
    policy.deny.insert(pair.clone());
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy, &granted),
        Err(ResolutionError::Missing { .. })
    ));
    policy.selections.insert(requirement(), offer("z-provider"));
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy, &granted),
        Err(ResolutionError::Ineligible { .. })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    policy.deny.clear();
    policy.selections.clear();
    build(plugins.snapshots().collect(), &policy, &granted).unwrap();
    let mut cycle = Provider::new("first", false, 10, &count);
    cycle.requires.push(Key::new("a-consumer", "main"));
    plugins.reload(&z, cycle).unwrap();
    let before = count.load(Ordering::SeqCst);
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy, &granted),
        Err(ResolutionError::Cycle(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), before);
    let mut foreign = Provider::new("first", false, 10, &count);
    foreign.advertised_path = Some("second".into());
    plugins
        .reload(&y, Provider::new("second", true, 20, &count))
        .unwrap();
    policy = selected("y-provider");
    plugins.reload(&z, foreign).unwrap();
    let error = build(plugins.snapshots().collect(), &policy, &granted)
        .err()
        .unwrap();
    assert!(
        matches!(&error, ResolutionError::WrongOrigin { offer, producer, unit }
        if *offer == Key::new("z-provider", "offer") && **producer == Key::new("z-provider", "main") && unit == "second"),
        "{error}"
    );
    let mut sibling = Provider::new("first", false, 10, &count);
    sibling.actual_composer = "sibling";
    let granted = options(
        &[
            ("z-provider", "sibling", "first"),
            ("y-provider", "main", "second"),
        ],
        true,
    );
    plugins.reload(&z, sibling).unwrap();
    let error = build(plugins.snapshots().collect(), &policy, &granted)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("origin mismatch"), "{error}");
}

#[test]
fn duplicate_owner_is_rejected_even_for_distinct_revisions_before_composition() {
    let granted = BuildOptions::default();
    let mut catalog = PluginCatalog::new();
    let z = catalog.register("z-provider", |p: Provider| Ok(p)).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let mut plugins = catalog.instance();
    let old = plugins
        .load(&z, Provider::new("old", false, 1, &count))
        .unwrap();
    let staged = plugins
        .prepare_reload(&z, Provider::new("new", true, 2, &count))
        .unwrap();
    assert!(
        matches!(build(vec![old, staged.candidate().unwrap()], &ImportPolicy::default(), &granted),
        Err(ResolutionError::DuplicateOwner(owner)) if owner == "z-provider")
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn self_imports_do_not_create_an_inter_composer_cycle() {
    struct Combined;
    impl PluginInstance for Combined {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(ContractDefinition {
                id: "definition".into(),
                contract: contract("service@1"),
            })?;
            Contribution::new(
                "main",
                NetworkPatch::new([
                    Edit::Add {
                        path: "provider".into(),
                        definition: support::definition(false, true, 1, "service@1"),
                    },
                    Edit::ExposeInput {
                        name: "request".into(),
                        targets: vec![support::path("provider", "run")],
                        capacity: support::capacity(),
                    },
                ]),
            )
            .offer("offer", "provider", Key::new("combined", "definition"))
            .import("service", Key::new("combined", "definition"))
            .prepare(|cx| {
                cx.bind_import("service")?.input::<i64>("run")?;
                Ok(CandidateArtifacts::default())
            })
            .register(into)
        }
    }
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("combined", |_: ()| Ok(Combined)).unwrap();
    let mut plugins = catalog.instance();
    let granted = [false, true].map(|explicit| {
        let mut options = BuildOptions::default();
        if explicit {
            options.imports.selections.insert(
                Key::new("combined", "service"),
                Key::new("combined", "offer"),
            );
        }
        support::grant(
            &mut options,
            Key::new("combined", "main"),
            [
                Footprint::Component("provider".into()),
                Footprint::InputExport("request".into()),
            ],
        );
        options
    });
    plugins.load(&key, ()).unwrap();
    for options in granted {
        let mut kernel = Kernel::build_with_options(
            "imports",
            plugins.snapshots(),
            Arc::new(LocalProcessDispatcher),
            options,
        )
        .unwrap();
        kernel.start().unwrap();
        support::run(&kernel, "request", 1);
    }
}
