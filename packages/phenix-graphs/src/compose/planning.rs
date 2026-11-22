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

/// Exact former lexical sweep order, with direct parents only. Transitive
/// closure is computed lazily when a splice or an overlapping edit needs it.
/// Disjoint registries never pay for materializing a quadratic ancestor matrix.
struct DependencyOrder {
    order: Vec<String>,
    parents: Vec<Vec<usize>>,
}

impl DependencyOrder {
    fn ancestor_mask(&self, index: usize) -> Vec<bool> {
        let mut seen = vec![false; self.order.len()];
        let mut pending = self.parents[index].clone();
        while let Some(parent) = pending.pop() {
            if !seen[parent] {
                seen[parent] = true;
                pending.extend(self.parents[parent].iter().copied());
            }
        }
        seen
    }
}

/// Preserve the former lexical *sweep* order, not ordinary Kahn order:
/// descendants whose names sort before their final prerequisite wait until
/// the next sweep; later names can become ready in the current sweep.
fn dependency_order(
    registrations: &BTreeMap<String, Registration>,
) -> Result<DependencyOrder, ComposeError> {
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

    let mut order = Vec::with_capacity(registrations.len());
    while order.len() < registrations.len() {
        if ready.is_empty() {
            return Err(ComposeError::DependencyCycle);
        }
        let mut next_sweep = BTreeSet::new();
        while let Some(id) = ready.pop_first() {
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
    let positions: BTreeMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect();
    let parents = order
        .iter()
        .map(|id| {
            registrations[id]
                .dependencies
                .iter()
                .map(|dependency| positions[dependency.as_str()])
                .collect()
        })
        .collect();
    Ok(DependencyOrder { order, parents })
}

pub(super) fn plan(
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
) -> Result<SelectedPlan, ComposeError> {
    let dependencies = dependency_order(registrations)?;
    let order = &dependencies.order;
    let mut ancestor_cache: Vec<Option<Vec<bool>>> = vec![None; order.len()];
    let mut footprints = BTreeMap::new();
    for (index, id) in order.iter().enumerate() {
        for edit in registrations[id].patch.edits() {
            edit.validate_payload(&registrations[id].origin)?;
        }
        let mut writes = registrations[id].patch.footprints();
        let mut tails = BTreeMap::new();
        if registrations[id]
            .patch
            .edits()
            .iter()
            .any(|edit| matches!(edit, Edit::Splice { .. }))
        {
            let ancestors =
                ancestor_cache[index].get_or_insert_with(|| dependencies.ancestor_mask(index));
            for (prior, predecessor) in order.iter().enumerate().take(index) {
                if ancestors[prior] {
                    for edit in registrations[predecessor].patch.edits() {
                        if let Edit::Splice { anchor, output, .. } = edit {
                            tails.insert(anchor.clone(), output.clone());
                        }
                    }
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
    let collisions = ConflictIndex::new(registrations, order, &footprints);
    for i in 0..order.len() {
        for j in collisions.after(i, registrations, order, &footprints) {
            // The topological replay order forbids j being an ancestor of i.
            // Cache closures only for registrations with a potential collision.
            if ancestor_cache[j].get_or_insert_with(|| dependencies.ancestor_mask(j))[i] {
                continue;
            }
            let a = &order[i];
            let b = &order[j];
            let left = &footprints[a];
            let right = &footprints[b];
            // Check all anchors, not just the first colliding footprint: the
            // patches may also collide on an added component declaration.
            for edit in registrations[a].patch.edits() {
                if let Edit::Splice { anchor, .. } = edit
                    && registrations[b].patch.edits().iter().any(
                        |other| matches!(other, Edit::Splice { anchor: other, .. } if other == anchor),
                    )
                {
                    return Err(ComposeError::Conflict {
                        first: a.clone(),
                        second: b.clone(),
                        footprint: Box::new(Footprint::Connection(anchor.clone())),
                    });
                }
            }
            // New nested exports may depend on a declaration introduced by
            // another plugin. Reject undeclared dependencies regardless of
            // lexical replay order, and reject an independent ancestor revision
            // that would overwrite the exported port.
            if let Some(path) = nested_export_dependency(&registrations[a], &registrations[b])
                .or_else(|| nested_export_dependency(&registrations[b], &registrations[a]))
            {
                return Err(ComposeError::Conflict {
                    first: a.clone(),
                    second: b.clone(),
                    footprint: Box::new(Footprint::Component(path)),
                });
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
    let selected = selection::select(registrations, order, &blockers, selection_limits)?;
    let active: BTreeSet<String> = order
        .iter()
        .zip(selected)
        .filter(|(_, selected)| *selected)
        .map(|(id, _)| id.clone())
        .collect();
    Ok(SelectedPlan {
        order: dependencies.order,
        active,
        footprints,
    })
}

/// The registration index stores only exact and path-prefix postings, never
/// the O(n^2) Cartesian set of conflicting registration pairs. Query one
/// earlier registration at a time, in lexical replay order, to keep the old
/// first-error precedence and reject dense equal-priority collisions early.
#[derive(Default)]
struct ConflictIndex {
    exact: BTreeMap<Footprint, BTreeSet<usize>>,
    component_exact: BTreeMap<String, BTreeSet<usize>>,
    component_descendants: BTreeMap<String, BTreeSet<usize>>,
    connection_descendants: BTreeMap<String, BTreeSet<usize>>,
    nested_export_references: BTreeMap<String, BTreeSet<usize>>,
}

fn path_ancestors(path: &str) -> impl Iterator<Item = &str> {
    std::iter::once(path).chain(path.match_indices('/').map(|(at, _)| &path[..at]))
}

impl ConflictIndex {
    fn new(
        registrations: &BTreeMap<String, Registration>,
        order: &[String],
        footprints: &BTreeMap<String, BTreeSet<Footprint>>,
    ) -> Self {
        fn index<K: Ord>(map: &mut BTreeMap<K, BTreeSet<usize>>, key: K, owner: usize) {
            map.entry(key).or_default().insert(owner);
        }

        let mut result = Self::default();
        for (owner, id) in order.iter().enumerate() {
            for write in &footprints[id] {
                index(&mut result.exact, write.clone(), owner);
                match write {
                    Footprint::Component(path) => {
                        index(&mut result.component_exact, path.clone(), owner);
                        for prefix in path_ancestors(path) {
                            index(
                                &mut result.component_descendants,
                                prefix.to_owned(),
                                owner,
                            );
                        }
                    }
                    Footprint::Connection(edge) => {
                        for endpoint in [&edge.from.process, &edge.to.process] {
                            for prefix in path_ancestors(endpoint) {
                                if prefix != endpoint || endpoint.contains('/') {
                                    index(
                                        &mut result.connection_descendants,
                                        prefix.to_owned(),
                                        owner,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            for edit in registrations[id].patch.edits() {
                match edit {
                    Edit::ExposeNestedInput { parent, targets, .. } => {
                        for prefix in path_ancestors(parent) {
                            index(&mut result.nested_export_references, prefix.to_owned(), owner);
                        }
                        for target in targets {
                            index(
                                &mut result.nested_export_references,
                                target.process.clone(),
                                owner,
                            );
                        }
                    }
                    Edit::ExposeNestedOutput { parent, from, .. } => {
                        for prefix in path_ancestors(parent) {
                            index(&mut result.nested_export_references, prefix.to_owned(), owner);
                        }
                        index(&mut result.nested_export_references, from.process.clone(), owner);
                    }
                    _ => {}
                }
            }
        }
        result
    }

    fn after(
        &self,
        i: usize,
        registrations: &BTreeMap<String, Registration>,
        order: &[String],
        footprints: &BTreeMap<String, BTreeSet<Footprint>>,
    ) -> BTreeSet<usize> {
        fn from_path(
            destinations: &mut BTreeSet<usize>,
            map: &BTreeMap<String, BTreeSet<usize>>,
            path: &str,
            after: usize,
        ) {
            if let Some(owners) = map.get(path) {
                destinations.extend(owners.iter().copied().filter(|owner| *owner > after));
            }
        }
        let id = &order[i];
        let mut candidates = BTreeSet::new();
        for write in &footprints[id] {
            if let Some(owners) = self.exact.get(write) {
                candidates.extend(owners.iter().copied().filter(|owner| *owner > i));
            }
            match write {
                Footprint::Component(path) => {
                    for prefix in path_ancestors(path) {
                        from_path(&mut candidates, &self.component_exact, prefix, i);
                    }
                    for map in [
                        &self.component_descendants,
                        &self.connection_descendants,
                        &self.nested_export_references,
                    ] {
                        from_path(&mut candidates, map, path, i);
                    }
                }
                Footprint::Connection(edge) => {
                    for endpoint in [&edge.from.process, &edge.to.process] {
                        for prefix in path_ancestors(endpoint) {
                            if prefix != endpoint || endpoint.contains('/') {
                                from_path(&mut candidates, &self.component_exact, prefix, i);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // The reverse direction matters when the earlier registration is an
        // export and a later registration declares a referenced component.
        for edit in registrations[id].patch.edits() {
            match edit {
                Edit::ExposeNestedInput { parent, targets, .. } => {
                    for prefix in path_ancestors(parent) {
                        from_path(&mut candidates, &self.component_exact, prefix, i);
                    }
                    for target in targets {
                        from_path(&mut candidates, &self.component_exact, &target.process, i);
                    }
                }
                Edit::ExposeNestedOutput { parent, from, .. } => {
                    for prefix in path_ancestors(parent) {
                        from_path(&mut candidates, &self.component_exact, prefix, i);
                    }
                    from_path(&mut candidates, &self.component_exact, &from.process, i);
                }
                _ => {}
            }
        }
        candidates
    }
}

/// Tests can materialize the complete candidate matrix to compare against the
/// previous exhaustive predicates; the production planner never does so.
#[cfg(test)]
fn conflict_candidates(
    registrations: &BTreeMap<String, Registration>,
    order: &[String],
    footprints: &BTreeMap<String, BTreeSet<Footprint>>,
) -> BTreeSet<(usize, usize)> {
    let index = ConflictIndex::new(registrations, order, footprints);
    (0..order.len())
        .flat_map(|i| {
            index
                .after(i, registrations, order, footprints)
                .into_iter()
                .map(move |j| (i, j))
        })
        .collect()
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
                (Footprint::Component(path), Footprint::Connection(edge))
                | (Footprint::Connection(edge), Footprint::Component(path)) => {
                    // Mutating an ancestor replaces its entire nested body.
                    // Connecting to a new nested declaration also requires
                    // dependency ordering, never incidental replay ordering.
                    [&edge.from.process, &edge.to.process]
                        .into_iter()
                        .any(|endpoint| {
                            endpoint
                                .strip_prefix(path)
                                .is_some_and(|suffix| suffix.starts_with('/'))
                                || (path.contains('/') && endpoint == path)
                        })
                }
                _ => false,
            }
        })
    })
}

/// An export may refer to a component written by another registration only
/// when that registration is an explicit dependency. Parent replacements also
/// conflict with any independently published nested export.
fn nested_export_dependency(declaration: &Registration, export: &Registration) -> Option<String> {
    for edit in declaration.patch.edits() {
        let path = match edit {
            Edit::Add { path, .. } | Edit::Replace { path, .. } => path,
            _ => continue,
        };
        for edit in export.patch.edits() {
            match edit {
                Edit::ExposeNestedInput {
                    parent, targets, ..
                } => {
                    if nested_under(parent, path)
                        || targets.iter().any(|target| target.process == *path)
                    {
                        return Some(path.clone());
                    }
                }
                Edit::ExposeNestedOutput { parent, from, .. }
                    if nested_under(parent, path) || from.process == *path =>
                {
                    return Some(path.clone());
                }
                _ => {}
            }
        }
    }
    None
}

fn nested_under(parent: &str, path: &str) -> bool {
    parent
        .strip_prefix(path)
        .is_some_and(|suffix| suffix.starts_with('/') || suffix.is_empty())
}

#[cfg(test)]
mod dependency_order_tests {
    use super::*;
    use crate::GraphOrigin;
    use crate::compose::NetworkPatch;

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
        let result = dependency_order(&registrations).unwrap();
        assert_eq!(result.order, ["b", "c", "a", "d"]);
        let d = result.order.iter().position(|id| id == "d").unwrap();
        let ancestors = result.ancestor_mask(d);
        assert_eq!(
            result
                .order
                .iter()
                .zip(ancestors)
                .filter_map(|(name, found)| { found.then_some(name.clone()) })
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["a".into(), "b".into()])
        );
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
                            ranks[*other] < ranks[index] && (index * 11 + *other * 7 + seed) % 5 < 2
                        })
                        .map(|other| names[other].to_owned())
                        .collect();
                    (
                        names[index].to_owned(),
                        registration(names[index], dependencies),
                    )
                })
                .collect();
            let actual = dependency_order(&registrations).unwrap();
            let (expected_order, expected_ancestors) = reference_order(&registrations);
            assert_eq!(actual.order, expected_order, "seed {seed}");
            for (index, name) in actual.order.iter().enumerate() {
                let found = actual
                    .order
                    .iter()
                    .zip(actual.ancestor_mask(index))
                    .filter_map(|(ancestor, included)| included.then_some(ancestor.clone()))
                    .collect::<BTreeSet<_>>();
                assert_eq!(found, expected_ancestors[name], "seed {seed}, {name}");
            }
        }
    }

    #[test]
    fn missing_dependencies_and_cycles_are_rejected() {
        let missing = BTreeMap::from([("a".into(), registration("a", vec!["missing".into()]))]);
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

#[cfg(test)]
mod conflict_index_tests {
    use super::*;
    use crate::GraphOrigin;
    use crate::component::ComponentDef;
    use crate::compose::NetworkPatch;
    use crate::network::{ConnectionDef, PortPath};
    use std::{num::NonZeroUsize, sync::Arc};

    fn port(process: &str) -> PortPath {
        PortPath {
            process: process.into(),
            port: "value".into(),
        }
    }

    fn edge(from: &str, to: &str) -> ConnectionKey {
        ConnectionKey::new(port(from), port(to))
    }

    fn registration(id: &str, edits: Vec<Edit>) -> Registration {
        let patch = NetworkPatch::new(edits);
        Registration::new(id, [], 0, GraphOrigin::default(), patch.footprints(), patch)
    }

    /// Brute-force oracle matching the previous planner's three conflict
    /// predicates, including splice precedence and directional export checks.
    fn reference_pairs(
        registrations: &BTreeMap<String, Registration>,
        order: &[String],
        footprints: &BTreeMap<String, BTreeSet<Footprint>>,
    ) -> BTreeSet<(usize, usize)> {
        let mut found = BTreeSet::new();
        for (i, a) in order.iter().enumerate() {
            for (j, b) in order.iter().enumerate().skip(i + 1) {
                let splices = registrations[a].patch.edits().iter().any(|edit| {
                    if let Edit::Splice { anchor, .. } = edit {
                        registrations[b].patch.edits().iter().any(|other| {
                            matches!(other, Edit::Splice { anchor: other, .. } if anchor == other)
                        })
                    } else {
                        false
                    }
                });
                if splices
                    || nested_export_dependency(&registrations[a], &registrations[b]).is_some()
                    || nested_export_dependency(&registrations[b], &registrations[a]).is_some()
                    || overlapping_write(&footprints[a], &footprints[b]).is_some()
                {
                    found.insert((i, j));
                }
            }
        }
        found
    }

    #[test]
    fn path_index_matches_legacy_exhaustive_collision_predicates() {
        let paths = [
            "m",
            "m/inner",
            "m/inner/a",
            "m/inner/b",
            "m/side",
            "x",
            "x/y",
            "x2",
            "z",
        ];
        let one = NonZeroUsize::new(1).unwrap();
        let definition = Arc::new(ComponentDef::map(|n: i64| n));
        for seed in 0..256_usize {
            let mut registrations = BTreeMap::new();
            for index in 0..12_usize {
                let name = format!("r{index:02}");
                let mut edits = Vec::new();
                for edit_index in 0..2 {
                    let k = seed * 17 + index * 13 + edit_index * 7;
                    let path = paths[(k / 3) % paths.len()];
                    let other = paths[(k / 5 + 2) % paths.len()];
                    let edit = match k % 9 {
                        0 => Edit::Add {
                            path: path.into(),
                            definition: definition.clone(),
                        },
                        1 => Edit::Replace {
                            path: path.into(),
                            definition: definition.clone(),
                        },
                        2 => Edit::Connect(ConnectionDef {
                            from: port(path),
                            to: port(other),
                            capacity: one,
                        }),
                        3 => Edit::Disconnect(edge(path, other)),
                        4 => Edit::ExposeNestedOutput {
                            parent: "m/inner".into(),
                            name: format!("out{edit_index}"),
                            from: port(path),
                        },
                        5 => Edit::ExposeNestedInput {
                            parent: "m/inner".into(),
                            name: format!("in{edit_index}"),
                            targets: vec![port(path), port(other)],
                            capacity: one,
                        },
                        6 => Edit::Splice {
                            anchor: edge(path, other),
                            input: port("splice"),
                            output: port("splice"),
                        },
                        7 => Edit::AddSource(path.into()),
                        _ => Edit::ExposeOutput {
                            name: path.into(),
                            from: port(other),
                        },
                    };
                    edits.push(edit);
                }
                registrations.insert(name.clone(), registration(&name, edits));
            }
            let order: Vec<_> = registrations.keys().cloned().collect();
            let footprints = order
                .iter()
                .map(|id| (id.clone(), registrations[id].patch.footprints()))
                .collect();
            assert_eq!(
                conflict_candidates(&registrations, &order, &footprints),
                reference_pairs(&registrations, &order, &footprints),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn disjoint_and_sibling_writes_need_no_pairwise_planning() {
        let mut registrations = BTreeMap::new();
        let definition = Arc::new(ComponentDef::map(|n: i64| n));
        for i in 0..512 {
            let id = format!("r{i:04}");
            registrations.insert(
                id.clone(),
                registration(
                    &id,
                    vec![Edit::Add {
                        path: format!("m/sibling{i:04}"),
                        definition: definition.clone(),
                    }],
                ),
            );
        }
        let order = registrations.keys().cloned().collect::<Vec<_>>();
        let footprints = order
            .iter()
            .map(|id| (id.clone(), registrations[id].patch.footprints()))
            .collect();
        assert!(conflict_candidates(&registrations, &order, &footprints).is_empty());
    }

    #[test]
    fn root_and_nested_footprints_never_alias_and_nested_endpoint_equality_counts() {
        let registrations: BTreeMap<String, Registration> = BTreeMap::from([
            (
                "a".into(),
                registration(
                    "a",
                    vec![Edit::ExposeOutput {
                        name: "m/inner/telemetry".into(),
                        from: port("m"),
                    }],
                ),
            ),
            (
                "b".into(),
                registration(
                    "b",
                    vec![Edit::ExposeNestedOutput {
                        parent: "m/inner".into(),
                        name: "telemetry".into(),
                        from: port("m/inner/leaf"),
                    }],
                ),
            ),
        ]);
        let order = registrations.keys().cloned().collect::<Vec<_>>();
        let footprints = order
            .iter()
            .map(|id| (id.clone(), registrations[id].patch.footprints()))
            .collect();
        assert!(conflict_candidates(&registrations, &order, &footprints).is_empty());

        let registrations: BTreeMap<String, Registration> = BTreeMap::from([
            (
                "a".into(),
                registration(
                    "a",
                    vec![Edit::Add {
                        path: "m/inner".into(),
                        definition: Arc::new(ComponentDef::map(|n: i64| n)),
                    }],
                ),
            ),
            (
                "b".into(),
                registration(
                    "b",
                    vec![Edit::Connect(ConnectionDef {
                        from: port("m/inner"),
                        to: port("m/outer"),
                        capacity: NonZeroUsize::new(1).unwrap(),
                    })],
                ),
            ),
        ]);
        let order = registrations.keys().cloned().collect::<Vec<_>>();
        let footprints = order
            .iter()
            .map(|id| (id.clone(), registrations[id].patch.footprints()))
            .collect();
        assert_eq!(
            conflict_candidates(&registrations, &order, &footprints),
            BTreeSet::from([(0, 1)])
        );
    }

    #[test]
    fn dense_equal_priority_collision_rejects_first_pair_without_materializing_matrix() {
        let mut registrations = BTreeMap::new();
        let definition = Arc::new(ComponentDef::map(|n: i64| n));
        for i in 0..256 {
            let id = format!("r{i:04}");
            registrations.insert(
                id.clone(),
                registration(
                    &id,
                    vec![Edit::Add {
                        path: "shared".into(),
                        definition: definition.clone(),
                    }],
                ),
            );
        }
        let order = registrations.keys().cloned().collect::<Vec<_>>();
        let footprints = order
            .iter()
            .map(|id| (id.clone(), registrations[id].patch.footprints()))
            .collect();
        let index = ConflictIndex::new(&registrations, &order, &footprints);
        assert_eq!(index.after(0, &registrations, &order, &footprints).len(), 255);
        assert!(matches!(
            plan(&registrations, SelectionLimits::default()),
            Err(ComposeError::Conflict { first, second, .. })
                if first == "r0000" && second == "r0001"
        ));
    }
}
