//! Bounded reference model for future Graph V2 capability-derived progress.
//!
//! This is NOT the production scheduler. It exercises single-wave causal
//! obligations before changing existing terminal-edge semantics.
//! See spec/graph-architecture-v2.4.md sections 7.7.1 and 9.4.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy)]
struct Edge {
    from: &'static str,
    port: &'static str,
    to: &'static str,
}

#[derive(Clone, Copy)]
struct Capability {
    scope: u64,
    location: &'static str,
}

#[derive(Default)]
struct ProgressModel {
    edges: Vec<Edge>,
    active: BTreeMap<u64, Capability>,
    sealed: BTreeSet<u64>,
    leases: BTreeMap<u64, usize>,
    deliveries: Vec<(u64, &'static str, &'static str)>,
    next: u64,
}

impl ProgressModel {
    fn new(edges: Vec<Edge>) -> Self {
        Self {
            edges,
            ..Self::default()
        }
    }

    fn register(&mut self, scope: u64, location: &'static str) -> u64 {
        self.next += 1;
        let id = self.next;
        assert!(self
            .active
            .insert(id, Capability { scope, location })
            .is_none());
        id
    }

    fn admit(&mut self, scope: u64, location: &'static str) -> u64 {
        assert!(!self.sealed.contains(&scope));
        self.register(scope, location)
    }

    fn seal_admission(&mut self, scope: u64) {
        self.sealed.insert(scope);
    }

    // Atomic transition: register downstream obligations before retiring
    // the producer. Unselected outputs register no delivery/closure event.
    fn complete(&mut self, id: u64, selected: &[&str]) -> Vec<u64> {
        let producer = *self.active.get(&id).expect("live producer");
        let mut children = Vec::new();
        for edge in self.edges.clone() {
            if edge.from == producer.location && selected.contains(&edge.port) {
                self.deliveries.push((producer.scope, edge.from, edge.to));
                children.push(self.register(producer.scope, edge.to));
            }
        }
        assert!(self.active.remove(&id).is_some());
        children
    }

    // Conservative potential future reachability, irrespective of unknown
    // plugin-side route choices; cycles terminate through visited nodes.
    fn can_reach(&self, from: &'static str, target: &'static str) -> bool {
        let mut seen = BTreeSet::new();
        let mut todo = VecDeque::from([from]);
        while let Some(node) = todo.pop_front() {
            if node == target {
                return true;
            }
            if !seen.insert(node) {
                continue;
            }
            for edge in &self.edges {
                if edge.from == node {
                    todo.push_back(edge.to);
                }
            }
        }
        false
    }

    fn exhausted(&self, scope: u64, target: &'static str) -> bool {
        self.sealed.contains(&scope)
            && !self
                .active
                .values()
                .any(|cap| cap.scope == scope && self.can_reach(cap.location, target))
    }

    // This single-wave model conservatively counts the full scope's leases;
    // a future region-aware runtime must attribute individual physical work.
    fn gate_ready(&self, scope: u64, region: &[&'static str]) -> bool {
        self.sealed.contains(&scope)
            && region.iter().all(|node| self.exhausted(scope, node))
            && self.leases.get(&scope).copied().unwrap_or(0) == 0
    }
}

#[test]
fn one_skip_cannot_close_another_invocations_output() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "retry",
        to: "retry",
    }]);
    let first = m.admit(1, "a");
    let second = m.admit(1, "a");
    m.seal_admission(1);
    assert!(m.complete(first, &[]).is_empty());
    assert!(!m.exhausted(1, "retry"));
    let children = m.complete(second, &["retry"]);
    assert_eq!(children.len(), 1);
    assert!(!m.exhausted(1, "retry"));
    m.complete(children[0], &[]);
    assert!(m.exhausted(1, "retry"));
}

#[test]
fn unused_branch_exhausts_without_explicit_close_delivery() {
    let mut m = ProgressModel::new(vec![
        Edge {
            from: "a",
            port: "retry",
            to: "b",
        },
        Edge {
            from: "b",
            port: "next",
            to: "c",
        },
    ]);
    let a = m.admit(1, "a");
    m.seal_admission(1);
    assert!(!m.exhausted(1, "c"));
    m.complete(a, &[]);
    assert!(m.exhausted(1, "b"));
    assert!(m.exhausted(1, "c"));
    assert!(m.deliveries.is_empty());
}

#[test]
fn alternate_producer_keeps_any_input_possible() {
    let mut m = ProgressModel::new(vec![
        Edge {
            from: "a",
            port: "out",
            to: "consumer",
        },
        Edge {
            from: "b",
            port: "out",
            to: "consumer",
        },
    ]);
    let a = m.admit(1, "a");
    let b = m.admit(1, "b");
    m.seal_admission(1);
    m.complete(a, &[]);
    assert!(!m.exhausted(1, "consumer"));
    let children = m.complete(b, &["out"]);
    assert_eq!(children.len(), 1);
    m.complete(children[0], &[]);
    assert!(m.exhausted(1, "consumer"));
}

#[test]
fn each_delivery_creates_a_separate_invocation() {
    let mut m = ProgressModel::new(vec![
        Edge {
            from: "a",
            port: "out",
            to: "consumer",
        },
        Edge {
            from: "b",
            port: "out",
            to: "consumer",
        },
    ]);
    let a = m.admit(1, "a");
    let b = m.admit(1, "b");
    let ca = m.complete(a, &["out"]);
    let cb = m.complete(b, &["out"]);
    assert_ne!(ca, cb);
    assert_eq!(m.deliveries.len(), 2);
    m.seal_admission(1);
    m.complete(ca[0], &[]);
    assert!(!m.exhausted(1, "consumer"));
    m.complete(cb[0], &[]);
    assert!(m.exhausted(1, "consumer"));
}

#[test]
fn exhausted_frontier_does_not_imply_physical_drain() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "out",
        to: "b",
    }]);
    let a = m.admit(1, "a");
    m.leases.insert(1, 1);
    m.seal_admission(1);
    m.complete(a, &[]);
    assert!(m.exhausted(1, "b"));
    assert!(!m.gate_ready(1, &["b"]));
    m.leases.insert(1, 0);
    assert!(m.gate_ready(1, &["b"]));
}

#[test]
fn different_roots_never_hold_each_others_frontier_open() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "out",
        to: "b",
    }]);
    let a1 = m.admit(1, "a");
    let _a2 = m.admit(2, "a");
    m.seal_admission(1);
    m.complete(a1, &[]);
    assert!(m.exhausted(1, "b"));
    assert!(!m.exhausted(2, "b"));
    assert!(m.gate_ready(1, &["b"]));
}
