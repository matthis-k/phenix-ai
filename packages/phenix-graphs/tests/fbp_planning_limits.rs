use phenix_graph::{
    ComponentDef, Contract, GraphOrigin, NetworkBuilder,
    compose::{
        ComposeError, Composer, Edit, NetworkPatch, PlanningLimits, Registration, SelectionLimits,
    },
    network::{NetworkDef, PortPath},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn base() -> (NetworkDef, Contract) {
    let def = ComponentDef::map(|n: i64| n);
    let contract = Contract::new("planning/identity", def.interface().clone()).unwrap();
    let mut builder = NetworkBuilder::new();
    builder
        .add("node", Arc::new(def.claim(&contract).unwrap()))
        .unwrap();
    (builder.build().unwrap(), contract)
}

fn item(id: &str, dependencies: &[&str], priority: i64, patch: NetworkPatch) -> Registration {
    Registration::new(
        id,
        dependencies.iter().map(|id| id.to_string()),
        priority,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    )
}

fn export(name: &str) -> NetworkPatch {
    NetworkPatch::new([Edit::ExposeOutput {
        name: name.into(),
        from: PortPath {
            process: "node".into(),
            port: "output".into(),
        },
    }])
}

fn replace() -> NetworkPatch {
    NetworkPatch::new([Edit::Replace {
        path: "node".into(),
        definition: Arc::new(ComponentDef::map(|n: i64| n).claim(&base().1).unwrap()),
    }])
}

fn admit(registrations: Vec<Registration>, limit: usize) -> (Result<(), ComposeError>, usize) {
    let mut composer = Composer::new(base().0).unwrap();
    composer.set_planning_limits(PlanningLimits { max_units: limit });
    let mut used = 0;
    let result = composer
        .register_with_planning_usage(registrations, &mut used)
        .map(|_| ());
    (result, used)
}

#[test]
fn exact_boundaries_cover_every_reservation_before_conclusion() {
    let mut inner = NetworkBuilder::new();
    for name in ["left", "right"] {
        inner
            .add(name, Arc::new(ComponentDef::map(|n: i64| n)))
            .unwrap();
    }
    let fixtures = [
        vec![item("a", &[], 0, NetworkPatch::new([]))],
        vec![
            item("a", &[], 0, replace()),
            item("b", &["a"], 0, replace()),
        ],
        vec![
            item("a", &[], 0, export("x")),
            item("b", &[], 1, export("x")),
        ],
        vec![item(
            "module",
            &[],
            0,
            NetworkPatch::new([Edit::Add {
                path: "module".into(),
                definition: Arc::new(ComponentDef::composite(inner.build().unwrap())),
            }]),
        )],
    ];
    for fixture in fixtures {
        let (result, cost) = admit(fixture.clone(), usize::MAX);
        result.unwrap();
        for limit in 0..cost {
            let (result, used) = admit(fixture.clone(), limit);
            assert_eq!(result, Err(ComposeError::PlanningBudgetExceeded { limit }));
            assert!(used <= limit);
        }
        assert_eq!(admit(fixture, cost), (Ok(()), cost));
    }
}

#[test]
fn text_query_and_diagnostic_preflight_precedes_semantic_decision() {
    let long = "x".repeat(512);
    let missing = format!("{}y", "x".repeat(511));
    let invalid = item(&long, &[&missing], 0, NetworkPatch::new([]));
    let (conclusion, cost) = admit(vec![invalid.clone()], usize::MAX);
    assert_eq!(
        conclusion,
        Err(ComposeError::MissingDependency {
            registration: long,
            dependency: missing
        })
    );
    assert_eq!(admit(vec![invalid.clone()], cost).0, conclusion);
    assert_eq!(
        admit(vec![invalid], cost - 1).0,
        Err(ComposeError::PlanningBudgetExceeded { limit: cost - 1 })
    );

    let bad_path = format!("{}/..", "z".repeat(512));
    let bad = item(
        "invalid",
        &[],
        0,
        NetworkPatch::new([Edit::Add {
            path: bad_path,
            definition: Arc::new(ComponentDef::map(|n: i64| n)),
        }]),
    );
    let (conclusion, cost) = admit(vec![bad.clone()], usize::MAX);
    assert_eq!(conclusion, Err(ComposeError::PrivateEditScope("..".into())));
    assert_eq!(
        admit(vec![bad], cost - 1).0,
        Err(ComposeError::PlanningBudgetExceeded { limit: cost - 1 })
    );
}

#[test]
fn zero_empty_cumulative_usage_and_larger_limit_retry_preserve_bindings() {
    let (base, contract) = base();
    let mut composer = Composer::new(base).unwrap();
    composer.set_planning_limits(PlanningLimits { max_units: 0 });
    composer.register([]).unwrap();
    assert!(composer.revoke("absent").unwrap().is_empty());
    let old = composer
        .current()
        .compiled()
        .bind("node", &contract)
        .unwrap()
        .output::<i64>("output")
        .unwrap();
    let a = item("a", &[], 0, export("x"));
    assert_eq!(
        composer.register([a.clone()]).unwrap_err(),
        ComposeError::PlanningBudgetExceeded { limit: 0 }
    );
    old.resolve(composer.current().compiled()).unwrap();

    let one = admit(vec![a.clone()], usize::MAX).1;
    composer.set_planning_limits(PlanningLimits { max_units: one });
    let mut used = 0;
    composer
        .register_with_planning_usage([a], &mut used)
        .unwrap();
    assert_eq!(used, one);
    let old = composer
        .current()
        .compiled()
        .bind("node", &contract)
        .unwrap()
        .output::<i64>("output")
        .unwrap();
    let b = item("b", &[], 0, NetworkPatch::new([]));
    assert_eq!(
        composer
            .register_with_planning_usage([b.clone()], &mut used)
            .unwrap_err(),
        ComposeError::PlanningBudgetExceeded { limit: one }
    );
    assert_eq!(used, one);
    old.resolve(composer.current().compiled()).unwrap();
    composer.set_planning_limits(PlanningLimits { max_units: 0 });
    assert_eq!(
        composer.revoke("a").unwrap_err(),
        ComposeError::PlanningBudgetExceeded { limit: 0 }
    );
    old.resolve(composer.current().compiled()).unwrap();
    composer.set_planning_limits(PlanningLimits::default());
    composer
        .register_with_planning_usage([b], &mut used)
        .unwrap();
    assert!(old.resolve(composer.current().compiled()).is_err());
    assert_eq!(composer.revoke("b").unwrap(), ["b".into()].into());
    assert!(composer.current().active().contains("a"));
}

#[test]
fn selector_zero_is_not_a_validation_shortcut_or_a_planner_storage_bound() {
    let dense: Vec<_> = (0..64)
        .map(|i| item(&format!("r{i:03}"), &[], i, export("x")))
        .collect();
    let mut composer = Composer::new(base().0).unwrap();
    composer.set_selection_limits(SelectionLimits { max_work: 0 });
    composer.set_planning_limits(PlanningLimits { max_units: 20_000 });
    assert_eq!(
        composer.register(dense.clone()).unwrap_err(),
        ComposeError::PlanningBudgetExceeded { limit: 20_000 }
    );
    composer.set_planning_limits(PlanningLimits {
        max_units: 2_000_000,
    });
    assert_eq!(
        composer.register(dense).unwrap_err(),
        ComposeError::SelectionBudgetExceeded { limit: 0 }
    );
    assert!(matches!(
        composer.register([item("missing", &["absent"], 0, NetworkPatch::new([]))]),
        Err(ComposeError::MissingDependency { .. })
    ));
    assert!(composer.current().active().is_empty());
}

#[test]
fn default_is_new_policy_not_compatibility_for_unbounded_identifiers() {
    let registrations: Vec<_> = (0..2048)
        .map(|i| {
            let id = format!("{i:04}{}", "x".repeat(508));
            assert_eq!(id.len(), 512);
            item(&id, &[], 0, NetworkPatch::new([]))
        })
        .collect();
    let (result, _) = admit(registrations.clone(), PlanningLimits::default().max_units);
    assert_eq!(
        result,
        Err(ComposeError::PlanningBudgetExceeded { limit: 1_000_000 })
    );
    let (result, used) = admit(registrations.clone(), 50_000_000);
    result.unwrap();
    assert!(used > 1_048_576);
    println!("2048 x 512-byte IDs: successful_planning_units={used}");
    let mut composer = Composer::new(base().0).unwrap();
    composer.set_planning_limits(PlanningLimits {
        max_units: 50_000_000,
    });
    composer.register(registrations).unwrap();
    composer.set_planning_limits(PlanningLimits::default());
    assert_eq!(
        composer.register([]).unwrap_err(),
        ComposeError::PlanningBudgetExceeded { limit: 1_000_000 }
    );
    assert_eq!(composer.current().active().len(), 2048);
    composer.set_planning_limits(PlanningLimits {
        max_units: 50_000_000,
    });
    composer.register([]).unwrap();
}

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("payload cleanup");
    }
}

fn bomb_item(id: &str, drops: Arc<AtomicUsize>) -> Registration {
    let bomb = Bomb(drops);
    item(
        id,
        &[],
        0,
        NetworkPatch::new([Edit::Replace {
            path: "node".into(),
            definition: Arc::new(
                ComponentDef::map(move |n: i64| {
                    let _ = &bomb;
                    n
                })
                .claim(&base().1)
                .unwrap(),
            ),
        }]),
    )
}

struct Iter {
    item: Option<Registration>,
    panic_next: bool,
    panic_drop: bool,
    drops: Arc<AtomicUsize>,
}
impl Iterator for Iter {
    type Item = Registration;
    fn next(&mut self) -> Option<Registration> {
        if self.item.is_some() {
            return self.item.take();
        }
        assert!(!self.panic_next, "iterator next");
        None
    }
}
impl Drop for Iter {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic_drop, "iterator drop");
    }
}

struct IntoPanic;
impl IntoIterator for IntoPanic {
    type Item = Registration;
    type IntoIter = std::iter::Empty<Registration>;
    fn into_iter(self) -> Self::IntoIter {
        panic!("into_iter")
    }
}

#[test]
fn iterator_conversion_next_and_drop_are_precommit_transactions() {
    let (base, contract) = base();
    let mut composer = Composer::new(base).unwrap();
    let bound = composer
        .current()
        .compiled()
        .bind("node", &contract)
        .unwrap()
        .output::<i64>("output")
        .unwrap();
    assert_eq!(
        composer.register(IntoPanic).unwrap_err(),
        ComposeError::Panicked
    );
    for (panic_next, panic_drop) in [(true, false), (false, true), (true, true)] {
        let drops = Arc::new(AtomicUsize::new(0));
        let iterator = Iter {
            item: Some(item("rejected", &[], 0, export("out"))),
            panic_next,
            panic_drop,
            drops: drops.clone(),
        };
        assert_eq!(
            composer.register(iterator).unwrap_err(),
            ComposeError::Panicked
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(composer.current().active().is_empty());
        bound.resolve(composer.current().compiled()).unwrap();
    }
    composer
        .register([item("rejected", &[], 0, export("out"))])
        .unwrap();
}

#[test]
fn established_errors_survive_rejected_iterator_and_payload_cleanup_panics() {
    let mut composer = Composer::new(base().0).unwrap();
    composer
        .register([item("duplicate", &[], 0, NetworkPatch::new([]))])
        .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let iterator = Iter {
        item: Some(bomb_item("duplicate", drops.clone())),
        panic_next: false,
        panic_drop: true,
        drops: drops.clone(),
    };
    assert_eq!(
        composer.register(iterator).unwrap_err(),
        ComposeError::DuplicateRegistration("duplicate".into())
    );
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    composer.set_planning_limits(PlanningLimits { max_units: 0 });
    let drops = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        composer
            .register([bomb_item("budget", drops.clone())])
            .unwrap_err(),
        ComposeError::PlanningBudgetExceeded { limit: 0 }
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(composer.current().active(), &["duplicate".into()].into());
}

#[test]
fn retired_payload_panic_cannot_turn_a_committed_revoke_into_failure() {
    let mut composer = Composer::new(base().0).unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    composer
        .register([bomb_item("retired", drops.clone())])
        .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        composer.revoke("retired").unwrap(),
        ["retired".into()].into()
    );
    assert!(composer.current().active().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    composer
        .register([item("after", &[], 0, NetworkPatch::new([]))])
        .unwrap();
}

#[test]
fn rejected_candidate_cleanup_cannot_replace_a_conclusive_authority_error() {
    let mut composer = Composer::new(base().0).unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let item = bomb_item("unauthorized", drops.clone());
    let unauthorized = Registration::new(
        "unauthorized",
        [],
        0,
        GraphOrigin::default(),
        [],
        item.patch().clone(),
    );
    drop(item);
    assert!(matches!(
        composer.register([unauthorized]),
        Err(ComposeError::Unauthorized { .. })
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(composer.current().active().is_empty());
}

#[test]
fn revoke_has_one_exact_allowance_for_closure_copy_and_remaining_plan() {
    let attempt = |limit| {
        let (base, contract) = base();
        let mut composer = Composer::new(base).unwrap();
        composer
            .register([
                item("root", &[], 0, NetworkPatch::new([])),
                item("child", &["root"], 0, export("child")),
                item("keep", &[], 0, export("keep")),
            ])
            .unwrap();
        let old = composer
            .current()
            .compiled()
            .bind("node", &contract)
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer.set_planning_limits(PlanningLimits { max_units: limit });
        match composer.revoke("root") {
            Ok(removed) => {
                assert_eq!(removed, ["root".into(), "child".into()].into());
                assert_eq!(composer.current().active(), &["keep".into()].into());
                assert!(old.resolve(composer.current().compiled()).is_err());
                true
            }
            Err(error) => {
                assert_eq!(error, ComposeError::PlanningBudgetExceeded { limit });
                old.resolve(composer.current().compiled()).unwrap();
                assert_eq!(composer.current().active().len(), 3);
                false
            }
        }
    };
    let (mut low, mut high) = (0, 10_000);
    assert!(attempt(high));
    while low < high {
        let middle = low + (high - low) / 2;
        if attempt(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(!attempt(low - 1));
    assert!(attempt(low));
    println!("revoke closure+remaining writer exact_planning_units={low}");
}

#[test]
fn successful_dense_shuffled_priorities_publish_exact_winners_then_recover() {
    for seed in 0..8 {
        let mut ranked: Vec<_> = (0..64)
            .map(|i| (format!("r{i:04}"), ((i * 17 + seed) % 64) as i64))
            .collect();
        let mut composer = Composer::new(base().0).unwrap();
        composer
            .register(
                ranked
                    .iter()
                    .rev()
                    .map(|(id, priority)| item(id, &[], *priority, export("capture"))),
            )
            .unwrap();
        ranked.sort_by_key(|(_, priority)| *priority);
        let (winner, _) = ranked.pop().unwrap();
        assert_eq!(composer.current().active(), &[winner.clone()].into());
        assert_eq!(composer.revoke(&winner).unwrap(), [winner].into());
        assert_eq!(
            composer.current().active(),
            &[ranked.last().unwrap().0.clone()].into()
        );
        assert_eq!(
            composer
                .current()
                .network()
                .compile()
                .unwrap()
                .processes()
                .len(),
            1
        );
    }
}
