use phenix_graph::{
    DeclaredValue, GraphBuilder, GraphContract, InterfaceShape, LocalExecutor, Value,
};
use phenix_kernel::{
    ContractDefinition, ContractRequirement, Contribution, GraphOffer, ImportPolicy, Kernel, Key,
    ResolutionError,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, PluginSnapshot, Registrar};
use std::collections::BTreeSet;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn contract(id: &str) -> GraphContract {
    GraphContract::new(
        id,
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

#[derive(Clone)]
struct Provider {
    path: String,
    module: bool,
    increment: i64,
    semantic: &'static str,
    requires: Vec<Key>,
    producer: Option<Key>,
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
            producer: None,
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
        into.add(GraphOffer {
            id: "offer".into(),
            unit: self.path.clone(),
            producer: self
                .producer
                .clone()
                .unwrap_or_else(|| Key::new(&owner, "main")),
            contract: Key::new(&owner, "definition"),
        })?;
        let config = self.clone();
        into.add(Contribution {
            id: self.actual_composer.into(),
            requires: self.requires.clone(),
            routes: vec![],
            compose: Box::new(move |context| {
                config.compositions.fetch_add(1, Ordering::SeqCst);
                let graph = &mut *context.graph;
                let increment = config.increment;
                if config.module {
                    let mut child = GraphBuilder::new(format!("module-{}", config.path));
                    let node = child.node("private", move |v: i64| v + increment)?;
                    child.expose_input_slot("run", "value", node.inlet())?;
                    child.expose_output("value", node.output())?;
                    child.implements_public(&contract(config.semantic))?;
                    graph.instance(&config.path, &child.module())?;
                } else {
                    graph.node(&config.path, move |v: i64| v + increment)?;
                    graph.implements(&config.path, &contract(config.semantic))?;
                }
                Ok(())
            }),
        })?;
        if self.actual_composer != "main" {
            into.add(Contribution {
                id: "main".into(),
                requires: vec![Key::new(&owner, self.actual_composer)],
                routes: vec![],
                compose: Box::new(|_| Ok(())),
            })?;
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
        into.add(ContractRequirement {
            id: "service".into(),
            consumer: Key::new(into.owner(), "main"),
            contract: Key::new("z-provider", "definition"),
        })?;
        let config = self.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec![],
            compose: Box::new(move |context| {
                let selected = context.import("service")?;
                assert!(context.import("undeclared").is_err());
                config.compositions.fetch_add(1, Ordering::SeqCst);
                let graph = &mut *context.graph;
                let bound = selected.bind(graph)?;
                assert!(bound.local_output::<i64>("private").is_err());
                assert!(bound.local_output::<String>("value").is_err());
                let input = bound.local_input::<i64>("run", "value")?;
                let output = bound.local_output::<i64>("value")?;
                let observed = config.observed.clone();
                let sink = graph.node("sink", move |v: i64| {
                    observed.lock().unwrap().push(v);
                    v
                })?;
                graph.connect(output, sink.inlet())?;
                let public = graph.expose_input("request", input)?;
                graph.trigger("run", [public])?;
                Ok(())
            }),
        })
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
fn build(snapshots: Vec<PluginSnapshot>, policy: &ImportPolicy) -> Result<Kernel, ResolutionError> {
    Kernel::build_with_options(
        "imports",
        snapshots,
        Arc::new(LocalExecutor),
        phenix_kernel::BuildOptions {
            imports: policy.clone(),
            ..Default::default()
        },
    )
}

#[test]
fn every_policy_table_rejects_unknown_requirement_and_offer_keys() {
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
            let error = build(plugins.snapshots().collect(), &policy)
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
        let error = build(snapshots.clone(), &ImportPolicy::default())
            .err()
            .unwrap();
        assert!(
            matches!(error, ResolutionError::Ambiguous { requirement: key, offers }
            if key == requirement() && offers == vec![offer("y-provider"), offer("z-provider")])
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let mut old = build(snapshots.clone(), &selected("z-provider")).unwrap();
        let mut switched = build(
            snapshots.into_iter().rev().collect(),
            &selected("y-provider"),
        )
        .unwrap();
        old.start().unwrap();
        switched.start().unwrap();
        assert!(
            old.fire("run", vec![Value::local(1_i64)])
                .unwrap()
                .success()
        );
        assert!(
            switched
                .fire("run", vec![Value::local(1_i64)])
                .unwrap()
                .success()
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
        )
        .unwrap();
        plugins.commit(staged).unwrap();
        new.start().unwrap();
        assert!(
            new.fire("run", vec![Value::local(1_i64)])
                .unwrap()
                .success()
        );
        assert!(
            old.fire("run", vec![Value::local(2_i64)])
                .unwrap()
                .success()
        );
        assert_eq!(*observed.lock().unwrap(), [11, 21, 31, 12]);
    }
}

#[test]
fn eligibility_semantics_cycles_and_exact_ownership_fail_closed() {
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
        build(plugins.snapshots().collect(), &selected("y-provider")),
        Err(ResolutionError::Ineligible { .. })
    ));
    let pair = (requirement(), offer("z-provider"));
    let mut policy = ImportPolicy {
        allow: Some(BTreeSet::new()),
        ..Default::default()
    };
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy),
        Err(ResolutionError::Missing { .. })
    ));
    policy.allow.as_mut().unwrap().insert(pair.clone());
    policy.deny.insert(pair.clone());
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy),
        Err(ResolutionError::Missing { .. })
    ));
    policy.selections.insert(requirement(), offer("z-provider"));
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy),
        Err(ResolutionError::Ineligible { .. })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    policy.deny.clear();
    policy.selections.clear();
    build(plugins.snapshots().collect(), &policy).unwrap();
    let mut cycle = Provider::new("first", false, 10, &count);
    cycle.requires.push(Key::new("a-consumer", "main"));
    plugins.reload(&z, cycle).unwrap();
    let before = count.load(Ordering::SeqCst);
    assert!(matches!(
        build(plugins.snapshots().collect(), &policy),
        Err(ResolutionError::Cycle(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), before);
    let mut foreign = Provider::new("first", false, 10, &count);
    foreign.producer = Some(Key::new("y-provider", "main"));
    plugins.reload(&z, foreign).unwrap();
    assert!(
        build(plugins.snapshots().collect(), &policy)
            .err()
            .unwrap()
            .to_string()
            .contains("ownership")
    );
    let mut sibling = Provider::new("first", false, 10, &count);
    sibling.actual_composer = "sibling";
    plugins.reload(&z, sibling).unwrap();
    let error = build(plugins.snapshots().collect(), &policy)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("origin mismatch"), "{error}");
}

#[test]
fn duplicate_owner_is_rejected_even_for_distinct_revisions_before_composition() {
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
        matches!(build(vec![old, staged.candidate().unwrap()], &ImportPolicy::default()),
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
            into.add(GraphOffer {
                id: "offer".into(),
                producer: Key::new("combined", "main"),
                unit: "provider".into(),
                contract: Key::new("combined", "definition"),
            })?;
            into.add(ContractRequirement {
                id: "service".into(),
                consumer: Key::new("combined", "main"),
                contract: Key::new("combined", "definition"),
            })?;
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec![],
                compose: Box::new(|cx| {
                    let selected = cx.import("service")?;
                    cx.graph.node("provider", |v: i64| v + 1)?;
                    cx.graph.implements("provider", &contract("service@1"))?;
                    let input = selected
                        .bind(cx.graph)?
                        .local_input::<i64>("run", "value")?;
                    let public = cx.graph.expose_input("request", input)?;
                    cx.graph.trigger("run", [public])?;
                    Ok(())
                }),
            })
        }
    }
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("combined", |_: ()| Ok(Combined)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&key, ()).unwrap();
    for explicit in [false, true] {
        let mut policy = ImportPolicy::default();
        if explicit {
            policy.selections.insert(
                Key::new("combined", "service"),
                Key::new("combined", "offer"),
            );
        }
        let mut kernel = build(plugins.snapshots().collect(), &policy).unwrap();
        kernel.start().unwrap();
        assert!(
            kernel
                .fire("run", vec![Value::local(1_i64)])
                .unwrap()
                .success()
        );
    }
}
