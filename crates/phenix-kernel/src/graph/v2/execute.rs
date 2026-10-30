//! Private finite initial-wave readiness. Effects and tickets remain in GraphExecution.
use super::*;
use crate::{graph::GraphExecution, resolve::Selection, runtime::Outcome, Call};
use phenix_contract::ScopedRequest;

const MAX_ROOT_VALUES: usize = 16_384;
const MAX_ROOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ROOT_NODES: usize = 4_096;
const MAX_ROOT_WAVES: u64 = 256;
const MAX_DEFERRED_QUEUE: usize = 1_024;

struct DeferredActivation {
    edge: String,
    value: Value,
    resources: BTreeSet<String>,
}

struct EachBranch {
    key: String,
    route: usize,
    request: Value,
    started: bool,
    outcome: Option<Value>,
}

struct EachBatch {
    branches: Vec<EachBranch>,
}

// Deterministic work count, not wall-clock timing.
#[cfg(test)]
thread_local! {
    static PORT_INPUT_EDGE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
enum Resolution {
    Unresolved,
    // None retains successful terminal bookkeeping only for a non-runnable consumer.
    Value(Option<Value>, BTreeSet<String>),
    Closed,
    Failed,
}

// Private scheduler diagnostics, never exposed as plugin payloads or authority.
#[derive(Clone, Debug, PartialEq)]
enum TerminalCause {
    InactiveSource {
        source: String,
    },
    Closed {
        node: String,
    },
    Domain {
        node: String,
        value: std::rc::Rc<Value>,
    },
}

#[derive(Clone)]
struct ClosureEvidence {
    // None is an unconstrained potential scope, never an execution capability.
    resources: Option<BTreeSet<String>>,
    authorized: bool,
    causal: bool,
}

impl ClosureEvidence {
    fn intersect(&mut self, other: &Self) {
        if let Some(other) = &other.resources {
            if let Some(resources) = &mut self.resources {
                resources.retain(|r| other.contains(r));
            } else {
                self.resources = Some(other.clone());
            }
        }
        self.authorized &= other.authorized;
        self.causal |= other.causal;
    }
}

pub(in crate::graph) struct Wave {
    pub(in crate::graph) id: u64,
    queue: std::collections::VecDeque<DeferredActivation>,
    emitted: BTreeMap<String, DeferredActivation>,
    executions: usize,
    source: String,
    nodes: BTreeMap<String, (String, NodeDecl)>,
    edges: BTreeMap<String, EdgeDecl>,
    arrivals: BTreeMap<String, Resolution>,
    // Keep diagnostic provenance separate from readiness and authority evidence.
    // Original incoming causes remain available after canonical propagation.
    terminal_causes: BTreeMap<String, TerminalCause>,
    domain_causes: BTreeMap<String, TerminalCause>,
    input_edges: BTreeMap<(String, String), Vec<String>>,
    started: BTreeSet<String>,
    done: BTreeSet<String>,
    resources: BTreeMap<String, BTreeSet<String>>,
    closure: BTreeMap<String, ClosureEvidence>,
    causes: BTreeMap<String, ClosureEvidence>,
    values: usize,
    bytes: usize,
    each: BTreeMap<String, EachBatch>,
    branch_keys: BTreeMap<String, (String, u32)>,
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
            .filter(|(_, e)| matches!(e.kind, EdgeKind::Flow { .. } | EdgeKind::Deferred { .. }))
            .map(|(id, e)| {
                (
                    id.clone(),
                    if matches!(e.kind, EdgeKind::Deferred { .. }) {
                        Resolution::Closed
                    } else {
                        Resolution::Unresolved
                    },
                )
            })
            .collect();
        Ok(Self {
            id: 0,
            queue: Default::default(),
            emitted: BTreeMap::new(),
            executions: 0,
            source: source.into(),
            nodes,
            edges,
            arrivals,
            terminal_causes: BTreeMap::new(),
            domain_causes: BTreeMap::new(),
            input_edges: graph.input_edges.clone(),
            started: BTreeSet::new(),
            done: BTreeSet::new(),
            resources: BTreeMap::new(),
            closure: BTreeMap::new(),
            causes: BTreeMap::new(),
            values: 0,
            bytes: 0,
            each: BTreeMap::new(),
            branch_keys: BTreeMap::new(),
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
        if !self.each.contains_key(node) {
            self.executions += 1;
        }
        if self.executions > MAX_ROOT_NODES {
            return Err(fail(
                Code::InfrastructureFailure,
                "graph-v2 root execution limit",
            ));
        }
        let terminal_cause = self
            .domain_causes
            .get(node)
            .cloned()
            .or_else(|| {
                if selected.is_some() {
                    return None;
                }
                self.edges.iter().find_map(|(id, edge)| {
                    if matches!(&edge.kind, EdgeKind::Flow { to, .. } if to.node == node)
                        && matches!(self.arrivals[id], Resolution::Failed) == failed
                        && matches!(self.arrivals[id], Resolution::Closed | Resolution::Failed)
                    {
                        self.terminal_causes.get(id).cloned()
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| match &self.nodes[node].1.kind {
                NodeKind::Source { source } if source != &self.source => {
                    TerminalCause::InactiveSource {
                        source: source.clone(),
                    }
                }
                _ => TerminalCause::Closed { node: node.into() },
            });
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
        let cause = self.resources.get(node).map(|resources| ClosureEvidence {
            resources: Some(resources.clone()),
            authorized: true,
            causal: true,
        }).or_else(|| self.causes.get(node).cloned()).or_else(|| {
            matches!(&self.nodes[node].1.kind, NodeKind::Source { source } if source == &self.source)
                .then(|| ClosureEvidence { resources: Some(scope.authority.resources.clone()), authorized: true, causal: true })
        });
        // Project even discarded deliveries: their terminal metadata may later
        // explain observable absence, but dropping a value is not an access.
        for (id, edge) in &self.edges {
            let EdgeKind::Flow { from, to } = &edge.kind else {
                continue;
            };
            if from.node != node {
                continue;
            }
            {
                // An inactive candidate constrains a later causal anchor but
                // cannot itself authorize execution or manufacture root rights.
                let mut evidence = cause.clone().unwrap_or(ClosureEvidence {
                    resources: None,
                    authorized: true,
                    causal: false,
                });
                let (owner, definition) = &self.nodes[node];
                let target_owner = &self.nodes[&to.node].0;
                if owner != target_owner
                    || graph.each_routes.contains_key(node)
                    || graph.each_routes.contains_key(&to.node)
                    || graph.routes.get(node).unwrap_or(owner)
                        != graph.routes.get(&to.node).unwrap_or(target_owner)
                {
                    if let Some(grant) = graph.policy.delivery.get(&(
                        from.node.clone(),
                        from.port.clone(),
                        to.node.clone(),
                        to.port.clone(),
                    )) {
                        if let Some(resources) = &mut evidence.resources {
                            resources.retain(|r| {
                                grant.resources.contains(r) && scope.authority.resources.contains(r)
                            });
                        } else {
                            evidence.resources = Some(
                                grant
                                    .resources
                                    .intersection(&scope.authority.resources)
                                    .cloned()
                                    .collect(),
                            );
                        }
                        let output = definition
                            .outputs
                            .iter()
                            .find(|p| p.name == from.port)
                            .unwrap();
                        evidence.authorized &= output.security.confidentiality
                            <= grant.maximum_classification
                            && evidence.resources.as_ref().is_some_and(|r| !r.is_empty());
                    } else {
                        evidence.resources = Some(BTreeSet::new());
                        evidence.authorized = false;
                    }
                }
                evidence.authorized &= evidence.resources.as_ref().is_none_or(|resources| {
                    scope
                        .resource
                        .as_ref()
                        .is_none_or(|r| resources.contains(r))
                });
                self.closure.insert(id.clone(), evidence);
            }
        }
        // Inspect all outputs of this completion atomically, without installing
        // released payloads on consumers that could still run.
        let discarded: BTreeSet<_> = self
            .edges
            .values()
            .filter_map(|edge| {
                let EdgeKind::Flow { from, to } = &edge.kind else {
                    return None;
                };
                (from.node == node
                    && self.non_runnable(&to.node, Some((node, selected.map(|(p, _)| p), failed))))
                .then(|| to.node.clone())
            })
            .collect();
        // Resolve sibling closure/failure and discarded successes before any
        // actual delivery. Edge ID order cannot affect readiness or access.
        for (id, edge) in &self.edges {
            let EdgeKind::Flow { from, to } = &edge.kind else {
                continue;
            };
            if from.node != node {
                continue;
            }
            if selected.is_none_or(|(port, _)| port != from.port) {
                self.terminal_causes
                    .insert(id.clone(), terminal_cause.clone());
                self.arrivals.insert(
                    id.clone(),
                    if failed {
                        Resolution::Failed
                    } else {
                        Resolution::Closed
                    },
                );
            } else if discarded.contains(&to.node) {
                let resources = cause
                    .as_ref()
                    .filter(|e| e.causal)
                    .and_then(|e| e.resources.as_ref())
                    .ok_or_else(|| denied("delivery without causal resource scope"))?;
                self.arrivals
                    .insert(id.clone(), Resolution::Value(None, resources.clone()));
            }
        }
        for consumer in &discarded {
            self.release_payloads(consumer)?;
        }
        let (owner, definition) = &self.nodes[node];
        let inherited = cause
            .as_ref()
            .filter(|e| e.causal)
            .and_then(|e| e.resources.as_ref());
        for (id, edge) in &self.edges {
            let (EdgeKind::Flow { from, to } | EdgeKind::Deferred { from, to, .. }) = &edge.kind
            else {
                continue;
            };
            if from.node != node {
                continue;
            }
            if let Some((port, value)) = selected.filter(|(port, _)| *port == from.port) {
                if matches!(edge.kind, EdgeKind::Flow { .. }) && discarded.contains(&to.node) {
                    continue;
                }
                let inherited =
                    inherited.ok_or_else(|| denied("delivery without causal resource scope"))?;
                let output = definition.outputs.iter().find(|p| p.name == port).unwrap();
                let (target_owner, _) = &self.nodes[&to.node];
                let mut resources = inherited.clone();
                if owner != target_owner
                    || graph.each_routes.contains_key(node)
                    || graph.each_routes.contains_key(&to.node)
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
                if matches!(edge.kind, EdgeKind::Deferred { .. }) {
                    if self.queue.len() + self.emitted.len() >= MAX_DEFERRED_QUEUE {
                        return Err(fail(
                            Code::InfrastructureFailure,
                            "graph-v2 deferred queue limit",
                        ));
                    }
                    self.emitted.insert(
                        id.clone(),
                        DeferredActivation {
                            edge: id.clone(),
                            value: value.clone(),
                            resources,
                        },
                    );
                } else {
                    self.arrivals.insert(
                        id.clone(),
                        Resolution::Value(Some(value.clone()), resources),
                    );
                }
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
        if self.completed(node) {
            return Ok(false);
        }
        if let Some((authored, ordinal)) = self.branch_keys.get(node) {
            let branch = &mut self.each.get_mut(authored).unwrap().branches[*ordinal as usize];
            if branch.outcome.is_none() {
                let value = match outcome {
                    Outcome::Success(value) | Outcome::Domain(value) => value,
                };
                self.values += 1;
                self.bytes = self
                    .bytes
                    .checked_add(value.encode()?.len())
                    .ok_or_else(|| {
                        fail(Code::InfrastructureFailure, "InvokeEach result byte limit")
                    })?;
                if self.values > MAX_ROOT_VALUES || self.bytes > MAX_ROOT_BYTES {
                    return Err(fail(Code::InfrastructureFailure, "InvokeEach result limit"));
                }
                let (tag, value) = match outcome {
                    Outcome::Success(value) => ("Ok", value),
                    Outcome::Domain(value) => ("Err", value),
                };
                branch.outcome = Some(Value::map([(tag, value.clone())]));
            }
            return Ok(false);
        }
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
                self.domain_causes.insert(
                    node.into(),
                    TerminalCause::Domain {
                        node: node.into(),
                        value: std::rc::Rc::new(value.clone()),
                    },
                );
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
        let candidates: Vec<_> = self
            .edges
            .iter()
            .filter(|(_, edge)| matches!(&edge.kind, EdgeKind::Flow { to, .. } | EdgeKind::Deferred { to, .. } if to.node == node))
            .map(|(id, _)| &self.arrivals[id])
            .collect();
        let non_runnable = self.join_non_runnable(node)?;
        if !non_runnable
            && candidates
                .iter()
                .any(|r| matches!(r, Resolution::Value(None, _)))
        {
            return Err(fail(
                Code::InfrastructureFailure,
                "graph-v2 runnable Join payload released",
            ));
        }
        if candidates
            .iter()
            .any(|r| matches!(r, Resolution::Unresolved))
        {
            return Ok(Resolution::Unresolved);
        }
        if candidates.iter().any(|r| matches!(r, Resolution::Failed)) {
            return Ok(Resolution::Failed);
        }
        if non_runnable {
            return Ok(Resolution::Closed);
        }
        let mut fields = BTreeMap::new();
        let mut resources: Option<BTreeSet<String>> = None;
        for port in &definition.inputs {
            let Resolution::Value(Some(value), scope) = self.port_input(node, &port.name)? else {
                return Err(fail(
                    Code::InfrastructureFailure,
                    "graph-v2 runnable Join input not a retained value",
                ));
            };
            fields.insert(port.name.clone(), value);
            if let Some(resources) = &mut resources {
                resources.retain(|r| scope.contains(r));
            } else {
                resources = Some(scope);
            }
        }
        let resources = resources.ok_or_else(|| {
            fail(
                Code::InfrastructureFailure,
                "graph-v2 Join resources absent",
            )
        })?;
        Ok(Resolution::Value(Some(Value::Map(fields)), resources))
    }

    pub(in crate::graph) fn branch_identity(&self, key: &str) -> Option<(&str, u32)> {
        self.branch_keys
            .get(key)
            .map(|(node, ordinal)| (node.as_str(), *ordinal))
    }

    pub(in crate::graph) fn completed(&self, key: &str) -> bool {
        if let Some((node, ordinal)) = self.branch_keys.get(key) {
            self.done.contains(node)
                || self
                    .each
                    .get(node)
                    .is_none_or(|batch| batch.branches[*ordinal as usize].outcome.is_some())
        } else {
            self.done.contains(key)
        }
    }

    fn admit_each(
        &mut self,
        graph: &CompiledGraph,
        scope: &ScopedRequest,
        node: &NodeDecl,
        request: &Value,
        mut resources: BTreeSet<String>,
    ) -> Result<(), Failure> {
        let NodeKind::InvokeEach { max, .. } = node.kind else {
            unreachable!()
        };
        let Value::List(items) = request else {
            return Err(invalid("InvokeEach input is not a list"));
        };
        if items.len() > max as usize || !node.inputs[0].schema.accepts(request) {
            return Err(fail(
                Code::InfrastructureFailure,
                "InvokeEach membership admission rejected",
            ));
        }
        let mut branches = Vec::new();
        let bytes = request.encode()?.len();
        if self.executions + items.len() + 1 > MAX_ROOT_NODES
            || self.values + items.len() > MAX_ROOT_VALUES
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|n| n > MAX_ROOT_BYTES)
        {
            return Err(fail(
                Code::InfrastructureFailure,
                "InvokeEach root admission limit",
            ));
        }
        for (ordinal, item) in items.iter().enumerate() {
            let Value::Map(fields) = item else {
                unreachable!()
            };
            let Value::U64(route) = fields["route"] else {
                unreachable!()
            };
            let route = usize::try_from(route)
                .ok()
                .filter(|route| *route < graph.each_routes[&node.id].len())
                .ok_or_else(|| denied("InvokeEach route outside prepared authorizations"))?;
            resources.retain(|resource| graph.each_routes[&node.id][route].1.contains(resource));
            branches.push(EachBranch {
                key: format!("$each:{}:{ordinal}", node.id),
                route,
                request: fields["input"].clone(),
                started: false,
                outcome: None,
            });
        }
        if resources.is_empty()
            || scope
                .resource
                .as_ref()
                .is_some_and(|resource| !resources.contains(resource))
        {
            return Err(denied(
                "InvokeEach collection outside common effective route scope",
            ));
        }
        self.executions += branches.len() + 1;
        self.values += branches.len();
        self.bytes += bytes;
        for (ordinal, branch) in branches.iter().enumerate() {
            self.branch_keys
                .insert(branch.key.clone(), (node.id.clone(), ordinal as u32));
        }
        self.resources.insert(node.id.clone(), resources);
        self.started.insert(node.id.clone());
        self.each.insert(node.id.clone(), EachBatch { branches });
        Ok(())
    }

    // Terminal arrivals never change: once a required input closes, this remains
    // true even while other producers are pending. No upstream cancellation.
    fn join_non_runnable(&self, node: &str) -> Result<bool, Failure> {
        Ok(self.non_runnable(node, None))
    }

    fn non_runnable(&self, node: &str, completion: Option<(&str, Option<&str>, bool)>) -> bool {
        for port in &self.nodes[node].1.inputs {
            let mut terminal = true;
            let mut all_closed = true;
            let mut count = 0;
            // The compiled input index avoids scanning unrelated graph edges
            // for every consumer readiness check.
            for id in self
                .input_edges
                .get(&(node.to_owned(), port.name.clone()))
                .into_iter()
                .flatten()
            {
                let Some(edge) = self.edges.get(id) else {
                    continue;
                };
                let (EdgeKind::Flow { from, .. } | EdgeKind::Deferred { from, .. }) = &edge.kind
                else {
                    continue;
                };
                if let Some((_, selected, failed)) = completion.filter(|(producer, _, _)| {
                    matches!(edge.kind, EdgeKind::Flow { .. }) && *producer == from.node
                }) {
                    let value = selected == Some(from.port.as_str());
                    count += usize::from(value);
                    all_closed &= !value && !failed;
                } else {
                    terminal &= !matches!(self.arrivals[id], Resolution::Unresolved);
                    all_closed &= matches!(self.arrivals[id], Resolution::Closed);
                    count += usize::from(matches!(self.arrivals[id], Resolution::Value(..)));
                }
            }
            let unsatisfied = match port.readiness {
                InputReadiness::One => all_closed,
                InputReadiness::Optional => false,
                InputReadiness::StaticMany { min, max } => {
                    count < min as usize || count > max as usize
                }
            };
            if terminal && unsatisfied {
                return true;
            }
        }
        false
    }

    fn release_payloads(&mut self, node: &str) -> Result<(), Failure> {
        if self.join_non_runnable(node)? {
            for (id, edge) in &self.edges {
                if matches!(&edge.kind, EdgeKind::Flow { to, .. } | EdgeKind::Deferred { to, .. } if to.node == node)
                {
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
        for edge in self
            .input_edges
            .get(&(node.into(), port.into()))
            .into_iter()
            .flatten()
        {
            #[cfg(test)]
            PORT_INPUT_EDGE_VISITS.with(|visits| visits.set(visits.get() + 1));
            if !self.edges.contains_key(edge) {
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
        let readiness = self.nodes[node]
            .1
            .inputs
            .iter()
            .find(|p| p.name == port)
            .unwrap()
            .readiness;
        if !matches!(readiness, InputReadiness::One) {
            if let InputReadiness::StaticMany { min, max } = readiness {
                if values.len() < min as usize || values.len() > max as usize {
                    return Ok(Resolution::Closed);
                }
            }
            if matches!(readiness, InputReadiness::Optional) && values.len() > 1 {
                return Err(invalid("multiple Optional candidates"));
            }
            let evidence = self
                .input_evidence(node)
                .filter(|e| e.causal)
                .ok_or_else(|| denied("absence without causal authority"))?;
            let resources = evidence
                .resources
                .ok_or_else(|| denied("absence without causal resource scope"))?;
            if !evidence.authorized {
                return Err(denied("absence outside effective flow scope"));
            }
            let mut payloads = Vec::new();
            for (value, _) in values {
                payloads.push(value.clone().ok_or_else(|| {
                    fail(
                        Code::InfrastructureFailure,
                        "graph-v2 runnable input payload released",
                    )
                })?);
            }
            let value = if matches!(readiness, InputReadiness::Optional) && payloads.is_empty() {
                Value::Null
            } else {
                Value::List(payloads)
            };
            return Ok(Resolution::Value(Some(value), resources));
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

    fn input_evidence(&self, node: &str) -> Option<ClosureEvidence> {
        let mut result: Option<ClosureEvidence> = None;
        for (id, edge) in &self.edges {
            if !matches!(&edge.kind, EdgeKind::Flow { to, .. } | EdgeKind::Deferred { to, .. } if to.node == node)
            {
                continue;
            }
            let evidence = self
                .closure
                .get(id)
                .cloned()
                .or_else(|| match &self.arrivals[id] {
                    Resolution::Value(_, resources) => Some(ClosureEvidence {
                        resources: Some(resources.clone()),
                        authorized: true,
                        causal: true,
                    }),
                    _ => None,
                });
            if let Some(evidence) = evidence {
                if let Some(result) = &mut result {
                    result.intersect(&evidence);
                } else {
                    result = Some(evidence);
                }
            }
        }
        result
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
    fn pump_each(
        &mut self,
        observe: &mut impl FnMut(&crate::graph::GraphUpdate<'_>),
    ) -> Result<(), Failure> {
        let nodes: Vec<_> = self.v2.as_ref().unwrap().each.keys().cloned().collect();
        for id in nodes {
            let count = self.v2.as_ref().unwrap().each[&id].branches.len();
            for ordinal in 0..count {
                if self.stopped {
                    return Ok(());
                }
                let wave = self.v2.as_mut().unwrap();
                let branch = &mut wave.each.get_mut(&id).unwrap().branches[ordinal];
                if branch.started {
                    continue;
                }
                let graph = self.root.kernel.prepared.graph_v2.as_ref().unwrap();
                let (provider, grants) = &graph.each_routes[&id][branch.route];
                if self
                    .root
                    .kernel
                    .state
                    .borrow()
                    .tickets
                    .values()
                    .any(|ticket| !ticket.quiescent && &ticket.selection.provider == provider)
                {
                    continue;
                }
                let (owner, node) = &wave.nodes[&id];
                let NodeKind::InvokeEach { operation, .. } = &node.kind else {
                    unreachable!()
                };
                let (interface, operation) = operation.split_once('/').unwrap();
                let selection = Selection {
                    interface: interface.into(),
                    operation: operation.into(),
                    provider: provider.clone(),
                };
                let resources = wave.resources[&id].intersection(grants).cloned().collect();
                let key = branch.key.clone();
                branch.started = true;
                let original =
                    std::mem::replace(&mut self.root.scope.authority.resources, resources);
                let result =
                    self.root
                        .call_graph(owner, &selection, &branch.request, self.pending_allowed);
                self.root.scope.authority.resources = original;
                match result {
                    Ok(Call::Ready(outcome)) => self.finish_node(key, Ok(outcome), observe),
                    Ok(Call::Pending(ticket)) => {
                        let ready = self.root.kernel.state.borrow().tickets[&ticket.id].logical
                            != crate::pending::Logical::Pending;
                        if ready {
                            self.accept_poll(key, ticket, observe)?;
                        } else {
                            self.emit(
                                crate::graph::GraphEvent::NodePending { node: &key },
                                observe,
                            );
                            self.pending.insert(key, ticket);
                        }
                    }
                    Err(failure) => self.finish_node(key, Err(failure), observe),
                }
            }
            if self.stopped {
                return Ok(());
            }
            let wave = self.v2.as_mut().unwrap();
            let batch = &wave.each[&id];
            if self.draining.is_empty()
                && batch.branches.iter().all(|branch| branch.outcome.is_some())
            {
                let value = Value::List(
                    batch
                        .branches
                        .iter()
                        .map(|branch| branch.outcome.clone().unwrap())
                        .collect(),
                );
                let graph = self.root.kernel.prepared.graph_v2.as_ref().unwrap();
                wave.deliver(graph, &self.root.scope, &id, Some(("out", &value)), false)?;
                wave.each.remove(&id);
                self.waiting.remove(&id);
            }
        }
        Ok(())
    }
    pub(in crate::graph) fn advance_v2(&mut self) -> Result<(), Failure> {
        if self.stopped
            || !self.waiting.is_empty()
            || !self.pending.is_empty()
            || !self.draining.is_empty()
        {
            return Ok(());
        }
        let wave = self.v2.as_mut().unwrap();
        wave.queue
            .extend(std::mem::take(&mut wave.emitted).into_values());
        let Some(activation) = wave.queue.pop_front() else {
            return Ok(());
        };
        if wave.id + 1 >= MAX_ROOT_WAVES {
            return Err(fail(
                Code::InfrastructureFailure,
                "graph-v2 root wave limit",
            ));
        }
        let graph = self.root.kernel.prepared.graph_v2.as_ref().unwrap();
        let mut next = Wave::new(graph, &format!("$deferred:{}", activation.edge))?;
        next.id = wave.id + 1;
        next.values = wave.values;
        next.bytes = wave.bytes;
        next.executions = wave.executions;
        next.queue = std::mem::take(&mut wave.queue);
        next.arrivals.insert(
            activation.edge,
            Resolution::Value(Some(activation.value), activation.resources),
        );
        self.waiting = graph.order.iter().cloned().collect();
        self.v2 = Some(next);
        Ok(())
    }

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
            if !graph.source_reachability[id].contains(&wave.source) {
                wave.deliver(graph, &self.root.scope, id, None, false)?;
                self.waiting.remove(id);
                continue;
            }
            let input = wave.input(id)?;
            match input {
                Resolution::Unresolved => continue,
                Resolution::Closed | Resolution::Failed => {
                    if let Some(evidence) = wave.input_evidence(id) {
                        wave.causes.insert(id.clone(), evidence);
                    }
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
                    if matches!(node.kind, NodeKind::InvokeEach { .. }) {
                        wave.admit_each(graph, &self.root.scope, &node, &request, resources)?;
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
                                self.emit(
                                    crate::graph::GraphEvent::NodePending { node: id },
                                    observe,
                                );
                                self.pending.insert(id.clone(), ticket);
                            }
                        }
                        Err(failure) => self.finish_node(id.clone(), Err(failure), observe),
                    }
                }
            }
        }
        self.pump_each(observe)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;

#[cfg(test)]
mod each_tests;
