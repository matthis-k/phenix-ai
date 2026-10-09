//! Deterministic, fail-closed ordering of author-owned patch slot insertions.
//!
//! This is a pure precursor to Core's IR patch composer. It never selects
//! providers, edits a running generation, or evaluates plugin callbacks.
//! The caller must additionally validate slot cardinality, typed IR ports,
//! frame initialization and authority on the fully composed candidate.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotInsertion {
    /// Selected, authenticated contribution ID (not a plugin priority).
    pub contribution: String,
    /// Owner-published slot identity; qualified sites remain distinct.
    pub slot: String,
    /// Proposed IR node identity.
    pub node: String,
    /// Whether relative ordering changes observable side effects.
    pub effectful: bool,
    /// Stable contribution IDs that must execute before this insertion.
    pub after: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlotOrderError {
    DuplicateContribution(String),
    DuplicateNode {
        slot: String,
        node: String,
    },
    MissingDependency {
        contribution: String,
        dependency: String,
    },
    CrossSlotDependency {
        contribution: String,
        dependency: String,
    },
    CyclicOrder {
        slot: String,
    },
    AmbiguousEffects {
        slot: String,
        first: String,
        second: String,
    },
}

/// Resolve slot order without depending on plugin enumeration order.
///
/// The result is a deterministic map of slot IDs to selected contribution
/// IDs in topological order. Unordered effectful pairs are rejected even when
/// lexicographic order would produce reproducible but semantically arbitrary
/// execution. Read-only independent nodes may use identity-based tie-breaking.
pub fn resolve_slot_order(
    insertions: &[SlotInsertion],
) -> Result<BTreeMap<String, Vec<String>>, SlotOrderError> {
    let mut all = BTreeMap::new();
    let mut node_identities = BTreeSet::new();
    for item in insertions {
        if all.insert(item.contribution.clone(), item).is_some() {
            return Err(SlotOrderError::DuplicateContribution(
                item.contribution.clone(),
            ));
        }
        if !node_identities.insert((item.slot.clone(), item.node.clone())) {
            return Err(SlotOrderError::DuplicateNode {
                slot: item.slot.clone(),
                node: item.node.clone(),
            });
        }
    }
    let mut slots: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut successors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in all.values() {
        slots
            .entry(item.slot.clone())
            .or_default()
            .insert(item.contribution.clone());
        for dependency in &item.after {
            let Some(prerequisite) = all.get(dependency) else {
                return Err(SlotOrderError::MissingDependency {
                    contribution: item.contribution.clone(),
                    dependency: dependency.clone(),
                });
            };
            if prerequisite.slot != item.slot {
                return Err(SlotOrderError::CrossSlotDependency {
                    contribution: item.contribution.clone(),
                    dependency: dependency.clone(),
                });
            }
            successors
                .entry(dependency.clone())
                .or_default()
                .insert(item.contribution.clone());
        }
    }
    let mut result = BTreeMap::new();
    for (slot, members) in slots {
        // An explicit path is required between *every* effectful pair. A
        // deterministic implementation tie-breaker cannot authorize order.
        let effects: Vec<_> = members.iter().filter(|id| all[*id].effectful).collect();
        for (index, first) in effects.iter().enumerate() {
            for second in effects.iter().skip(index + 1) {
                if !reachable(first, second, &successors) && !reachable(second, first, &successors)
                {
                    return Err(SlotOrderError::AmbiguousEffects {
                        slot,
                        first: first.to_string(),
                        second: second.to_string(),
                    });
                }
            }
        }
        let mut indegree: BTreeMap<String, usize> =
            members.iter().map(|id| (id.clone(), 0)).collect();
        for member in &members {
            for next in successors.get(member).into_iter().flatten() {
                *indegree
                    .get_mut(next)
                    .expect("validated same-slot ordering") += 1;
            }
        }
        let mut ready: BTreeSet<String> = indegree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut ordered = Vec::with_capacity(members.len());
        while let Some(next) = ready.pop_first() {
            for successor in successors.get(&next).into_iter().flatten() {
                let count = indegree
                    .get_mut(successor)
                    .expect("validated same-slot ordering");
                *count -= 1;
                if *count == 0 {
                    ready.insert(successor.clone());
                }
            }
            ordered.push(next);
        }
        if ordered.len() != members.len() {
            return Err(SlotOrderError::CyclicOrder { slot });
        }
        result.insert(slot, ordered);
    }
    Ok(result)
}

fn reachable(start: &str, target: &str, successors: &BTreeMap<String, BTreeSet<String>>) -> bool {
    let mut visited = BTreeSet::new();
    let mut pending = vec![start.to_owned()];
    while let Some(node) = pending.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if let Some(next) = successors.get(&node) {
            for successor in next {
                if successor == target {
                    return true;
                }
                pending.push(successor.clone());
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insertion(id: &str, slot: &str, effectful: bool, after: &[&str]) -> SlotInsertion {
        SlotInsertion {
            contribution: id.into(),
            slot: slot.into(),
            node: format!("node-{id}"),
            effectful,
            after: after.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn identical_results_under_reversed_discovery_and_transitive_ordering() {
        let original = vec![
            insertion("c", "tools.before", true, &["b"]),
            insertion("a", "tools.before", true, &[]),
            insertion("b", "tools.before", true, &["a"]),
            insertion("z", "audit", false, &[]),
        ];
        let mut reversed = original.clone();
        reversed.reverse();
        let expected = BTreeMap::from([
            (
                "tools.before".into(),
                vec!["a".into(), "b".into(), "c".into()],
            ),
            ("audit".into(), vec!["z".into()]),
        ]);
        assert_eq!(resolve_slot_order(&original), Ok(expected.clone()));
        assert_eq!(resolve_slot_order(&reversed), Ok(expected));
    }

    #[test]
    fn ambiguous_effectful_insertions_are_not_silently_alphabetized() {
        let entries = vec![
            insertion("a", "tool", true, &[]),
            insertion("b", "tool", true, &[]),
        ];
        assert!(matches!(
            resolve_slot_order(&entries),
            Err(SlotOrderError::AmbiguousEffects { .. })
        ));
    }

    #[test]
    fn equal_duplicate_ids_still_fail() {
        let a = insertion("x", "tool", false, &[]);
        assert_eq!(
            resolve_slot_order(&[a.clone(), a]),
            Err(SlotOrderError::DuplicateContribution("x".into()))
        );
    }

    #[test]
    fn unknown_cross_slot_and_cyclic_constraints_fail_closed() {
        assert!(matches!(
            resolve_slot_order(&[insertion("a", "tool", true, &["missing"])]),
            Err(SlotOrderError::MissingDependency { .. })
        ));
        assert!(matches!(
            resolve_slot_order(&[
                insertion("a", "tool", false, &["b"]),
                insertion("b", "audit", false, &[]),
            ]),
            Err(SlotOrderError::CrossSlotDependency { .. })
        ));
        assert!(matches!(
            resolve_slot_order(&[
                insertion("a", "tool", false, &["b"]),
                insertion("b", "tool", false, &["a"]),
            ]),
            Err(SlotOrderError::CyclicOrder { .. })
        ));
    }

    #[test]
    fn private_subplan_slots_do_not_collide_with_parent_slot_identity() {
        let items = vec![
            insertion("parent", "tool", true, &[]),
            insertion("child", "__include__/child/tool", true, &[]),
        ];
        let resolved = resolve_slot_order(&items).unwrap();
        assert_eq!(resolved.len(), 2);
    }
}
