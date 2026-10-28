//! Private finite initial-wave readiness. Effects and tickets remain in GraphExecution.
use super::*;
use crate::{graph::GraphExecution, resolve::Selection, runtime::Outcome, Call};
use phenix_contract::ScopedRequest;

const MAX_ROOT_VALUES: usize = 16_384;
const MAX_ROOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ROOT_NODES: usize = 4_096;

#[derive(Clone)]
enum Resolution {
    Unresolved,
    // None retains successful terminal bookkeeping only for a non-runnable Join.
    Value(Option<Value>, BTreeSet<String>),
    Closed,
    Failed,
}

pub(in crate::graph) struct Wave {
    // This profile admits exactly one initial wave under the existing Root ID.
    _id: u64,
    source: String,
    nodes: BTreeMap<String, (String, NodeDecl)>,
    edges: BTreeMap<String, EdgeDecl>,
    arrivals: BTreeMap<String, Resolution>,
    started: BTreeSet<String>,
    done: BTreeSet<String>,
    resources: BTreeMap<String, BTreeSet<String>>,
    values: usize,
    bytes: usize,
}

impl Wave {
    pub(in crate::graph) fn new(graph: &CompiledGraph, source: &str) -> Result<Self, Failure> {
        let nodes: BTreeMap<_, _> = graph
            .declarations
            .iter()
            .flat_map(|d| {
                d.nodes
                    .iter()
                    .map(|n| (n.id.clone(), (d.contributor.clone(), n.clone())))
            })
            .collect();
        if nodes.len() > MAX_ROOT_NODES {
            return Err(fail(
                Code::InfrastructureFailure,
                "graph-v2 root node limit",
            ));
        }
        let edges: BTreeMap<_, _> = graph
            .declarations
            .iter()
            .flat_map(|d| d.edges.iter().map(|e| (e.id.clone(), e.clone())))
            .collect();
        let arrivals = edges
            .iter()
            .filter(|(_, e)| matches!(e.kind, EdgeKind::Flow { .. }))
            .map(|(id, _)| (id.clone(), Resolution::Unresolved))
            .collect();
        Ok(Self {
            _id: 0,
            source: source.into(),
            nodes,
            edges,
            arrivals,
            started: BTreeSet::new(),
            done: BTreeSet::new(),
            resources: BTreeMap::new(),
            values: 0,
            bytes: 0,
        })
    }

    fn deliver(
        &mut self,
        graph: &CompiledGraph,
        scope: &ScopedRequest,
        node: &str,
        selected: Option<(&str, &Value)>,
        failed: bool,
    ) -> Result<(), Failure> {
        if self.done.contains(node) {
            return Ok(());
        }
        if let Some((_, value)) = selected {
            self.values += 1;
            self.bytes = self
                .bytes
                .checked_add(value.encode()?.len())
                .ok_or_else(|| fail(Code::InfrastructureFailure, "graph-v2 byte limit"))?;
            if self.values > MAX_ROOT_VALUES || self.bytes > MAX_ROOT_BYTES {
                return Err(fail(
                    Code::InfrastructureFailure,
                    "graph-v2 root value/byte limit",
                ));
            }
        }
        let mut joins = BTreeSet::new();
        // Resolve sibling closure/failure before any successful delivery. Edge ID
        // order must not authorize a payload that this same completion discards.
        for (id, edge) in &self.edges {
            let EdgeKind::Flow { from, to } = &edge.kind else {
                continue;
            };
            if from.node != node {
                continue;
            }
            if matches!(self.nodes[&to.node].1.kind, NodeKind::Join { .. }) {
                joins.insert(to.node.clone());
            }
            if selected.is_none_or(|(port, _)| port != from.port) {
                self.arrivals.insert(
                    id.clone(),
                    if failed {
                        Resolution::Failed
                    } else {
                        Resolution::Closed
                    },
                );
            }
        }
        for join in joins {
            self.release_join_payloads(&join)?;
        }
        let (owner, definition) = &self.nodes[node];
        let inherited = self
            .resources
            .get(node)
            .unwrap_or(&scope.authority.resources);
        for (id, edge) in &self.edges {
            let EdgeKind::Flow { from, to } = &edge.kind else {
                continue;
            };
            if from.node != node {
                continue;
            }
            if let Some((port, value)) = selected.filter(|(port, _)| *port == from.port) {
                if matches!(self.nodes[&to.node].1.kind, NodeKind::Join { .. })
                    && self.join_non_runnable(&to.node)?
                {
                    self.arrivals
                        .insert(id.clone(), Resolution::Value(None, inherited.clone()));
                    continue;
                }
                let output = definition.outputs.iter().find(|p| p.name == port).unwrap();
                let (target_owner, _) = &self.nodes[&to.node];
                let mut resources = inherited.clone();
                if owner != target_owner
                    || graph.routes.get(node).unwrap_or(owner)
                        != graph.routes.get(&to.node).unwrap_or(target_owner)
                {
                    let grant = graph
                        .policy
                        .delivery
                        .get(&(
                            from.node.clone(),
                            from.port.clone(),
                            to.node.clone(),
                            to.port.clone(),
                        ))
                        .ok_or_else(|| denied("missing exact runtime flow grant"))?;
                    resources.retain(|r| grant.resources.contains(r));
                    if output.security.confidentiality > grant.maximum_classification
                        || resources.is_empty()
                        || scope
                            .resource
                            .as_ref()
                            .is_some_and(|r| !resources.contains(r))
                    {
                        return Err(denied("flow outside effective root resource scope"));
                    }
                }
                self.values += 1;
                self.bytes = self
                    .bytes
                    .checked_add(value.encode()?.len())
                    .ok_or_else(|| fail(Code::InfrastructureFailure, "graph-v2 byte limit"))?;
                if self.values > MAX_ROOT_VALUES || self.bytes > MAX_ROOT_BYTES {
                    return Err(fail(
                        Code::InfrastructureFailure,
                        "graph-v2 root value/byte limit",
                    ));
                }
                self.arrivals.insert(
                    id.clone(),
                    Resolution::Value(Some(value.clone()), resources),
                );
            }
        }
        self.done.insert(node.into());
        Ok(())
    }

    pub(in crate::graph) fn complete(
        &mut self,
        graph: &CompiledGraph,
        scope: &ScopedRequest,
        node: &str,
        outcome: &Outcome,
    ) -> Result<bool, Failure> {
        let policy = self.nodes[node].1.execution.on_domain_error;
        match outcome {
            Outcome::Success(value) => {
                self.deliver(graph, scope, node, Some(("ok", value)), false)?
            }
            Outcome::Domain(value) if policy == DomainErrorPolicy::RouteErr => {
                self.deliver(graph, scope, node, Some(("err", value)), false)?
            }
            Outcome::Domain(value) => {
                // Unrouted domain results are still retained by trusted steps().
                if !self.done.contains(node) {
                    self.values += 1;
                    self.bytes = self
                        .bytes
                        .checked_add(value.encode()?.len())
                        .ok_or_else(|| fail(Code::InfrastructureFailure, "graph-v2 byte limit"))?;
                    if self.values > MAX_ROOT_VALUES || self.bytes > MAX_ROOT_BYTES {
                        return Err(fail(
                            Code::InfrastructureFailure,
                            "graph-v2 root value/byte limit",
                        ));
                    }
                }
                self.deliver(graph, scope, node, None, true)?;
            }
        }
        Ok(matches!(outcome, Outcome::Domain(_)) && policy == DomainErrorPolicy::FailRoot)
    }

    fn input(&self, node: &str) -> Result<Resolution, Failure> {
        let definition = &self.nodes[node].1;
        if !matches!(definition.kind, NodeKind::Join { .. }) {
            let input = self.port_input(node, &definition.inputs[0].name)?;
            if matches!(input, Resolution::Value(None, _)) {
                return Err(fail(
                    Code::InfrastructureFailure,
                    "graph-v2 runnable input payload released",
                ));
            }
            return Ok(input);
        }
        let non_runnable = self.join_non_runnable(node)?;
        let mut fields = BTreeMap::new();
        let mut resources: Option<BTreeSet<String>> = None;
        let mut unresolved = false;
        let mut failed = false;
        for port in &definition.inputs {
            match self.port_input(node, &port.name)? {
                Resolution::Unresolved => unresolved = true,
                Resolution::Failed => failed = true,
                Resolution::Closed => {}
                Resolution::Value(Some(value), scope) => {
                    fields.insert(port.name.clone(), value);
                    if let Some(resources) = &mut resources {
                        resources.retain(|r| scope.contains(r));
                    } else {
                        resources = Some(scope);
                    }
                }
                Resolution::Value(None, _) if non_runnable => {}
                Resolution::Value(None, _) => {
                    return Err(fail(
                        Code::InfrastructureFailure,
                        "graph-v2 runnable Join payload released",
                    ))
                }
            }
        }
        if unresolved {
            Ok(Resolution::Unresolved)
        } else if failed {
            Ok(Resolution::Failed)
        } else if non_runnable {
            Ok(Resolution::Closed)
        } else {
            let resources = resources.ok_or_else(|| {
                fail(
                    Code::InfrastructureFailure,
                    "graph-v2 Join resources absent",
                )
            })?;
            Ok(Resolution::Value(Some(Value::Map(fields)), resources))
        }
    }

    // Terminal arrivals never change: once a required input closes, this remains
    // true even while other producers are pending. No upstream cancellation.
    fn join_non_runnable(&self, node: &str) -> Result<bool, Failure> {
        for port in &self.nodes[node].1.inputs {
            // Join inputs have at most one candidate. No candidate is dormant
            // closure, not a missing success payload or an unresolved producer.
            if self.edges.iter().filter(|(_, e)| matches!(&e.kind, EdgeKind::Flow { to, .. } if to.node == node && to.port == port.name))
                .all(|(id, _)| matches!(self.arrivals[id], Resolution::Closed)) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn release_join_payloads(&mut self, node: &str) -> Result<(), Failure> {
        if self.join_non_runnable(node)? {
            for (id, edge) in &self.edges {
                if matches!(&edge.kind, EdgeKind::Flow { to, .. } if to.node == node) {
                    if let Some(Resolution::Value(payload, _)) = self.arrivals.get_mut(id) {
                        *payload = None;
                    }
                }
            }
        }
        Ok(())
    }

    fn port_input(&self, node: &str, port: &str) -> Result<Resolution, Failure> {
        let mut unresolved = false;
        let mut failed = false;
        let mut values = Vec::new();
        for (edge, e) in &self.edges {
            if !matches!(&e.kind, EdgeKind::Flow { to, .. } if to.node == node && to.port == port) {
                continue;
            }
            match &self.arrivals[edge] {
                Resolution::Unresolved => unresolved = true,
                Resolution::Failed => failed = true,
                Resolution::Value(value, resources) => values.push((value, resources)),
                Resolution::Closed => {}
            }
        }
        // Publish nothing until every candidate is terminal, including failures.
        if unresolved {
            return Ok(Resolution::Unresolved);
        }
        if failed {
            return Ok(Resolution::Failed);
        }
        match values.as_slice() {
            [] => Ok(Resolution::Closed),
            [(value, resources)] => Ok(Resolution::Value((*value).clone(), (*resources).clone())),
            // Intrinsic graph-constraint violation, not a declared operation error.
            _ => Err(fail(
                Code::InfrastructureFailure,
                "graph-v2 Merge ExactlyOne cardinality violated",
            )),
        }
    }

    fn order_ready(&self, graph: &CompiledGraph, node: &str) -> bool {
        if matches!(&self.nodes[node].1.kind, NodeKind::Source { source } if source != &self.source)
        {
            return true;
        }
        graph.predecessors[node].iter().all(|predecessor| {
            !graph.source_reachability[predecessor].contains(&self.source)
                || self.done.contains(predecessor)
        })
    }

    fn route_intrinsic(
        &mut self,
        graph: &CompiledGraph,
        scope: &ScopedRequest,
        node: &NodeDecl,
        request: &Value,
        resources: BTreeSet<String>,
    ) -> Result<(), Failure> {
        self.resources.insert(node.id.clone(), resources);
        let (port, value) = match &node.kind {
            NodeKind::Switch { .. } => {
                if !node.inputs[0].schema.accepts(request) {
                    return Err(invalid("invalid Switch value"));
                }
                let Value::Map(cases) = request else {
                    unreachable!()
                };
                let (case, payload) = cases.iter().next().unwrap();
                let phenix_contract::ValueSchema::Tagged(schemas) = &node.inputs[0].schema else {
                    unreachable!()
                };
                let value = if schemas[case].is_none() {
                    Value::Map(BTreeMap::new())
                } else {
                    payload.clone()
                };
                (case.as_str(), value)
            }
            NodeKind::Merge { .. } | NodeKind::Join { .. } => ("out", request.clone()),
            _ => unreachable!(),
        };
        self.deliver(graph, scope, &node.id, Some((port, &value)), false)
    }
}

impl GraphExecution<'_> {
    pub(in crate::graph) fn pump_v2(
        &mut self,
        observe: &mut impl FnMut(&crate::graph::GraphUpdate<'_>),
    ) -> Result<(), Failure> {
        let order = self
            .root
            .kernel
            .prepared
            .graph_v2
            .as_ref()
            .unwrap()
            .order()
            .to_vec();
        for id in &order {
            let graph = self.root.kernel.prepared.graph_v2.as_ref().unwrap();
            if self.stopped {
                break;
            }
            let wave = self.v2.as_mut().unwrap();
            if wave.started.contains(id) || wave.done.contains(id) {
                continue;
            }
            let (owner, node) = wave.nodes[id].clone();
            if let NodeKind::Source { source } = &node.kind {
                if !wave.order_ready(graph, id) {
                    continue;
                }
                let selected = (source == &wave.source).then_some(("out", &self.payload));
                wave.deliver(graph, &self.root.scope, id, selected, false)?;
                self.waiting.remove(id);
                continue;
            }
            let input = wave.input(id)?;
            match input {
                Resolution::Unresolved => continue,
                Resolution::Closed | Resolution::Failed => {
                    wave.deliver(
                        graph,
                        &self.root.scope,
                        id,
                        None,
                        matches!(input, Resolution::Failed),
                    )?;
                    self.waiting.remove(id);
                    continue;
                }
                Resolution::Value(request, mut resources) => {
                    let request = request.ok_or_else(|| {
                        fail(
                            Code::InfrastructureFailure,
                            "graph-v2 runnable request payload released",
                        )
                    })?;
                    if !wave.order_ready(graph, id) {
                        continue;
                    }
                    if let NodeKind::Constant { value } = &node.kind {
                        wave.resources.insert(id.clone(), resources);
                        wave.deliver(graph, &self.root.scope, id, Some(("out", value)), false)?;
                        self.waiting.remove(id);
                        continue;
                    }
                    if matches!(
                        node.kind,
                        NodeKind::Switch { .. } | NodeKind::Merge { .. } | NodeKind::Join { .. }
                    ) {
                        wave.route_intrinsic(graph, &self.root.scope, &node, &request, resources)?;
                        self.waiting.remove(id);
                        continue;
                    }
                    let NodeKind::Invoke { operation, .. } = &node.kind else {
                        unreachable!()
                    };
                    let provider = &graph.routes[id];
                    if self
                        .root
                        .kernel
                        .state
                        .borrow()
                        .tickets
                        .values()
                        .any(|r| !r.quiescent && &r.selection.provider == provider)
                    {
                        continue;
                    }
                    resources.retain(|r| {
                        self.root.kernel.prepared.grants[&(owner.clone(), operation.clone())]
                            .contains(r)
                    });
                    wave.resources.insert(id.clone(), resources.clone());
                    wave.started.insert(id.clone());
                    self.waiting.remove(id);
                    let (interface, operation) = operation.split_once('/').unwrap();
                    let selection = Selection {
                        interface: interface.into(),
                        operation: operation.into(),
                        provider: provider.clone(),
                    };
                    let original = self.root.scope.authority.resources.clone();
                    self.root.scope.authority.resources = resources;
                    let result =
                        self.root
                            .call_graph(&owner, &selection, &request, self.pending_allowed);
                    self.root.scope.authority.resources = original;
                    match result {
                        Ok(Call::Ready(outcome)) => {
                            self.finish_node(id.clone(), Ok(outcome), observe)
                        }
                        Ok(Call::Pending(ticket)) => {
                            let ready = self.root.kernel.state.borrow().tickets[&ticket.id].logical
                                != crate::pending::Logical::Pending;
                            if ready {
                                self.accept_poll(id.clone(), ticket, observe)?;
                            } else {
                                self.pending.insert(id.clone(), ticket);
                            }
                        }
                        Err(failure) => self.finish_node(id.clone(), Err(failure), observe),
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;
