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
    Value(Value, BTreeSet<String>),
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
            let resolution =
                if let Some((port, value)) = selected.filter(|(port, _)| *port == from.port) {
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
                    Resolution::Value(value.clone(), resources)
                } else if failed {
                    Resolution::Failed
                } else {
                    Resolution::Closed
                };
            self.arrivals.insert(id.clone(), resolution);
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

    fn input(&self, node: &str) -> Resolution {
        self.edges
            .iter()
            .find_map(|(edge, e)| match &e.kind {
                EdgeKind::Flow { to, .. } if to.node == node => Some(self.arrivals[edge].clone()),
                _ => None,
            })
            .unwrap_or(Resolution::Closed)
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
            let input = wave.input(id);
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
                    if !wave.order_ready(graph, id) {
                        continue;
                    }
                    if let NodeKind::Constant { value } = &node.kind {
                        wave.resources.insert(id.clone(), resources);
                        wave.deliver(graph, &self.root.scope, id, Some(("out", value)), false)?;
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
