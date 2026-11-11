use super::*;

#[derive(Clone, Debug)]
pub struct BlockedCycle {
    pub processes: Vec<usize>,
    pub queues: Vec<QueueWait>,
}
impl NetworkSession {
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
