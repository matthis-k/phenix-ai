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

#[derive(Clone, Default)]
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
        assert!(selected.iter().all(|port| {
            self.edges
                .iter()
                .any(|edge| edge.from == producer.location && edge.port == *port)
        }));
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
    fn gate_ready(&self, scope: u64, region: &[&'static str], input_admitted: bool) -> bool {
        input_admitted
            && self.sealed.contains(&scope)
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
    assert!(!m.gate_ready(1, &["b"], true));
    m.leases.insert(1, 0);
    assert!(m.gate_ready(1, &["b"], true));
    assert!(!m.gate_ready(1, &["b"], false));
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
    assert!(m.gate_ready(1, &["b"], true));
}

#[test]
fn opposite_outputs_of_repeated_invocations_remain_independent() {
    let mut m = ProgressModel::new(vec![
        Edge {
            from: "a",
            port: "left",
            to: "left",
        },
        Edge {
            from: "a",
            port: "right",
            to: "right",
        },
    ]);
    let first = m.admit(1, "a");
    let second = m.admit(1, "a");
    m.seal_admission(1);
    let left = m.complete(first, &["left"]);
    m.complete(left[0], &[]);
    assert!(!m.exhausted(1, "left"));
    assert!(!m.exhausted(1, "right"));
    let right = m.complete(second, &["right"]);
    assert!(m.exhausted(1, "left"));
    assert!(!m.exhausted(1, "right"));
    m.complete(right[0], &[]);
    assert!(m.exhausted(1, "right"));
    assert_eq!(m.deliveries, vec![(1, "a", "left"), (1, "a", "right")]);
}

#[test]
fn all_bounded_completion_orders_preserve_frontiers_and_delivery_counts() {
    // Independent oracle for this topology: each unfinished producer can still
    // reach the consumer; each delivered but unfinished consumer owns work too.
    // Explore all schedules, including consuming the first delivery before the
    // other producer settles, and every emit/skip choice for both producers.
    fn explore(
        m: ProgressModel,
        producers: [u64; 2],
        choices: [bool; 2],
        finished: [bool; 2],
        consumers: Vec<u64>,
    ) -> usize {
        let expected = finished.iter().all(|done| *done) && consumers.is_empty();
        assert_eq!(m.exhausted(1, "consumer"), expected);
        let emitted = (0..2).filter(|i| finished[*i] && choices[*i]).count();
        assert_eq!(m.deliveries.len(), emitted);
        if expected {
            assert_eq!(emitted, choices.iter().filter(|emit| **emit).count());
            return 1;
        }
        let mut traces = 0;
        for i in 0..2 {
            if !finished[i] {
                let mut next = m.clone();
                let mut done = finished;
                done[i] = true;
                let mut pending = consumers.clone();
                pending
                    .extend(next.complete(producers[i], if choices[i] { &["out"] } else { &[] }));
                traces += explore(next, producers, choices, done, pending);
            }
        }
        for i in 0..consumers.len() {
            let mut next = m.clone();
            let mut pending = consumers.clone();
            next.complete(pending.remove(i), &[]);
            traces += explore(next, producers, choices, finished, pending);
        }
        traces
    }
    let mut traces = 0;
    for choices in [[false, false], [false, true], [true, false], [true, true]] {
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
        let producers = [m.admit(1, "a"), m.admit(1, "b")];
        m.seal_admission(1);
        traces += explore(m, producers, choices, [false; 2], vec![]);
    }
    assert_eq!(traces, 14);
}

#[test]
fn pending_producer_retains_progress_until_cancellation_is_fenced() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "out",
        to: "b",
    }]);
    let pending = m.admit(1, "a");
    m.seal_admission(1);
    m.leases.insert(1, 1);
    // A cancellation request alone does not revoke native emission rights.
    assert!(!m.exhausted(1, "b"));
    assert!(!m.gate_ready(1, &["b"], true));
    // Only a fenced producer can settle without output. Physical cleanup can
    // still lag logical settlement, as with a ready-but-not-quiescent ticket.
    m.complete(pending, &[]);
    assert!(m.exhausted(1, "b"));
    assert!(!m.gate_ready(1, &["b"], true));
    m.leases.insert(1, 0);
    assert!(m.gate_ready(1, &["b"], true));
}

#[test]
#[should_panic(expected = "live producer")]
fn retired_occurrence_cannot_emit_again() {
    let mut m = ProgressModel::default();
    let a = m.admit(1, "a");
    m.complete(a, &[]);
    m.complete(a, &[]);
}

#[test]
#[should_panic]
fn sealed_scope_cannot_admit_a_new_external_occurrence() {
    let mut m = ProgressModel::default();
    m.seal_admission(1);
    m.admit(1, "a");
}
