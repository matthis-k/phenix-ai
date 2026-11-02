//! Bounded reference model for future Graph V2 capability-derived progress.
//!
//! This is NOT the production scheduler. It exercises single-wave causal
//! obligations per scope before changing existing terminal-edge semantics.
//! Deferred transfer between waves is not modeled.
//! See spec/graph-architecture-v2.4.md sections 7.7.1 and 9.4.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Scope {
    root: u64,
    wave: u64,
    activation: u64,
}

const SCOPE: Scope = Scope {
    root: 1,
    wave: 1,
    activation: 1,
};

#[derive(Clone, Copy)]
struct Edge {
    from: &'static str,
    port: &'static str,
    to: &'static str,
}

#[derive(Clone, Copy)]
struct Capability {
    scope: Scope,
    location: &'static str,
}

#[derive(Clone, Default)]
struct ProgressModel {
    edges: Vec<Edge>,
    active: BTreeMap<u64, Capability>,
    sealed: BTreeSet<Scope>,
    leases: BTreeMap<Scope, usize>,
    deliveries: Vec<(Scope, &'static str, &'static str)>,
    next: u64,
}

impl ProgressModel {
    fn new(edges: Vec<Edge>) -> Self {
        Self {
            edges,
            ..Self::default()
        }
    }

    fn register(&mut self, scope: Scope, location: &'static str) -> u64 {
        self.next += 1;
        let id = self.next;
        assert!(self
            .active
            .insert(id, Capability { scope, location })
            .is_none());
        id
    }

    fn admit(&mut self, scope: Scope, location: &'static str) -> u64 {
        assert!(!self.sealed.contains(&scope));
        self.register(scope, location)
    }

    fn seal_admission(&mut self, scope: Scope) {
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

    fn exhausted(&self, scope: Scope, target: &'static str) -> bool {
        self.sealed.contains(&scope)
            && !self
                .active
                .values()
                .any(|cap| cap.scope == scope && self.can_reach(cap.location, target))
    }

    // This single-wave model conservatively counts the full scope's leases;
    // a future region-aware runtime must attribute individual physical work.
    fn gate_ready(&self, scope: Scope, region: &[&'static str], input_admitted: bool) -> bool {
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
    let first = m.admit(SCOPE, "a");
    let second = m.admit(SCOPE, "a");
    m.seal_admission(SCOPE);
    assert!(m.complete(first, &[]).is_empty());
    assert!(!m.exhausted(SCOPE, "retry"));
    let children = m.complete(second, &["retry"]);
    assert_eq!(children.len(), 1);
    assert!(!m.exhausted(SCOPE, "retry"));
    m.complete(children[0], &[]);
    assert!(m.exhausted(SCOPE, "retry"));
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
    let a = m.admit(SCOPE, "a");
    m.seal_admission(SCOPE);
    assert!(!m.exhausted(SCOPE, "c"));
    m.complete(a, &[]);
    assert!(m.exhausted(SCOPE, "b"));
    assert!(m.exhausted(SCOPE, "c"));
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
    let a = m.admit(SCOPE, "a");
    let b = m.admit(SCOPE, "b");
    m.seal_admission(SCOPE);
    m.complete(a, &[]);
    assert!(!m.exhausted(SCOPE, "consumer"));
    let children = m.complete(b, &["out"]);
    assert_eq!(children.len(), 1);
    m.complete(children[0], &[]);
    assert!(m.exhausted(SCOPE, "consumer"));
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
    let a = m.admit(SCOPE, "a");
    let b = m.admit(SCOPE, "b");
    let ca = m.complete(a, &["out"]);
    let cb = m.complete(b, &["out"]);
    assert_ne!(ca, cb);
    assert_eq!(m.deliveries.len(), 2);
    m.seal_admission(SCOPE);
    m.complete(ca[0], &[]);
    assert!(!m.exhausted(SCOPE, "consumer"));
    m.complete(cb[0], &[]);
    assert!(m.exhausted(SCOPE, "consumer"));
}

#[test]
fn exhausted_frontier_does_not_imply_physical_drain() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "out",
        to: "b",
    }]);
    let a = m.admit(SCOPE, "a");
    m.leases.insert(SCOPE, 1);
    m.seal_admission(SCOPE);
    m.complete(a, &[]);
    assert!(m.exhausted(SCOPE, "b"));
    assert!(!m.gate_ready(SCOPE, &["b"], true));
    m.leases.insert(SCOPE, 0);
    assert!(m.gate_ready(SCOPE, &["b"], true));
    assert!(!m.gate_ready(SCOPE, &["b"], false));
}

#[test]
fn different_roots_never_hold_each_others_frontier_open() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "out",
        to: "b",
    }]);
    let other = Scope { root: 2, ..SCOPE };
    let a1 = m.admit(SCOPE, "a");
    let _a2 = m.admit(other, "a");
    m.seal_admission(SCOPE);
    m.complete(a1, &[]);
    assert!(m.exhausted(SCOPE, "b"));
    assert!(!m.exhausted(other, "b"));
    assert!(m.gate_ready(SCOPE, &["b"], true));
}

#[test]
fn waves_and_activations_isolate_frontiers_and_sealed_admission() {
    for other in [
        Scope { wave: 2, ..SCOPE },
        Scope {
            activation: 2,
            ..SCOPE
        },
    ] {
        for (first, second) in [(SCOPE, other), (other, SCOPE)] {
            let mut m = ProgressModel::new(vec![Edge {
                from: "a",
                port: "out",
                to: "b",
            }]);
            let producer = m.admit(first, "a");
            m.seal_admission(first);
            // Even an empty second scope must not inherit the first's seal.
            assert!(!m.exhausted(second, "b"));
            assert!(!m.gate_ready(second, &["b"], true));
            let independent = m.admit(second, "a");
            m.leases.insert(second, 1);
            m.complete(producer, &[]);
            assert!(m.exhausted(first, "b"));
            assert!(m.gate_ready(first, &["b"], true));
            m.seal_admission(second);
            assert!(!m.exhausted(second, "b"));
            let children = m.complete(independent, &["out"]);
            assert_eq!(children.len(), 1);
            assert!(m.exhausted(first, "b"));
            assert!(!m.exhausted(second, "b"));
            m.complete(children[0], &[]);
            assert!(m.exhausted(second, "b"));
            assert!(!m.gate_ready(second, &["b"], true));
            m.leases.insert(second, 0);
            assert!(m.gate_ready(second, &["b"], true));
        }
    }
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
    let first = m.admit(SCOPE, "a");
    let second = m.admit(SCOPE, "a");
    m.seal_admission(SCOPE);
    let left = m.complete(first, &["left"]);
    m.complete(left[0], &[]);
    assert!(!m.exhausted(SCOPE, "left"));
    assert!(!m.exhausted(SCOPE, "right"));
    let right = m.complete(second, &["right"]);
    assert!(m.exhausted(SCOPE, "left"));
    assert!(!m.exhausted(SCOPE, "right"));
    m.complete(right[0], &[]);
    assert!(m.exhausted(SCOPE, "right"));
    assert_eq!(
        m.deliveries,
        vec![(SCOPE, "a", "left"), (SCOPE, "a", "right")]
    );
}

#[test]
fn selected_output_without_a_consumer_still_settles_the_occurrence() {
    // This untyped model describes connections, not declared output ports.
    // A real declared output need not have any downstream consumer.
    let mut m = ProgressModel::default();
    let producer = m.admit(SCOPE, "a");
    m.seal_admission(SCOPE);
    assert!(m.complete(producer, &["unconnected"]).is_empty());
    assert!(m.exhausted(SCOPE, "a"));
    assert!(m.deliveries.is_empty());
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
        assert_eq!(m.exhausted(SCOPE, "consumer"), expected);
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
        let producers = [m.admit(SCOPE, "a"), m.admit(SCOPE, "b")];
        m.seal_admission(SCOPE);
        traces += explore(m, producers, choices, [false; 2], vec![]);
    }
    assert_eq!(traces, 14);
}

#[test]
fn pending_producer_settlement_assumes_external_cancellation_fencing() {
    let mut m = ProgressModel::new(vec![Edge {
        from: "a",
        port: "out",
        to: "b",
    }]);
    let pending = m.admit(SCOPE, "a");
    m.seal_admission(SCOPE);
    m.leases.insert(SCOPE, 1);
    // A cancellation request alone does not revoke native emission rights.
    assert!(!m.exhausted(SCOPE, "b"));
    assert!(!m.gate_ready(SCOPE, &["b"], true));
    // External precondition: cancellation has fenced the producer before this
    // settlement. This model does not implement or verify fencing. Physical
    // cleanup can still lag logical settlement.
    m.complete(pending, &[]);
    assert!(m.exhausted(SCOPE, "b"));
    assert!(!m.gate_ready(SCOPE, &["b"], true));
    m.leases.insert(SCOPE, 0);
    assert!(m.gate_ready(SCOPE, &["b"], true));
}

#[test]
#[should_panic(expected = "live producer")]
fn retired_occurrence_cannot_emit_again() {
    let mut m = ProgressModel::default();
    let a = m.admit(SCOPE, "a");
    m.complete(a, &[]);
    m.complete(a, &[]);
}

#[test]
#[should_panic]
fn sealed_scope_cannot_admit_a_new_external_occurrence() {
    let mut m = ProgressModel::default();
    m.seal_admission(SCOPE);
    m.admit(SCOPE, "a");
}
