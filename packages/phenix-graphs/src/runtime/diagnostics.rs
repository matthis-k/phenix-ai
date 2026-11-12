use super::*;

#[derive(Clone, Debug)]
pub struct BlockedCycle {
    pub processes: Vec<usize>,
    pub queues: Vec<QueueWait>,
}

/// Snapshot of `Readiness::All` input heads that cannot be correlated
/// without changing their FIFO order. It is not a proof of global deadlock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorrelationBlock {
    pub process: usize,
    pub path: String,
    /// One entry per required input port: all scoped requests present at
    /// physical queue heads (not at any later position in the queue).
    pub heads: Vec<(String, Vec<RequestId>)>,
}
impl NetworkSession {
    /// Inspect authenticated request heads without driving the session.
    /// Does not bypass FIFO, inspect packet values or call readiness policies.
    /// A mismatched snapshot may become obsolete after other graph activity.
    pub fn correlation_blocks(&self) -> Vec<CorrelationBlock> {
        let mut result = Vec::new();
        for (index, process) in self.processes.iter().enumerate() {
            if process.done
                || process.handoff.state() != HandoffState::Idle
                || process.inputs.len() < 2
                || !matches!(
                    process.waiting_policy.as_ref().unwrap_or(&process.policy),
                    Readiness::All
                )
            {
                continue;
            }
            let mut heads = Vec::with_capacity(process.inputs.len());
            let mut intersection: Option<BTreeSet<RequestId>> = None;
            let mut eligible = true;
            for port in &process.inputs {
                let mut ids = BTreeSet::new();
                for queue in &port.queues {
                    let Some(packet) = self.transport.queues()[*queue].head() else {
                        continue;
                    };
                    let Some(request) = packet.metadata().and_then(|m| m.request()) else {
                        eligible = false;
                        break;
                    };
                    if !self.requests.contains_key(&request) {
                        eligible = false;
                        break;
                    }
                    ids.insert(request);
                }
                if !eligible || ids.is_empty() {
                    eligible = false;
                    break;
                }
                intersection = Some(match intersection {
                    Some(previous) => previous.intersection(&ids).copied().collect(),
                    None => ids.clone(),
                });
                heads.push((port.name.clone(), ids.into_iter().collect()));
            }
            if eligible && intersection.as_ref().is_some_and(BTreeSet::is_empty) {
                result.push(CorrelationBlock {
                    process: index,
                    path: self.compiled.processes()[index].path().to_owned(),
                    heads,
                });
            }
        }
        result
    }

    pub(super) fn fail_input(&mut self, index: usize, reason: &str) {
        self.input_failure = self.processes[index]
            .inputs
            .iter()
            .flat_map(|port| &port.queues)
            .map(|q| {
                let queue = &self.transport.queues()[*q];
                QueueWait {
                    queue: *q,
                    producer: self.endpoints[*q].0.clone(),
                    consumer: self.endpoints[*q].1.clone(),
                    occupancy: queue.len(),
                    capacity: queue.capacity(),
                }
            })
            .collect();
        let packets: usize = self.input_failure.iter().map(|queue| queue.occupancy).sum();
        let ports: Vec<_> = self.processes[index]
            .inputs
            .iter()
            .map(|port| {
                format!(
                    "{}={}",
                    port.name,
                    port.queues
                        .iter()
                        .map(|q| self.transport.queues()[*q].len())
                        .sum::<usize>()
                )
            })
            .collect();
        self.fail_process(
            index,
            format!(
                "{reason} at {}: {packets} accepted undelivered packets ({})",
                self.compiled.processes()[index].path(),
                ports.join(", ")
            ),
        );
    }
    /// Strongly connected components of actual full-queue publication waits.
    /// Capture consumers are external and cannot create a process SCC.
    pub fn blocked_cycles(&self) -> Vec<BlockedCycle> {
        let waits = self.blocked_queues();
        let count = self.processes.len();
        let mut edges = vec![vec![]; count];
        let mut reverse = vec![vec![]; count];
        for wait in &waits {
            if let (
                QueueEndpoint::Process { index: from, .. },
                QueueEndpoint::Process { index: to, .. },
            ) = (&wait.producer, &wait.consumer)
            {
                edges[*from].push(*to);
                reverse[*to].push(*from);
            }
        }
        // Iterative Kosaraju avoids native stack growth on large cycles.
        let mut visited = vec![false; count];
        let mut order = vec![];
        for root in 0..count {
            if visited[root] {
                continue;
            }
            visited[root] = true;
            let mut stack = vec![(root, 0)];
            while let Some((node, next)) = stack.last_mut() {
                if *next < edges[*node].len() {
                    let child = edges[*node][*next];
                    *next += 1;
                    if !visited[child] {
                        visited[child] = true;
                        stack.push((child, 0));
                    }
                } else {
                    let (node, _) = stack.pop().unwrap();
                    order.push(node);
                }
            }
        }
        visited.fill(false);
        let mut result = vec![];
        for root in order.into_iter().rev() {
            if visited[root] {
                continue;
            }
            let mut members = vec![];
            let mut stack = vec![root];
            visited[root] = true;
            while let Some(node) = stack.pop() {
                members.push(node);
                for parent in &reverse[node] {
                    if !visited[*parent] {
                        visited[*parent] = true;
                        stack.push(*parent);
                    }
                }
            }
            if members.len() == 1 && !edges[root].contains(&root) {
                continue;
            }
            members.sort_unstable();
            let queues = waits.iter().filter(|wait| matches!((&wait.producer, &wait.consumer), (QueueEndpoint::Process { index: a, .. }, QueueEndpoint::Process { index: b, .. }) if members.binary_search(a).is_ok() && members.binary_search(b).is_ok())).cloned().collect();
            result.push(BlockedCycle {
                processes: members,
                queues,
            });
        }
        result
    }
}
