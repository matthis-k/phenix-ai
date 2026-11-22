use super::*;

/// A proof of current input closure, independent of whether it was observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureProof {
    GlobalEos,
    ScopedRequestAbsent,
    NotProven(ClosureWitness),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureWitness {
    OpenGlobalWriter { queue: usize },
    BufferedGlobalInput { queue: usize },
    UnsealedRequest { request: RequestId },
    RequestPacket { queue: usize, count: usize },
    UpstreamOwner { process: usize },
    ClosureObligation { process: usize },
}

/// Ephemeral closure DFS membership. A short upstream walk needs no heap
/// allocation; dense reachable chains use one indexed vector instead of one
/// tree allocation per ancestor. Sparse scopes avoid a whole-inventory bitset.
/// The membership set is local to one immutable closure proof (no stale cache).
enum ClosureVisited {
    Small { members: [usize; 16], len: usize },
    Dense(Vec<bool>),
    Sparse(BTreeSet<usize>),
}

impl Default for ClosureVisited {
    fn default() -> Self {
        Self::Small {
            members: [usize::MAX; 16],
            len: 0,
        }
    }
}

impl ClosureVisited {
    /// Return true for an unseen process. Cycle and diamond deduplication
    /// remains exact; never omit a producer or reorder the DFS stack.
    fn insert(&mut self, index: usize, total: usize, use_dense: bool) -> bool {
        match self {
            Self::Dense(members) => {
                if members[index] {
                    false
                } else {
                    members[index] = true;
                    true
                }
            }
            Self::Sparse(members) => members.insert(index),
            Self::Small { members, len } => {
                if members[..*len].contains(&index) {
                    return false;
                }
                if *len < members.len() {
                    members[*len] = index;
                    *len += 1;
                    return true;
                }
                if use_dense {
                    let mut next = vec![false; total];
                    for &member in members.iter() {
                        next[member] = true;
                    }
                    next[index] = true;
                    *self = Self::Dense(next);
                } else {
                    let mut next = members.iter().copied().collect::<BTreeSet<_>>();
                    next.insert(index);
                    *self = Self::Sparse(next);
                }
                true
            }
        }
    }
}

impl NetworkSession {
    pub(super) fn stranded_at_eos(&self, index: usize) -> bool {
        let p = &self.processes[index];
        if !p
            .inputs
            .iter()
            .flat_map(|port| &port.queues)
            .all(|q| self.transport.queues()[*q].writer_closed())
        {
            return false;
        }
        let available = p
            .inputs
            .iter()
            .filter(|port| {
                port.queues
                    .iter()
                    .any(|q| !self.transport.queues()[*q].is_empty())
            })
            .count();
        if available == 0 {
            return p.handoff.state() == HandoffState::WaitingInput;
        }
        match p.waiting_policy.as_ref().unwrap_or(&p.policy) {
            Readiness::All | Readiness::CorrelatedAll => available < p.inputs.len(),
            Readiness::AtLeast(n) => available < n.get(),
            Readiness::Optional { required, optional } => {
                required.iter().any(|name| {
                    let port = p.inputs.iter().position(|port| &port.name == name).unwrap();
                    self.closed(index, port)
                }) || p.inputs.iter().any(|port| {
                    !required.contains(&port.name)
                        && !optional.contains(&port.name)
                        && port
                            .queues
                            .iter()
                            .any(|q| !self.transport.queues()[*q].is_empty())
                })
            }
            _ => false,
        }
    }
    pub(super) fn note_request_ingress(&mut self, request: RequestId, name: &str) {
        let roots: Vec<_> = self.compiled.inputs()[name]
            .iter()
            .map(|(index, _)| *index)
            .collect();
        for root in roots {
            self.note_request_root(request, root);
        }
    }

    pub(super) fn note_request_root(&mut self, request: RequestId, root: usize) {
        let scope = self.requests.get_mut(&request).expect("admitted request");
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            if !scope.reachable.insert(index) {
                continue;
            }
            for child in &self.downstream[index] {
                stack.push(*child);
            }
        }
    }

    /// Registered by actual request roots, not discovered by a zero-packet
    /// settlement callback. An all-optional downstream consumer still owes its
    /// scoped closure notification even when occupied by another request.
    pub(super) fn closure_obligation(&self, index: usize, request: RequestId) -> bool {
        let scope = &self.requests[&request];
        let p = &self.processes[index];
        if scope.terminal.is_some() || p.done || !scope.reachable.contains(&index) {
            return false;
        }
        let names = match &p.policy {
            Readiness::Optional { required, optional } if required.is_empty() => {
                optional.as_slice()
            }
            Readiness::Custom(_) => scope
                .closure_ports
                .get(&index)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            _ => return false,
        };
        names.iter().any(|name| {
            let port = p
                .inputs
                .iter()
                .position(|port| &port.name == name)
                .expect("validated closure readiness port");
            !scope.observed_inputs.contains(&(index, port))
        })
    }

    fn scoped_closure_proof(&self, index: usize, port: usize, request: RequestId) -> ClosureProof {
        let scope = &self.requests[&request];
        if !scope.sealed {
            return ClosureProof::NotProven(ClosureWitness::UnsealedRequest { request });
        }
        // Request roots register their entire downstream reachability. An
        // upstream process outside this scope cannot own or publish packets
        // bearing this request ID. Host ingress queues remain checked.
        // A request reaching the entire compiled graph has nothing to prune.
        // Avoid repeated tree lookups on long, wholly reachable closure chains.
        let whole_graph = scope.reachable.len() == self.processes.len();
        let relevant = |queue: usize| {
            whole_graph
                || match &self.endpoints[queue].0 {
                    QueueEndpoint::Process { index, .. } => scope.reachable.contains(index),
                    QueueEndpoint::Input(_) | QueueEndpoint::Capture(_) => true,
                }
        };
        let queues = &self.processes[index].inputs[port].queues;
        for queue in queues.iter().copied().filter(|q| relevant(*q)) {
            let count = self.transport.queues()[queue].request_count(request);
            if count != 0 {
                return ClosureProof::NotProven(ClosureWitness::RequestPacket { queue, count });
            }
        }
        let mut visited = ClosureVisited::default();
        let mut stack = vec![];
        for queue in queues {
            if let QueueEndpoint::Process { index, .. } = &self.endpoints[*queue].0
                && (whole_graph || scope.reachable.contains(index))
            {
                stack.push(*index);
            }
        }
        while let Some(producer) = stack.pop() {
            if !visited.insert(
                producer,
                self.processes.len(),
                scope.reachable.len().saturating_mul(4) >= self.processes.len(),
            ) {
                continue;
            }
            let p = &self.processes[producer];
            if p.request == Some(request) {
                return ClosureProof::NotProven(ClosureWitness::UpstreamOwner {
                    process: producer,
                });
            }
            if self.closure_obligation(producer, request) {
                return ClosureProof::NotProven(ClosureWitness::ClosureObligation {
                    process: producer,
                });
            }
            for queue in p.inputs.iter().flat_map(|port| &port.queues) {
                if !relevant(*queue) {
                    continue;
                }
                let count = self.transport.queues()[*queue].request_count(request);
                if count != 0 {
                    return ClosureProof::NotProven(ClosureWitness::RequestPacket {
                        queue: *queue,
                        count,
                    });
                }
                if let QueueEndpoint::Process { index, .. } = &self.endpoints[*queue].0 {
                    stack.push(*index);
                }
            }
        }
        ClosureProof::ScopedRequestAbsent
    }

    pub(super) fn input_closure_proof(
        &self,
        index: usize,
        port: usize,
        request: Option<RequestId>,
    ) -> ClosureProof {
        let Some(queue) = self.first_non_eos_queue(index, port) else {
            return ClosureProof::GlobalEos;
        };
        if let Some(request) = request {
            return self.scoped_closure_proof(index, port, request);
        }
        ClosureProof::NotProven(if !self.transport.queues()[queue].is_empty() {
            ClosureWitness::BufferedGlobalInput { queue }
        } else {
            ClosureWitness::OpenGlobalWriter { queue }
        })
    }

    pub(super) fn input_closed_for(
        &self,
        index: usize,
        port: usize,
        request: Option<RequestId>,
    ) -> bool {
        !matches!(
            self.input_closure_proof(index, port, request),
            ClosureProof::NotProven(_)
        )
    }

    pub(super) fn closure_fresh(
        &self,
        index: usize,
        port: usize,
        request: Option<RequestId>,
    ) -> bool {
        let p = &self.processes[index];
        if p.handoff.state() == HandoffState::WaitingInput {
            return !p.closed_in_activation.contains(&port);
        }
        match request {
            Some(request) => !self.requests[&request]
                .observed_inputs
                .contains(&(index, port)),
            None => !p.inputs[port].eos_delivered,
        }
    }
}

#[cfg(test)]
mod closure_visited_tests {
    use super::ClosureVisited;

    #[test]
    fn small_graph_walk_deduplicates_cycles_without_allocating() {
        let mut seen = ClosureVisited::default();
        for index in [8, 2, 4, 1, 9, 7, 11, 0, 13, 3, 14, 5, 6, 12, 15, 10] {
            assert!(seen.insert(index, 128, true));
            assert!(!seen.insert(index, 128, true));
        }
        assert!(matches!(seen, ClosureVisited::Small { len: 16, .. }));
        assert!(!seen.insert(8, 128, false));
    }

    #[test]
    fn dense_closure_relocation_remains_exact_across_promotion() {
        let mut seen = ClosureVisited::default();
        for index in (0..160).rev() {
            assert!(seen.insert(index, 192, true));
            assert!(!seen.insert(index, 192, true));
        }
        assert!(matches!(seen, ClosureVisited::Dense(_)));
        for index in (0..160).rev() {
            assert!(!seen.insert(index, 192, true));
        }
        for index in 160..192 {
            assert!(seen.insert(index, 192, true));
        }
        assert!(!seen.insert(191, 192, true));
    }

    #[test]
    fn sparse_closure_does_not_allocate_a_whole_physical_inventory() {
        let mut seen = ClosureVisited::default();
        let members: Vec<_> = (0..80).map(|n| n * 1000 + 19).collect();
        for index in &members {
            assert!(seen.insert(*index, 100_000, false));
            assert!(!seen.insert(*index, 100_000, false));
        }
        assert!(matches!(seen, ClosureVisited::Sparse(_)));
        for index in members {
            assert!(!seen.insert(index, 100_000, false));
        }
        assert!(seen.insert(99_999, 100_000, false));
    }
}
