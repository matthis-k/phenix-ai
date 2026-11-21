//! Validate the entire candidate registry before selecting an ephemeral plan.

use super::{
    ComposeError, ConnectionKey, Edit, Footprint, Registration, SelectionLimits, selection,
};
use std::collections::{BTreeMap, BTreeSet};

/// Per-attempt data only; dependency replay order is not solver influence order.
pub(super) struct SelectedPlan {
    pub(super) order: Vec<String>,
    pub(super) active: BTreeSet<String>,
    pub(super) footprints: BTreeMap<String, BTreeSet<Footprint>>,
}

/// Preserve the former lexical *sweep* order, not ordinary Kahn order:
/// descendants whose names sort before their final prerequisite wait until
/// the next sweep; later names can become ready in the current sweep.
fn dependency_order(
    registrations: &BTreeMap<String, Registration>,
) -> Result<(Vec<String>, BTreeMap<String, BTreeSet<String>>), ComposeError> {
    let mut remaining = BTreeMap::new();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for (id, registration) in registrations {
        for dependency in &registration.dependencies {
            if !registrations.contains_key(dependency) {
                return Err(ComposeError::MissingDependency {
                    registration: id.clone(),
                    dependency: dependency.clone(),
                });
            }
            dependents
                .entry(dependency.clone())
                .or_default()
                .push(id.clone());
        }
        remaining.insert(id.clone(), registration.dependencies.len());
        if registration.dependencies.is_empty() {
            ready.insert(id.clone());
        }
    }

    let mut ancestors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut order = Vec::with_capacity(registrations.len());
    while order.len() < registrations.len() {
        if ready.is_empty() {
            return Err(ComposeError::DependencyCycle);
        }
        let mut next_sweep = BTreeSet::new();
        while let Some(id) = ready.pop_first() {
            let registration = &registrations[&id];
            let mut all = registration.dependencies.clone();
            for dependency in &registration.dependencies {
                all.extend(ancestors[dependency].iter().cloned());
            }
            ancestors.insert(id.clone(), all);
            order.push(id.clone());
            if let Some(children) = dependents.get(&id) {
                for child in children {
                    let count = remaining.get_mut(child).expect("indexed dependent");
                    *count -= 1;
                    if *count == 0 {
                        if child > &id {
                            ready.insert(child.clone());
                        } else {
                            next_sweep.insert(child.clone());
                        }
                    }
                }
            }
        }
        ready = next_sweep;
    }
    Ok((order, ancestors))
}

pub(super) fn plan(
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
) -> Result<SelectedPlan, ComposeError> {
    let (order, ancestors) = dependency_order(registrations)?;
    let mut footprints = BTreeMap::new();
    for id in &order {
        for edit in registrations[id].patch.edits() {
            edit.validate_payload(&registrations[id].origin)?;
        }
        let mut writes = registrations[id].patch.footprints();
        let mut tails = BTreeMap::new();
        for predecessor in order.iter().filter(|other| ancestors[id].contains(*other)) {
            for edit in registrations[predecessor].patch.edits() {
                if let Edit::Splice { anchor, output, .. } = edit {
                    tails.insert(anchor.clone(), output.clone());
                }
            }
        }
        for edit in registrations[id].patch.edits() {
            if let Edit::Splice {
                anchor,
                input,
                output,
            } = edit
            {
                let from = tails.get(anchor).unwrap_or(&anchor.from).clone();
                writes.insert(Footprint::Connection(ConnectionKey::new(
                    from.clone(),
                    anchor.to.clone(),
                )));
                writes.insert(Footprint::Connection(ConnectionKey::new(
                    from,
                    input.clone(),
                )));
                tails.insert(anchor.clone(), output.clone());
            }
        }
        for footprint in &writes {
            if !registrations[id].grants.contains(footprint) {
                return Err(ComposeError::Unauthorized {
                    registration: id.clone(),
                    footprint: footprint.clone(),
                });
            }
        }
        footprints.insert(id.clone(), writes);
    }
    // Fail closed on ambiguities, even when a third registration could defeat
    // one participant. Dependency ordering is the only splice ordering key.
    let mut blockers = Vec::new();
    for (i, a) in order.iter().enumerate() {
        for (j, b) in order.iter().enumerate().skip(i + 1) {
            if ancestors[b].contains(a) || ancestors[a].contains(b) {
                continue;
            }
            let left = &footprints[a];
            let right = &footprints[b];
            // Check all anchors, not just the first colliding footprint: the
            // patches may also collide on an added component declaration.
            for edit in registrations[a].patch.edits() {
                if let Edit::Splice { anchor, .. } = edit
                    && registrations[b].patch.edits().iter().any(|other|
                        matches!(other, Edit::Splice { anchor: other, .. } if other == anchor)) {
                    return Err(ComposeError::Conflict {
                        first: a.clone(), second: b.clone(),
                        footprint: Box::new(Footprint::Connection(anchor.clone())),
                    });
                }
            }
            if let Some(footprint) = overlapping_write(left, right) {
                match registrations[a].priority.cmp(&registrations[b].priority) {
                    std::cmp::Ordering::Equal => {
                        return Err(ComposeError::Conflict {
                            first: a.clone(),
                            second: b.clone(),
                            footprint: Box::new(footprint.clone()),
                        });
                    }
                    std::cmp::Ordering::Greater => blockers.push((i, j)),
                    std::cmp::Ordering::Less => blockers.push((j, i)),
                }
            }
        }
    }
    let selected = selection::select(registrations, &order, &blockers, selection_limits)?;
    let active: BTreeSet<String> = order
        .iter()
        .zip(selected)
        .filter(|(_, selected)| *selected)
        .map(|(id, _)| id.clone())
        .collect();
    Ok(SelectedPlan {
        order,
        active,
        footprints,
    })
}

/// Exact grants remain separate. Selection conflict is broader: replacing an
/// ancestor composite changes the definition containing every nested child.
/// Independently selected ancestor and descendant replacements would otherwise
/// silently erase each other depending on lexical replay order.
fn overlapping_write<'a>(
    left: &'a BTreeSet<Footprint>,
    right: &BTreeSet<Footprint>,
) -> Option<&'a Footprint> {
    left.iter().find(|write| {
        right.iter().any(|other| {
            if *write == other {
                return true;
            }
            match (write, other) {
                (Footprint::Component(a), Footprint::Component(b)) => {
                    b.strip_prefix(a)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                        || a.strip_prefix(b)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                }
                _ => false,
            }
        })
    })
}

#[cfg(test)]
mod dependency_order_tests {
    use super::*;
    use crate::compose::NetworkPatch;
    use crate::GraphOrigin;

    fn registration(id: &str, dependencies: Vec<String>) -> Registration {
        Registration::new(
            id,
            dependencies,
            0,
            GraphOrigin::default(),
            Vec::<Footprint>::new(),
            NetworkPatch::new(Vec::<Edit>::new()),
        )
    }

    /// The old planner swept the BTreeMap once per pass; a newly ready
    /// earlier key could not preempt an already ready later key.
    fn reference_order(
        registrations: &BTreeMap<String, Registration>,
    ) -> (Vec<String>, BTreeMap<String, BTreeSet<String>>) {
        let mut ancestors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut order = Vec::new();
        while order.len() < registrations.len() {
            let before = order.len();
            for (id, registration) in registrations {
                if ancestors.contains_key(id)
                    || !registration
                        .dependencies
                        .iter()
                        .all(|dependency| ancestors.contains_key(dependency))
                {
                    continue;
                }
                let mut all = registration.dependencies.clone();
                for dependency in &registration.dependencies {
                    all.extend(ancestors[dependency].iter().cloned());
                }
                ancestors.insert(id.clone(), all);
                order.push(id.clone());
            }
            assert!(order.len() > before, "reference input must be acyclic");
        }
        (order, ancestors)
    }

    #[test]
    fn reversed_dependencies_preserve_multi_sweep_replay_order() {
        let registrations = BTreeMap::from([
            ("a".into(), registration("a", vec!["b".into()])),
            ("b".into(), registration("b", Vec::new())),
            ("c".into(), registration("c", Vec::new())),
            ("d".into(), registration("d", vec!["a".into()])),
        ]);
        let (order, ancestors) = dependency_order(&registrations).unwrap();
        assert_eq!(order, ["b", "c", "a", "d"]);
        assert_eq!(ancestors["d"], BTreeSet::from(["a".into(), "b".into()]));
    }

    #[test]
    fn generated_acyclic_dependency_graphs_match_legacy_sweep_and_ancestors() {
        let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
        for seed in 0..128 {
            let ranks = (0..8)
                .map(|index| (index * 5 + seed) % 8)
                .collect::<Vec<_>>();
            let registrations = (0..8)
                .map(|index| {
                    let dependencies = (0..8)
                        .filter(|other| {
                            ranks[*other] < ranks[index]
                                && (index * 11 + *other * 7 + seed) % 5 < 2
                        })
                        .map(|other| names[other].to_owned())
                        .collect();
                    (names[index].to_owned(), registration(names[index], dependencies))
                })
                .collect();
            assert_eq!(
                dependency_order(&registrations).unwrap(),
                reference_order(&registrations),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn missing_dependencies_and_cycles_are_rejected() {
        let missing = BTreeMap::from([(
            "a".into(),
            registration("a", vec!["missing".into()]),
        )]);
        assert!(matches!(
            dependency_order(&missing),
            Err(ComposeError::MissingDependency { .. })
        ));
        let cycle = BTreeMap::from([
            ("a".into(), registration("a", vec!["b".into()])),
            ("b".into(), registration("b", vec!["a".into()])),
            ("c".into(), registration("c", Vec::new())),
        ]);
        assert!(matches!(
            dependency_order(&cycle),
            Err(ComposeError::DependencyCycle)
        ));
    }
}
