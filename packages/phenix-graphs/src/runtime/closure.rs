use super::*;

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
        match &p.policy {
            Readiness::Optional { required, optional } if required.is_empty() => {
                optional.iter().any(|name| {
                    let port = p.inputs.iter().position(|port| &port.name == name).unwrap();
                    !scope.observed_inputs.contains(&(index, port))
                })
            }
            _ => false,
        }
    }

    pub(super) fn request_input_closed(
        &self,
        index: usize,
        port: usize,
        request: RequestId,
    ) -> bool {
        let scope = &self.requests[&request];
        if !scope.sealed {
            return false;
        }
        let queues = &self.processes[index].inputs[port].queues;
        if queues
            .iter()
            .any(|q| self.transport.queues()[*q].request_count(request) != 0)
        {
            return false;
        }
        let mut visited = BTreeSet::new();
        let mut stack = vec![];
        for queue in queues {
            if let QueueEndpoint::Process { index, .. } = &self.endpoints[*queue].0 {
                stack.push(*index);
            }
        }
        while let Some(producer) = stack.pop() {
            if !visited.insert(producer) {
                continue;
            }
            let p = &self.processes[producer];
            if p.request == Some(request) || self.closure_obligation(producer, request) {
                return false;
            }
            for queue in p.inputs.iter().flat_map(|port| &port.queues) {
                if self.transport.queues()[*queue].request_count(request) != 0 {
                    return false;
                }
                if let QueueEndpoint::Process { index, .. } = &self.endpoints[*queue].0 {
                    stack.push(*index);
                }
            }
        }
        true
    }

    pub(super) fn input_closed_for(
        &self,
        index: usize,
        port: usize,
        request: Option<RequestId>,
    ) -> bool {
        self.closed(index, port)
            || request.is_some_and(|request| self.request_input_closed(index, port, request))
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
