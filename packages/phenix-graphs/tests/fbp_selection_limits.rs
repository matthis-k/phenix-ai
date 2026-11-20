use phenix_graph::compose::{
    ComposeError, Composer, Edit, NetworkPatch, Registration, SelectionLimits,
};
use phenix_graph::network::{NetworkDef, NetworkError, PortPath};
use phenix_graph::{ComponentDef, Contract, GraphOrigin, NetworkBuilder};
use std::{collections::BTreeSet, sync::Arc};

fn base() -> (NetworkDef, Contract) {
    let definition = ComponentDef::map(|value: i64| value);
    let contract = Contract::new("selection/identity", definition.interface().clone()).unwrap();
    let mut builder = NetworkBuilder::new();
    builder
        .add("node", Arc::new(definition.claim(&contract).unwrap()))
        .unwrap();
    (builder.build().unwrap(), contract)
}

fn registration(
    id: String,
    dependencies: Vec<String>,
    priority: i64,
    exports: &[String],
) -> Registration {
    let patch = NetworkPatch::new(exports.iter().map(|name| Edit::ExposeOutput {
        name: name.clone(),
        from: PortPath {
            process: "node".into(),
            port: "output".into(),
        },
    }));
    Registration::new(
        id,
        dependencies,
        priority,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    )
}

fn minimum_work(base: &NetworkDef, registrations: &[Registration]) -> usize {
    let accepts = |limit| {
        let mut composer = Composer::new(base.clone()).unwrap();
        composer.set_selection_limits(SelectionLimits { max_work: limit });
        match composer.register(registrations.to_vec()) {
            Ok(_) => true,
            Err(ComposeError::SelectionBudgetExceeded { limit: actual }) => {
                assert_eq!(actual, limit);
                false
            }
            Err(error) => panic!("expected a unique fixture, got {error}"),
        }
    };
    let (mut low, mut high) = (0, SelectionLimits::default().max_work);
    assert!(accepts(high), "fixture must fit the documented default");
    while low < high {
        let middle = low + (high - low) / 2;
        if accepts(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    if low != 0 {
        assert!(!accepts(low - 1));
    }
    assert!(accepts(low));
    low
}

#[test]
fn limits_change_no_binding_and_failed_register_or_revoke_retains_registry() {
    let (base, contract) = base();
    let mut composer = Composer::new(base).unwrap();
    let published = composer.current().compiled().clone();
    let binding = published
        .bind("node", &contract)
        .unwrap()
        .input::<i64>("input")
        .unwrap();
    composer.set_selection_limits(SelectionLimits { max_work: 0 });
    binding.resolve(composer.current().compiled()).unwrap();
    let item = registration("item".into(), vec![], 0, &[]);
    assert_eq!(
        composer.register([item.clone()]).unwrap_err(),
        ComposeError::SelectionBudgetExceeded { limit: 0 }
    );
    assert!(composer.current().active().is_empty());
    binding.resolve(composer.current().compiled()).unwrap();
    composer.set_selection_limits(SelectionLimits::default());
    binding.resolve(composer.current().compiled()).unwrap();
    composer
        .register([item.clone(), registration("keep".into(), vec![], 0, &[])])
        .unwrap();
    assert!(matches!(
        binding.resolve(composer.current().compiled()),
        Err(NetworkError::ForeignScope)
    ));
    let published = composer.current().compiled().clone();
    let binding = published
        .bind("node", &contract)
        .unwrap()
        .input::<i64>("input")
        .unwrap();
    composer.set_selection_limits(SelectionLimits { max_work: 0 });
    assert_eq!(
        composer.revoke("item").unwrap_err(),
        ComposeError::SelectionBudgetExceeded { limit: 0 }
    );
    binding.resolve(composer.current().compiled()).unwrap();
    assert_eq!(
        composer.register([item]).unwrap_err(),
        ComposeError::DuplicateRegistration("item".into())
    );
    assert_eq!(
        composer.current().active(),
        &BTreeSet::from(["item".into(), "keep".into()])
    );
    composer.set_selection_limits(SelectionLimits::default());
    assert_eq!(
        composer.revoke("item").unwrap(),
        BTreeSet::from(["item".into()])
    );
    assert_eq!(
        composer.revoke("keep").unwrap(),
        BTreeSet::from(["keep".into()])
    );
    assert!(composer.current().active().is_empty());
}

#[test]
fn independent_registrations_have_linear_solver_allowances_not_subset_search() {
    let (base, _) = base();
    let mut previous = None;
    for count in [32, 64, 128, 256] {
        let registrations: Vec<_> = (0..count)
            .map(|i| registration(format!("r{i:04}"), vec![], 0, &[]))
            .collect();
        let work = minimum_work(&base, &registrations);
        assert!(work <= 12 * count, "count={count}, work={work}");
        if let Some(previous) = previous {
            assert_eq!(work, 2 * previous);
        }
        previous = Some(work);
    }
}

#[test]
fn repeated_transient_defeat_resolves_in_linear_solver_work() {
    let (base, _) = base();
    let mut previous = None;
    for groups in [4, 8, 16, 32] {
        let mut registrations = Vec::new();
        let mut expected = BTreeSet::new();
        for i in 0..groups {
            let (a, b, x) = (format!("a{i}"), format!("b{i}"), format!("x{i}"));
            for (id, deps, priority, exports) in [
                (format!("P{i}"), vec![format!("p{i}")], 10, vec![x]),
                (format!("Q{i}"), vec![format!("q{i}")], 5, vec![a.clone()]),
                (format!("R{i}"), vec![], 3, vec![b.clone()]),
                (format!("p{i}"), vec![], 0, vec![a]),
                (format!("q{i}"), vec![], 0, vec![b]),
            ] {
                registrations.push(registration(id, deps, priority, &exports));
            }
            expected.extend([format!("P{i}"), format!("R{i}"), format!("p{i}")]);
        }
        let work = minimum_work(&base, &registrations);
        let mut composer = Composer::new(base.clone()).unwrap();
        composer.set_selection_limits(SelectionLimits { max_work: work });
        assert_eq!(
            composer.register(registrations).unwrap().active(),
            &expected
        );
        if let Some(previous) = previous {
            assert_eq!(work, 2 * previous);
        }
        previous = Some(work);
    }
}

#[test]
fn chain_diamond_and_dense_blockers_fit_edge_based_solver_allowances() {
    let (base, _) = base();
    let count = 128;
    let chain: Vec<_> = (0..count)
        .map(|i| {
            registration(
                format!("r{i:04}"),
                if i == 0 {
                    vec![]
                } else {
                    vec![format!("r{:04}", i - 1)]
                },
                i as i64,
                &[],
            )
        })
        .collect();
    assert!(minimum_work(&base, &chain) <= 20 * count);
    let diamond: Vec<_> = (0..count)
        .map(|i| {
            registration(
                format!("r{i:04}"),
                if i == 0 {
                    vec![]
                } else if i == count - 1 {
                    (1..count - 1).map(|j| format!("r{j:04}")).collect()
                } else {
                    vec!["r0000".into()]
                },
                i as i64,
                &[],
            )
        })
        .collect();
    assert!(minimum_work(&base, &diamond) <= 24 * count);
    let mut previous = None;
    for count in [16, 32, 64] {
        let dense: Vec<_> = (0..count)
            .map(|i| registration(format!("r{i:04}"), vec![], i as i64, &["capture".into()]))
            .collect();
        let work = minimum_work(&base, &dense);
        let edges = count * (count - 1) / 2;
        assert!(work <= 12 * count + 5 * edges, "count={count}, work={work}");
        if let Some(previous) = previous {
            assert!(work > 2 * previous && work <= 5 * previous);
        }
        previous = Some(work);
    }
}

#[test]
fn feedback_components_share_one_allowance_for_the_complete_uniqueness_proof() {
    let (base, _) = base();
    let feedback = |groups| {
        let mut registrations = Vec::new();
        for i in 0..groups {
            let (ab, ac, pb, qb, pc) = (
                format!("ab{i}"),
                format!("ac{i}"),
                format!("pb{i}"),
                format!("qb{i}"),
                format!("pc{i}"),
            );
            for (id, dependencies, priority, exports) in [
                (format!("a{i}"), vec![], 1, vec![ab.clone(), ac.clone()]),
                (format!("b{i}"), vec![], 3, vec![ab, pb.clone(), qb.clone()]),
                (format!("c{i}"), vec![], 0, vec![ac, pc.clone()]),
                (format!("P{i}"), vec![format!("a{i}")], 5, vec![pb, pc]),
                (format!("Q{i}"), vec![format!("c{i}")], 6, vec![qb]),
            ] {
                registrations.push(registration(id, dependencies, priority, &exports));
            }
        }
        registrations
    };
    let one = minimum_work(&base, &feedback(1));
    let registrations = feedback(4);
    let total = minimum_work(&base, &registrations);
    assert!(total > one);
    let mut composer = Composer::new(base).unwrap();
    composer.set_selection_limits(SelectionLimits { max_work: one });
    assert_eq!(
        composer.register(registrations.clone()).unwrap_err(),
        ComposeError::SelectionBudgetExceeded { limit: one }
    );
    assert!(composer.current().active().is_empty());
    composer.set_selection_limits(SelectionLimits { max_work: total });
    let expected: BTreeSet<_> = (0..4)
        .flat_map(|i| [format!("a{i}"), format!("P{i}")])
        .collect();
    assert_eq!(
        composer.register(registrations).unwrap().active(),
        &expected
    );
}
