//! Validate the entire candidate registry before selecting an ephemeral plan.

use super::{
    ComposeError, ConnectionKey, Edit, Footprint, PlanningBudget, Registration, SelectionLimits,
    selection,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

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
    fn ancestor_mask(
        &self,
        index: usize,
        budget: &mut PlanningBudget<'_>,
    ) -> Result<Vec<bool>, ComposeError> {
        budget.charge(self.order.len())?;
        #[cfg(test)]
        {
            budget.ancestor_cells += self.order.len();
        }
        let mut seen = vec![false; self.order.len()];
        budget.charge(self.parents[index].len())?;
        let mut pending = self.parents[index].clone();
        while let Some(parent) = pending.pop() {
            budget.charge(1)?;
            if !seen[parent] {
                seen[parent] = true;
                budget.charge(self.parents[parent].len())?;
                pending.extend(self.parents[parent].iter().copied());
            }
        }
        Ok(seen)
    }
}

/// Preserve the former lexical *sweep* order, not ordinary Kahn order:
/// descendants whose names sort before their final prerequisite wait until
/// the next sweep; later names can become ready in the current sweep.
fn dependency_order(
    registrations: &BTreeMap<String, Registration>,
    budget: &mut PlanningBudget<'_>,
) -> Result<DependencyOrder, ComposeError> {
    let mut remaining = BTreeMap::new();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for (id, registration) in registrations {
        budget.charge(1)?;
        for dependency in &registration.dependencies {
            budget.text(dependency)?;
            // Diagnostic/copy reservations precede the semantic decision.
            budget.text(id)?;
            budget.text(dependency)?;
            budget.charge(2)?;
            budget.text(dependency)?;
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
        budget.text(id)?;
        budget.text(id)?;
        budget.charge(1)?;
        remaining.insert(id.clone(), registration.dependencies.len());
        if registration.dependencies.is_empty() {
            budget.text(id)?;
            budget.text(id)?;
            budget.charge(1)?;
            ready.insert(id.clone());
        }
    }

    budget.charge(registrations.len())?;
    let mut order = Vec::with_capacity(registrations.len());
    while order.len() < registrations.len() {
        if ready.is_empty() {
            return Err(ComposeError::DependencyCycle);
        }
        let mut next_sweep = BTreeSet::new();
        while let Some(id) = ready.pop_first() {
            budget.text(&id)?;
            order.push(id.clone());
            budget.text(&id)?;
            if let Some(children) = dependents.get(&id) {
                for child in children {
                    budget.text(child)?;
                    let count = remaining.get_mut(child).expect("indexed dependent");
                    *count -= 1;
                    if *count == 0 {
                        budget.text(child)?;
                        budget.text(child)?;
                        budget.text(&id)?;
                        budget.text(child)?;
                        budget.charge(1)?;
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
    let mut positions = BTreeMap::new();
    for (index, id) in order.iter().enumerate() {
        budget.text(id)?;
        budget.charge(1)?;
        positions.insert(id.as_str(), index);
    }
    budget.charge(order.len())?;
    let mut parents = Vec::with_capacity(order.len());
    for id in &order {
        budget.text(id)?;
        let mut row = Vec::new();
        for dependency in &registrations[id].dependencies {
            budget.text(dependency)?;
            budget.charge(1)?;
            row.push(positions[dependency.as_str()]);
        }
        parents.push(row);
    }
    Ok(DependencyOrder { order, parents })
}

pub(super) fn plan(
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
    budget: &mut PlanningBudget<'_>,
) -> Result<SelectedPlan, ComposeError> {
    let dependencies = dependency_order(registrations, budget)?;
    let order = &dependencies.order;
    budget.charge(order.len())?;
    let mut ancestor_cache: Vec<Option<Vec<bool>>> = vec![None; order.len()];
    let mut footprints = BTreeMap::new();
    for (index, id) in order.iter().enumerate() {
        budget.text(id)?;
        let registration = &registrations[id];
        for edit in registration.patch.edits() {
            edit.validate_payload(&registration.origin, budget)?;
        }
        let mut writes = BTreeSet::new();
        let mut has_splice = false;
        for edit in registration.patch.edits() {
            // Reserve all possible footprint copies before the public helper
            // allocates them (Splice repeats its anchor's destination).
            budget.edit(edit)?;
            budget.edit(edit)?;
            if let Edit::Splice { anchor, .. } = edit {
                budget.port(&anchor.to)?;
                budget.port(&anchor.to)?;
            }
            budget.charge(4)?;
            writes.extend(edit.footprints());
            has_splice |= matches!(edit, Edit::Splice { .. });
        }
        let mut tails = BTreeMap::new();
        if has_splice && !dependencies.parents[index].is_empty() {
            if ancestor_cache[index].is_none() {
                ancestor_cache[index] = Some(dependencies.ancestor_mask(index, budget)?);
            }
            let ancestors = ancestor_cache[index].as_ref().expect("ancestor row");
            for (prior, predecessor) in order.iter().enumerate().take(index) {
                budget.charge(1)?;
                if ancestors[prior] {
                    budget.text(predecessor)?;
                    for edit in registrations[predecessor].patch.edits() {
                        budget.charge(1)?;
                        if let Edit::Splice { anchor, output, .. } = edit {
                            budget.edge(anchor)?;
                            budget.edge(anchor)?;
                            budget.port(output)?;
                            budget.charge(1)?;
                            tails.insert(anchor.clone(), output.clone());
                        }
                    }
                }
            }
        }
        for edit in registration.patch.edits() {
            budget.charge(1)?;
            if let Edit::Splice {
                anchor,
                input,
                output,
            } = edit
            {
                budget.edge(anchor)?;
                let from = tails.get(anchor).unwrap_or(&anchor.from);
                budget.port(from)?;
                budget.port(from)?;
                budget.port(&anchor.to)?;
                budget.port(input)?;
                budget.port(from)?;
                budget.port(from)?;
                budget.port(&anchor.to)?;
                budget.port(input)?;
                budget.edge(anchor)?;
                budget.edge(anchor)?;
                budget.port(output)?;
                budget.charge(3)?;
                let from = from.clone();
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
            budget.footprint(footprint)?;
            budget.footprint(footprint)?;
            budget.text(id)?;
            if !registration.grants.contains(footprint) {
                return Err(ComposeError::Unauthorized {
                    registration: id.clone(),
                    footprint: footprint.clone(),
                });
            }
        }
        budget.text(id)?;
        budget.text(id)?;
        budget.charge(1)?;
        footprints.insert(id.clone(), writes);
    }
    // Fail closed on ambiguities, even when a third registration could defeat
    // one participant. Dependency ordering is the only splice ordering key.
    let mut blockers = Vec::new();
    let collisions = ConflictIndex::new(registrations, order, &footprints, budget)?;
    for i in 0..order.len() {
        budget.charge(1)?;
        for j in collisions.after(i, registrations, order, &footprints, budget)? {
            budget.charge(1)?;
            // The topological replay order forbids j being an ancestor of i.
            // Cache closures only for registrations with a potential collision.
            if !dependencies.parents[j].is_empty() {
                if ancestor_cache[j].is_none() {
                    ancestor_cache[j] = Some(dependencies.ancestor_mask(j, budget)?);
                }
                if ancestor_cache[j].as_ref().expect("ancestor row")[i] {
                    continue;
                }
            }
            let a = &order[i];
            let b = &order[j];
            budget.text(a)?;
            budget.text(b)?;
            budget.charge(1)?;
            let left = &footprints[a];
            let right = &footprints[b];
            budget.text(a)?;
            budget.text(b)?;
            let ra = &registrations[a];
            let rb = &registrations[b];
            // Reserve ID diagnostic copies before any conflict decision.
            budget.text(a)?;
            budget.text(b)?;
            // Check all anchors, not just the first colliding footprint: the
            // patches may also collide on an added component declaration.
            for edit in ra.patch.edits() {
                budget.charge(1)?;
                if let Edit::Splice { anchor, .. } = edit {
                    for other in rb.patch.edits() {
                        budget.charge(1)?;
                        if let Edit::Splice { anchor: other, .. } = other {
                            budget.edge(anchor)?;
                            budget.edge(other)?;
                            budget.edge(anchor)?;
                            if other == anchor {
                                return Err(ComposeError::Conflict {
                                    first: a.clone(),
                                    second: b.clone(),
                                    footprint: Box::new(Footprint::Connection(anchor.clone())),
                                });
                            }
                        }
                    }
                }
            }
            // New nested exports may depend on a declaration introduced by
            // another plugin. Reject undeclared dependencies regardless of
            // lexical replay order, and reject an independent ancestor revision
            // that would overwrite the exported port.
            let mut nested = nested_export_dependency(ra, rb, budget)?;
            if nested.is_none() {
                nested = nested_export_dependency(rb, ra, budget)?;
            }
            if let Some(path) = nested {
                return Err(ComposeError::Conflict {
                    first: a.clone(),
                    second: b.clone(),
                    footprint: Box::new(Footprint::Component(path)),
                });
            }
            if let Some(footprint) = overlapping_write(left, right, budget)? {
                match ra.priority.cmp(&rb.priority) {
                    std::cmp::Ordering::Equal => {
                        return Err(ComposeError::Conflict {
                            first: a.clone(),
                            second: b.clone(),
                            footprint: Box::new(footprint.clone()),
                        });
                    }
                    std::cmp::Ordering::Greater => {
                        budget.charge(3)?;
                        #[cfg(test)]
                        {
                            budget.blockers += 1;
                        }
                        blockers.push((i, j));
                    }
                    std::cmp::Ordering::Less => {
                        budget.charge(3)?;
                        #[cfg(test)]
                        {
                            budget.blockers += 1;
                        }
                        blockers.push((j, i));
                    }
                }
            }
        }
    }
    let selected = selection::select(registrations, order, &blockers, selection_limits)?;
    let mut active = BTreeSet::new();
    for (id, selected) in order.iter().zip(selected) {
        budget.charge(1)?;
        if selected {
            budget.text(id)?;
            budget.text(id)?;
            budget.charge(1)?;
            active.insert(id.clone());
        }
    }
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
        budget: &mut PlanningBudget<'_>,
    ) -> Result<Self, ComposeError> {
        fn index<K: Ord>(map: &mut BTreeMap<K, BTreeSet<usize>>, key: K, owner: usize) {
            map.entry(key).or_default().insert(owner);
        }

        let mut result = Self::default();
        for (owner, id) in order.iter().enumerate() {
            budget.text(id)?;
            for write in &footprints[id] {
                budget.footprint(write)?;
                budget.footprint(write)?;
                budget.charge(2)?;
                index(&mut result.exact, write.clone(), owner);
                match write {
                    Footprint::Component(path) => {
                        budget.text(path)?;
                        budget.text(path)?;
                        budget.charge(2)?;
                        index(&mut result.component_exact, path.clone(), owner);
                        budget.text(path)?;
                        for prefix in path_ancestors(path) {
                            budget.text(prefix)?;
                            budget.text(prefix)?;
                            budget.charge(2)?;
                            index(&mut result.component_descendants, prefix.to_owned(), owner);
                        }
                    }
                    Footprint::Connection(edge) => {
                        for endpoint in [&edge.from.process, &edge.to.process] {
                            budget.text(endpoint)?;
                            for prefix in path_ancestors(endpoint) {
                                budget.text(prefix)?;
                                budget.text(endpoint)?;
                                budget.text(endpoint)?;
                                if prefix != endpoint || endpoint.contains('/') {
                                    budget.text(prefix)?;
                                    budget.text(prefix)?;
                                    budget.charge(2)?;
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
            budget.text(id)?;
            for edit in registrations[id].patch.edits() {
                budget.charge(1)?;
                match edit {
                    Edit::ExposeNestedInput {
                        parent, targets, ..
                    } => {
                        budget.text(parent)?;
                        for prefix in path_ancestors(parent) {
                            budget.text(prefix)?;
                            budget.text(prefix)?;
                            budget.charge(2)?;
                            index(
                                &mut result.nested_export_references,
                                prefix.to_owned(),
                                owner,
                            );
                        }
                        for target in targets {
                            budget.text(&target.process)?;
                            budget.text(&target.process)?;
                            budget.charge(2)?;
                            index(
                                &mut result.nested_export_references,
                                target.process.clone(),
                                owner,
                            );
                        }
                    }
                    Edit::ExposeNestedOutput { parent, from, .. } => {
                        budget.text(parent)?;
                        for prefix in path_ancestors(parent) {
                            budget.text(prefix)?;
                            budget.text(prefix)?;
                            budget.charge(2)?;
                            index(
                                &mut result.nested_export_references,
                                prefix.to_owned(),
                                owner,
                            );
                        }
                        budget.text(&from.process)?;
                        budget.text(&from.process)?;
                        budget.charge(2)?;
                        index(
                            &mut result.nested_export_references,
                            from.process.clone(),
                            owner,
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(result)
    }

    fn after(
        &self,
        i: usize,
        registrations: &BTreeMap<String, Registration>,
        order: &[String],
        footprints: &BTreeMap<String, BTreeSet<Footprint>>,
        budget: &mut PlanningBudget<'_>,
    ) -> Result<BTreeSet<usize>, ComposeError> {
        fn from_path(
            destinations: &mut BTreeSet<usize>,
            map: &BTreeMap<String, BTreeSet<usize>>,
            path: &str,
            after: usize,
            budget: &mut PlanningBudget<'_>,
        ) -> Result<(), ComposeError> {
            budget.text(path)?;
            if let Some(owners) = map.get(path) {
                for &owner in owners.range((Excluded(after), Unbounded)) {
                    budget.charge(2)?;
                    #[cfg(test)]
                    {
                        budget.posting_owners += 1;
                    }
                    destinations.insert(owner);
                }
            }
            Ok(())
        }
        let id = &order[i];
        let mut candidates = BTreeSet::new();
        budget.text(id)?;
        for write in &footprints[id] {
            budget.footprint(write)?;
            if let Some(owners) = self.exact.get(write) {
                for &owner in owners.range((Excluded(i), Unbounded)) {
                    budget.charge(2)?;
                    #[cfg(test)]
                    {
                        budget.posting_owners += 1;
                    }
                    candidates.insert(owner);
                }
            }
            match write {
                Footprint::Component(path) => {
                    budget.text(path)?;
                    for prefix in path_ancestors(path) {
                        from_path(&mut candidates, &self.component_exact, prefix, i, budget)?;
                    }
                    for map in [
                        &self.component_descendants,
                        &self.connection_descendants,
                        &self.nested_export_references,
                    ] {
                        from_path(&mut candidates, map, path, i, budget)?;
                    }
                }
                Footprint::Connection(edge) => {
                    for endpoint in [&edge.from.process, &edge.to.process] {
                        budget.text(endpoint)?;
                        for prefix in path_ancestors(endpoint) {
                            budget.text(prefix)?;
                            budget.text(endpoint)?;
                            budget.text(endpoint)?;
                            if prefix != endpoint || endpoint.contains('/') {
                                from_path(
                                    &mut candidates,
                                    &self.component_exact,
                                    prefix,
                                    i,
                                    budget,
                                )?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // The reverse direction matters when the earlier registration is an
        // export and a later registration declares a referenced component.
        budget.text(id)?;
        for edit in registrations[id].patch.edits() {
            budget.charge(1)?;
            match edit {
                Edit::ExposeNestedInput {
                    parent, targets, ..
                } => {
                    budget.text(parent)?;
                    for prefix in path_ancestors(parent) {
                        from_path(&mut candidates, &self.component_exact, prefix, i, budget)?;
                    }
                    for target in targets {
                        from_path(
                            &mut candidates,
                            &self.component_exact,
                            &target.process,
                            i,
                            budget,
                        )?;
                    }
                }
                Edit::ExposeNestedOutput { parent, from, .. } => {
                    budget.text(parent)?;
                    for prefix in path_ancestors(parent) {
                        from_path(&mut candidates, &self.component_exact, prefix, i, budget)?;
                    }
                    from_path(
                        &mut candidates,
                        &self.component_exact,
                        &from.process,
                        i,
                        budget,
                    )?;
                }
                _ => {}
            }
        }
        Ok(candidates)
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
    let mut used = 0;
    let mut budget = PlanningBudget::new(
        super::PlanningLimits {
            max_units: usize::MAX,
        },
        &mut used,
    )
    .unwrap();
    let index = ConflictIndex::new(registrations, order, footprints, &mut budget).unwrap();
    (0..order.len())
        .flat_map(|i| {
            index
                .after(i, registrations, order, footprints, &mut budget)
                .unwrap()
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
    budget: &mut PlanningBudget<'_>,
) -> Result<Option<&'a Footprint>, ComposeError> {
    // Only Component footprints participate in prefix/endpoint overlaps.
    // They sort first in Footprint's derived order. Without them the sorted
    // sets can be intersected directly, preserving the first left-side match.
    if left.is_empty() || right.is_empty() {
        return Ok(None);
    }
    budget.charge(2)?;
    if !matches!(left.first(), Some(Footprint::Component(_)))
        && !matches!(right.first(), Some(Footprint::Component(_)))
    {
        let mut right = right.iter().peekable();
        for write in left {
            while let Some(&other) = right.peek() {
                budget.footprint(write)?;
                budget.footprint(other)?;
                budget.footprint(write)?;
                match write.cmp(other) {
                    std::cmp::Ordering::Less => break,
                    std::cmp::Ordering::Equal => return Ok(Some(write)),
                    std::cmp::Ordering::Greater => {
                        right.next();
                    }
                }
            }
            if right.peek().is_none() {
                break;
            }
        }
        return Ok(None);
    }
    for write in left {
        for other in right {
            // Reserve operands and a possible diagnostic clone before deciding.
            budget.footprint(write)?;
            budget.footprint(other)?;
            budget.footprint(write)?;
            if write == other {
                return Ok(Some(write));
            }
            let overlaps = match (write, other) {
                (Footprint::Component(a), Footprint::Component(b)) => {
                    budget.text(a)?;
                    budget.text(b)?;
                    budget.text(a)?;
                    budget.text(b)?;
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
                    let mut found = false;
                    for endpoint in [&edge.from.process, &edge.to.process] {
                        budget.text(endpoint)?;
                        budget.text(path)?;
                        budget.text(path)?;
                        budget.text(endpoint)?;
                        budget.text(path)?;
                        if endpoint
                            .strip_prefix(path)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                            || (path.contains('/') && endpoint == path)
                        {
                            found = true;
                            break;
                        }
                    }
                    found
                }
                _ => false,
            };
            if overlaps {
                return Ok(Some(write));
            }
        }
    }
    Ok(None)
}

/// An export may refer to a component written by another registration only
/// when that registration is an explicit dependency. Parent replacements also
/// conflict with any independently published nested export.
fn nested_export_dependency(
    declaration: &Registration,
    export: &Registration,
    budget: &mut PlanningBudget<'_>,
) -> Result<Option<String>, ComposeError> {
    for edit in declaration.patch.edits() {
        budget.charge(1)?;
        let path = match edit {
            Edit::Add { path, .. } | Edit::Replace { path, .. } => path,
            _ => continue,
        };
        for edit in export.patch.edits() {
            budget.charge(1)?;
            match edit {
                Edit::ExposeNestedInput {
                    parent, targets, ..
                } => {
                    budget.text(path)?;
                    budget.text(parent)?;
                    budget.text(path)?;
                    let mut found = nested_under(parent, path);
                    if !found {
                        for target in targets {
                            budget.text(&target.process)?;
                            budget.text(path)?;
                            if target.process == *path {
                                found = true;
                                break;
                            }
                        }
                    }
                    if found {
                        return Ok(Some(path.clone()));
                    }
                }
                Edit::ExposeNestedOutput { parent, from, .. } => {
                    budget.text(path)?;
                    budget.text(parent)?;
                    budget.text(path)?;
                    budget.text(&from.process)?;
                    budget.text(path)?;
                    if nested_under(parent, path) || from.process == *path {
                        return Ok(Some(path.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(None)
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

    fn dependency_order(
        registrations: &BTreeMap<String, Registration>,
    ) -> Result<DependencyOrder, ComposeError> {
        super::dependency_order(
            registrations,
            &mut PlanningBudget::new(
                super::super::PlanningLimits {
                    max_units: usize::MAX,
                },
                &mut 0,
            )
            .unwrap(),
        )
    }

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
        let ancestors = result
            .ancestor_mask(
                d,
                &mut PlanningBudget::new(
                    super::super::PlanningLimits {
                        max_units: usize::MAX,
                    },
                    &mut 0,
                )
                .unwrap(),
            )
            .unwrap();
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
                    .zip(
                        actual
                            .ancestor_mask(
                                index,
                                &mut PlanningBudget::new(
                                    super::super::PlanningLimits {
                                        max_units: usize::MAX,
                                    },
                                    &mut 0,
                                )
                                .unwrap(),
                            )
                            .unwrap(),
                    )
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

    fn nested_export_dependency(a: &Registration, b: &Registration) -> Option<String> {
        super::nested_export_dependency(
            a,
            b,
            &mut PlanningBudget::new(
                super::super::PlanningLimits {
                    max_units: usize::MAX,
                },
                &mut 0,
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn overlapping_write<'a>(
        a: &'a BTreeSet<Footprint>,
        b: &BTreeSet<Footprint>,
    ) -> Option<&'a Footprint> {
        super::overlapping_write(
            a,
            b,
            &mut PlanningBudget::new(
                super::super::PlanningLimits {
                    max_units: usize::MAX,
                },
                &mut 0,
            )
            .unwrap(),
        )
        .unwrap()
    }

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
        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        let index = ConflictIndex::new(&registrations, &order, &footprints, &mut budget).unwrap();
        assert_eq!(
            index
                .after(0, &registrations, &order, &footprints, &mut budget)
                .unwrap()
                .len(),
            255
        );
        assert!(matches!(
            plan(&registrations, SelectionLimits::default(), &mut budget),
            Err(ComposeError::Conflict { first, second, .. })
                if first == "r0000" && second == "r0001"
        ));
    }

    #[test]
    fn successful_dense_plan_has_no_ancestor_cells_and_suffix_queries_skip_earlier_owners() {
        let n = 32;
        let definition = Arc::new(ComponentDef::map(|n: i64| n));
        let registrations: BTreeMap<_, _> = (0..n)
            .map(|i| {
                let id = format!("r{i:03}");
                let mut registration = registration(
                    &id,
                    vec![Edit::Replace {
                        path: "shared".into(),
                        definition: definition.clone(),
                    }],
                );
                registration.priority = i as i64;
                (id, registration)
            })
            .collect();
        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        let plan = super::plan(&registrations, SelectionLimits::default(), &mut budget).unwrap();
        assert_eq!(plan.active, BTreeSet::from(["r031".into()]));
        assert_eq!(budget.ancestor_cells, 0);
        assert_eq!(budget.blockers, n * (n - 1) / 2);
        assert_eq!(budget.posting_owners, 3 * n * (n - 1) / 2);
        println!(
            "dense32: ancestor_cells={} blockers={} posting_owners={} units={}",
            budget.ancestor_cells, budget.blockers, budget.posting_owners, used
        );

        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits { max_units: 10_000 },
            &mut used,
        )
        .unwrap();
        assert!(matches!(
            super::plan(&registrations, SelectionLimits { max_work: 0 }, &mut budget),
            Err(ComposeError::PlanningBudgetExceeded { limit: 10_000 })
        ));
        assert!(budget.blockers < n * (n - 1) / 2);
        assert_eq!(budget.ancestor_cells, 0);

        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        let index =
            ConflictIndex::new(&registrations, &plan.order, &plan.footprints, &mut budget).unwrap();
        let mut query_used = 0;
        let mut query = PlanningBudget::new(
            super::super::PlanningLimits { max_units: 128 },
            &mut query_used,
        )
        .unwrap();
        assert!(
            index
                .after(
                    n - 1,
                    &registrations,
                    &plan.order,
                    &plan.footprints,
                    &mut query
                )
                .unwrap()
                .is_empty()
        );
        assert_eq!(query.posting_owners, 0);
    }

    #[test]
    fn ancestry_storage_is_reserved_before_initialization_and_pending_growth() {
        let order = DependencyOrder {
            order: vec!["a".into(), "b".into()],
            parents: vec![vec![], vec![0]],
        };
        for (limit, cells) in [(1, 0), (2, 2), (3, 2)] {
            let mut used = 0;
            let mut budget =
                PlanningBudget::new(super::super::PlanningLimits { max_units: limit }, &mut used)
                    .unwrap();
            assert_eq!(
                order.ancestor_mask(1, &mut budget),
                Err(ComposeError::PlanningBudgetExceeded { limit })
            );
            assert_eq!(budget.ancestor_cells, cells);
        }
        let mut used = 0;
        let mut budget =
            PlanningBudget::new(super::super::PlanningLimits { max_units: 4 }, &mut used).unwrap();
        assert_eq!(order.ancestor_mask(1, &mut budget).unwrap(), [true, false]);
        assert_eq!(used, 4);
    }

    #[test]
    fn long_prefix_queries_and_predicates_reserve_text_before_decisions() {
        let parent = "p".repeat(512);
        let definition = Arc::new(ComponentDef::map(|n: i64| n));
        let registrations: BTreeMap<_, _> = (0..2)
            .map(|i| {
                let id = format!("r{i}");
                (
                    id.clone(),
                    registration(
                        &id,
                        vec![Edit::Replace {
                            path: format!("{parent}/c{i}"),
                            definition: definition.clone(),
                        }],
                    ),
                )
            })
            .collect();
        let order: Vec<_> = registrations.keys().cloned().collect();
        let footprints = registrations
            .iter()
            .map(|(id, r)| (id.clone(), r.patch.footprints()))
            .collect();
        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        let index = ConflictIndex::new(&registrations, &order, &footprints, &mut budget).unwrap();
        let mut used = 0;
        let mut budget =
            PlanningBudget::new(super::super::PlanningLimits { max_units: 511 }, &mut used)
                .unwrap();
        assert_eq!(
            index.after(0, &registrations, &order, &footprints, &mut budget),
            Err(ComposeError::PlanningBudgetExceeded { limit: 511 })
        );
        assert_eq!(budget.posting_owners, 0);
        let left = BTreeSet::from([Footprint::Component(parent.clone())]);
        let right = BTreeSet::from([Footprint::Component(format!("{parent}/child"))]);
        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        assert!(
            super::overlapping_write(&left, &right, &mut budget)
                .unwrap()
                .is_some()
        );
        let cost = used;
        assert!(cost > 4 * 512);
        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: cost - 1,
            },
            &mut used,
        )
        .unwrap();
        assert_eq!(
            super::overlapping_write(&left, &right, &mut budget),
            Err(ComposeError::PlanningBudgetExceeded { limit: cost - 1 })
        );
    }

    #[test]
    fn connection_queries_pay_separately_for_comparison_and_contains_scans() {
        let path = format!("{}/child", "p".repeat(512));
        let key = ConnectionKey::new(
            PortPath {
                process: path.clone(),
                port: "output".into(),
            },
            PortPath {
                process: path,
                port: "input".into(),
            },
        );
        let registrations: BTreeMap<String, Registration> =
            BTreeMap::from([("r0".into(), registration("r0", vec![Edit::Disconnect(key)]))]);
        let order = vec!["r0".into()];
        let footprints = registrations
            .iter()
            .map(|(id, r)| (id.clone(), r.patch.footprints()))
            .collect();
        let mut used = 0;
        let mut budget = PlanningBudget::new(
            super::super::PlanningLimits {
                max_units: usize::MAX,
            },
            &mut used,
        )
        .unwrap();
        let index = ConflictIndex::new(&registrations, &order, &footprints, &mut budget).unwrap();
        // ID queries/visit + full edge query + two nested endpoint scans.
        let exact = 21 + 16 * 519 + 4 * 513;
        for limit in [exact - 1, exact] {
            let mut used = 0;
            let mut budget =
                PlanningBudget::new(super::super::PlanningLimits { max_units: limit }, &mut used)
                    .unwrap();
            let result = index.after(0, &registrations, &order, &footprints, &mut budget);
            if limit == exact {
                assert!(result.unwrap().is_empty());
                assert_eq!(used, exact);
            } else {
                assert_eq!(result, Err(ComposeError::PlanningBudgetExceeded { limit }));
            }
        }
    }
}

#[cfg(test)]
mod exact_overlap_tests {
    use super::*;

    #[test]
    fn disjoint_exact_footprints_fit_a_linear_work_budget() {
        let left = (0..256)
            .map(|i| Footprint::InputExport(format!("p{:04}", i * 2)))
            .collect();
        let mut right: BTreeSet<_> = (0..256)
            .map(|i| Footprint::InputExport(format!("p{:04}", i * 2 + 1)))
            .collect();
        let mut used = 0;
        let mut budget =
            PlanningBudget::new(super::super::PlanningLimits { max_units: 20_000 }, &mut used)
                .unwrap();
        assert_eq!(overlapping_write(&left, &right, &mut budget), Ok(None));
        right.insert(Footprint::InputExport("p0256".into()));
        right.insert(Footprint::InputExport("p0002".into()));
        let mut used = 0;
        let mut budget =
            PlanningBudget::new(super::super::PlanningLimits { max_units: 20_000 }, &mut used)
                .unwrap();
        assert_eq!(
            overlapping_write(&left, &right, &mut budget),
            Ok(Some(&Footprint::InputExport("p0002".into())))
        );
    }
}
