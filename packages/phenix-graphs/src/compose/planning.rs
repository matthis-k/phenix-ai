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

pub(super) fn plan(
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
) -> Result<SelectedPlan, ComposeError> {
    let mut ancestors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut order = Vec::new();
    while order.len() < registrations.len() {
        let mut progress = false;
        for (id, registration) in registrations {
            for dependency in &registration.dependencies {
                if !registrations.contains_key(dependency) {
                    return Err(ComposeError::MissingDependency {
                        registration: id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
            if ancestors.contains_key(id)
                || !registration
                    .dependencies
                    .iter()
                    .all(|d| ancestors.contains_key(d))
            {
                continue;
            }
            let mut all = registration.dependencies.clone();
            for dependency in &registration.dependencies {
                all.extend(ancestors[dependency].iter().cloned());
            }
            ancestors.insert(id.clone(), all);
            order.push(id.clone());
            progress = true;
        }
        if !progress {
            return Err(ComposeError::DependencyCycle);
        }
    }
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
