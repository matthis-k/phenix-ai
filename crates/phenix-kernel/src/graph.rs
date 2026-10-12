//! Offline graph composition and serial action execution through the dispatcher.
//! The graph uses the shared DAG ordering algorithm but owns graph semantics.
use crate::{
    dag,
    resolve::Selection,
    runtime::{Kernel, Outcome, Root},
    Call, Poll, Ticket,
};
use phenix_contract::{
    graph_node_key, ActionDefinition, Code, EventDefinition, Failure, GraphContribution, GraphNode,
    ScopedRequest, Value, ValueSchema,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInfo {
    pub id: String,
    pub event: String,
    pub description: String,
    pub payload: ValueSchema,
    pub required_operations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CompiledNode {
    pub owner: String,
    pub provider: String,
    pub definition: GraphNode,
}

#[derive(Debug, Clone)]
pub struct CompiledGraph {
    /// SHA-256 of sorted canonical contributions, domain separated from generations.
    pub identity: String,
    pub nodes: BTreeMap<String, CompiledNode>,
    pub events: BTreeMap<String, EventDefinition>,
    pub actions: BTreeMap<String, ActionInfo>,
    /// Static predecessor constraints; NOT an activation list.
    pub predecessors: BTreeMap<String, BTreeSet<String>>,
    pub order: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}

fn ancestor(start: &str, sought: &str, predecessors: &BTreeMap<String, BTreeSet<String>>) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        for predecessor in &predecessors[node] {
            if predecessor == sought {
                return true;
            }
            pending.push(predecessor);
        }
    }
    false
}

impl CompiledGraph {
    /// Compile declarations without activating a plugin.
    ///
    /// The trusted resolver supplies a target lookup. It MUST authenticate the
    /// declaring plugin and selected slot against prepared bindings and grants.
    /// Keeping the lookup separate makes offline rejection independently testable.
    pub fn compile(
        declarations: &[GraphContribution],
        mut resolve: impl FnMut(&str, &str, &str) -> Result<String, Failure>,
    ) -> Result<Self, Failure> {
        let mut normalized = declarations
            .iter()
            .map(GraphContribution::normalized)
            .collect::<Result<Vec<_>, _>>()?;
        normalized.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        if normalized
            .windows(2)
            .any(|pair| pair[0].plugin == pair[1].plugin)
        {
            return Err(invalid("duplicate contributing plugin"));
        }
        let mut events = BTreeMap::new();
        let mut action_defs = BTreeMap::<String, ActionDefinition>::new();
        let mut nodes = BTreeMap::new();
        let mut predecessors = BTreeMap::new();

        for declaration in &normalized {
            for event in &declaration.events {
                if events.insert(event.id.clone(), event.clone()).is_some() {
                    return Err(invalid(format!("duplicate event {}", event.id)));
                }
            }
            for action in &declaration.actions {
                if action_defs
                    .insert(action.id.clone(), action.clone())
                    .is_some()
                {
                    return Err(invalid(format!("duplicate action {}", action.id)));
                }
            }
            for node in &declaration.nodes {
                let id = graph_node_key(&declaration.plugin, &node.id);
                let provider = resolve(&declaration.plugin, &node.selector, &node.operation)?;
                if provider.is_empty() {
                    return Err(invalid(format!("unresolved graph operation {id}")));
                }
                predecessors.insert(id.clone(), node.after.iter().cloned().collect());
                nodes.insert(
                    id,
                    CompiledNode {
                        owner: declaration.plugin.clone(),
                        provider,
                        definition: node.clone(),
                    },
                );
            }
        }

        let mut actions = BTreeMap::new();
        for (id, definition) in action_defs {
            let event = events.get(&definition.event).ok_or_else(|| {
                invalid(format!(
                    "action {id} references absent event {}",
                    definition.event
                ))
            })?;
            actions.insert(
                id.clone(),
                ActionInfo {
                    id,
                    event: definition.event,
                    description: definition.description,
                    payload: event.payload.clone(),
                    required_operations: definition.requires,
                },
            );
        }
        for (id, node) in &nodes {
            for trigger in &node.definition.triggers {
                if !events.contains_key(trigger) {
                    return Err(invalid(format!("node {id} has absent trigger {trigger}")));
                }
            }
        }
        for (id, dependencies) in &predecessors {
            for dependency in dependencies {
                if !nodes.contains_key(dependency) {
                    return Err(invalid(format!(
                        "node {id} has missing anchor {dependency}"
                    )));
                }
            }
        }
        let order = dag::order(&predecessors).map_err(|cycle| {
            invalid(format!("reactive dependency cycle: {}", cycle.join(" -> ")))
        })?;

        // Multiple writers for a cell are valid only when graph order fixes
        // their relative order. A shared event may activate unrelated roots.
        let mut writers = BTreeMap::<String, Vec<String>>::new();
        for (id, node) in &nodes {
            for write in &node.definition.writes {
                writers.entry(write.clone()).or_default().push(id.clone());
            }
        }
        for (property, ids) in writers {
            for (index, left) in ids.iter().enumerate() {
                for right in ids.iter().skip(index + 1) {
                    if !ancestor(left, right, &predecessors)
                        && !ancestor(right, left, &predecessors)
                    {
                        return Err(invalid(format!(
                            "unordered writers for {property}: {left}, {right}"
                        )));
                    }
                }
            }
        }

        let canonical = normalized
            .iter()
            .map(|definition| Ok(Value::Bytes(definition.encode()?)))
            .collect::<Result<Vec<_>, Failure>>()?;
        let mut bytes = b"phenix.graph.identity.v1\0".to_vec();
        bytes.extend(Value::List(canonical).encode()?);
        let identity = phenix_artifact::digest(&bytes);
        Ok(Self {
            identity,
            nodes,
            events,
            actions,
            predecessors,
            order,
        })
    }

    /// G3 preparation: derive the active projection for one admitted event set.
    /// Declared ordering edges do not activate successors or dormant ancestors.
    /// Runtime execution is deliberately outside this method.
    pub fn activation_order(&self, event_ids: &BTreeSet<String>) -> Result<Vec<String>, Failure> {
        for id in event_ids {
            if !self.events.contains_key(id) {
                return Err(invalid(format!("unknown admitted event {id}")));
            }
        }
        let active: BTreeSet<_> = self
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.definition
                    .triggers
                    .iter()
                    .any(|id| event_ids.contains(id))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let projection = active
            .iter()
            .map(|id| {
                let dependencies = self.predecessors[id]
                    .intersection(&active)
                    .cloned()
                    .collect();
                (id.clone(), dependencies)
            })
            .collect();
        dag::order(&projection).map_err(|cycle| {
            invalid(format!(
                "invalid active graph projection: {}",
                cycle.join(" -> ")
            ))
        })
    }

    pub fn discover(&self) -> impl Iterator<Item = &ActionInfo> {
        self.actions.values()
    }
}

impl crate::resolve::Prepared {
    /// Only the established M1 resolver may turn a declaration into a
    /// dispatchable target. The graph compiler never grants permissions.
    pub fn compile_graph(
        &self,
        declarations: &[GraphContribution],
    ) -> Result<CompiledGraph, Failure> {
        let graph = CompiledGraph::compile(declarations, |caller, selector, key| {
            let provider = if selector.is_empty() {
                caller.to_owned()
            } else {
                let identity = phenix_contract::import_identity(selector, key);
                self.bindings
                    .get(&(caller.to_owned(), identity))
                    .cloned()
                    .ok_or_else(|| Failure::new(Code::MissingBinding, "unselected graph import"))?
            };
            if !self
                .operations
                .contains_key(&(provider.clone(), key.to_owned()))
            {
                return Err(Failure::new(Code::MissingBinding, "graph operation absent"));
            }
            if !self.permits(caller, key)
                || !self
                    .grants
                    .contains_key(&(caller.to_owned(), key.to_owned()))
            {
                return Err(Failure::new(
                    Code::Denied,
                    "graph operation lacks prepared grant",
                ));
            }
            Ok(provider)
        })?;
        for (id, node) in &graph.nodes {
            let operation =
                &self.operations[&(node.provider.clone(), node.definition.operation.clone())];
            for event in &node.definition.triggers {
                if operation.schema.request != graph.events[event].payload {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        format!("node {id} request schema does not match event {event}"),
                    ));
                }
            }
            for write in &node.definition.writes {
                let (owner, property) = write.split_once('/').expect("validated property key");
                let target = self
                    .packages
                    .get(owner)
                    .and_then(|package| {
                        package
                            .metadata()
                            .properties
                            .iter()
                            .find(|field| field.id == property)
                    })
                    .ok_or_else(|| {
                        Failure::new(
                            Code::MissingBinding,
                            format!("graph property {write} not selected"),
                        )
                    })?;
                if !target.set {
                    return Err(Failure::new(
                        Code::Denied,
                        format!("graph property {write} is read-only"),
                    ));
                }
                let key = format!(
                    "{}/set",
                    phenix_contract::property_contract(owner, property)?
                );
                if !self.permits(&node.owner, &key)
                    || !self.grants.contains_key(&(node.owner.clone(), key))
                {
                    return Err(Failure::new(
                        Code::Denied,
                        format!("graph property {write} has no write grant"),
                    ));
                }
            }
        }
        Ok(graph)
    }
}

/// One completed serial graph operation, identified by its contributing node.
#[derive(Debug, PartialEq, Eq)]
pub struct GraphStep {
    pub node: String,
    pub outcome: Outcome,
}

/// Borrowed serial execution updates for a trusted host observer.
/// Delivery retains the admitted root; buffering and replay belong to consumers.
#[derive(Debug)]
pub struct GraphUpdate<'a> {
    pub root: u64,
    pub event: GraphEvent<'a>,
}

#[derive(Debug)]
pub enum GraphEvent<'a> {
    Admitted {
        action: &'a str,
        event: &'a str,
        graph: &'a str,
        resource: Option<&'a str>,
    },
    NodeFinished(&'a GraphStep),
    NodePending {
        node: &'a str,
    },
    NodeSkipped {
        node: &'a str,
    },
    Finished(GraphCompletion<'a>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum GraphCompletion<'a> {
    Success,
    /// At least one domain failure, including a handled skip.
    Domain,
    Cancelled,
    Failure(&'a Failure),
}

impl Kernel {
    /// Trusted control-plane action admission. Frontends submit the discovered
    /// action rather than invoking a graph handler directly. Synchronous nodes
    /// execute in active topological order. Pending and state triggers are G4.
    pub fn submit_action(
        &self,
        action_id: &str,
        payload: &Value,
    ) -> Result<Vec<GraphStep>, Failure> {
        self.submit_action_scoped(
            action_id,
            payload,
            ScopedRequest {
                authority: self.prepared.authority.clone(),
                resource: self.prepared.resource.clone(),
            },
        )
    }

    /// Admit a graph action using the caller's restricted authority and resource.
    /// Admission and all child effects share the existing serial root.
    pub fn submit_action_scoped(
        &self,
        action_id: &str,
        payload: &Value,
        scope: ScopedRequest,
    ) -> Result<Vec<GraphStep>, Failure> {
        self.submit_action_observed(action_id, payload, scope, |_| {})
    }

    /// Observe this serial action while its root remains admitted. Kernel
    /// failures stop execution even when the affected node declares Skip.
    pub fn submit_action_observed(
        &self,
        action_id: &str,
        payload: &Value,
        scope: ScopedRequest,
        mut observe: impl FnMut(&GraphUpdate<'_>),
    ) -> Result<Vec<GraphStep>, Failure> {
        let mut execution = self.begin_action_inner(action_id, payload, scope, false)?;
        execution.pump(&mut observe)?;
        if let Some(failure) = execution.failure.take() {
            return Err(failure);
        }
        Ok(std::mem::take(&mut execution.completed))
    }

    /// Admit pending-capable graph work. The host drives this same root with pump.
    pub fn begin_action_scoped(
        &self,
        action_id: &str,
        payload: &Value,
        scope: ScopedRequest,
    ) -> Result<GraphExecution<'_>, Failure> {
        self.begin_action_inner(action_id, payload, scope, true)
    }

    fn begin_action_inner(
        &self,
        action_id: &str,
        payload: &Value,
        mut scope: ScopedRequest,
        pending_allowed: bool,
    ) -> Result<GraphExecution<'_>, Failure> {
        self.check_thread()?;
        let graph = self
            .prepared
            .graph
            .as_ref()
            .ok_or_else(|| Failure::new(Code::MissingBinding, "no prepared graph"))?;
        let action = graph
            .actions
            .get(action_id)
            .ok_or_else(|| Failure::new(Code::MissingBinding, "unknown action"))?;
        if !action.payload.accepts(payload) {
            return Err(Failure::new(
                Code::SchemaRejected,
                "action payload does not match event schema",
            ));
        }
        if !action.required_operations.iter().all(|key| {
            self.prepared.authority.operations.contains(key)
                && scope.authority.operations.contains(key)
                && self
                    .prepared
                    .grants
                    .contains_key(&("root".into(), key.clone()))
        }) {
            return Err(Failure::new(
                Code::Denied,
                "root has no grant for action requirements",
            ));
        }

        // Action requirements are root operations. Their resource grants must
        // also bound delegation to graph owners, just as direct root dispatch.
        scope.authority.resources.retain(|resource| {
            action
                .required_operations
                .iter()
                .all(|key| self.prepared.grants[&("root".into(), key.clone())].contains(resource))
        });
        if scope
            .resource
            .as_ref()
            .is_some_and(|resource| !scope.authority.resources.contains(resource))
        {
            return Err(Failure::new(Code::Denied, "action resource is not granted"));
        }

        let active = graph.activation_order(&BTreeSet::from([action.event.clone()]))?;
        let root = self.admit_scoped_root(self.prepared.entry.clone(), scope)?;
        Ok(GraphExecution {
            root,
            action: action_id.into(),
            payload: payload.clone(),
            waiting: active.iter().cloned().collect(),
            active,
            pending: BTreeMap::new(),
            draining: Vec::new(),
            unsuccessful: BTreeSet::new(),
            completed: Vec::new(),
            failure: None,
            cancelled: false,
            stopped: false,
            admitted: false,
            terminal: false,
            pending_allowed,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum GraphStatus {
    Pending,
    Draining,
    Finished,
}

/// Readiness bookkeeping around one existing Root and its Ticket handles.
/// The host controls polling; no timeout or busy loop is imposed by the kernel.
pub struct GraphExecution<'a> {
    root: Root<'a>,
    action: String,
    payload: Value,
    active: Vec<String>,
    waiting: BTreeSet<String>,
    pending: BTreeMap<String, Ticket>,
    draining: Vec<Ticket>,
    unsuccessful: BTreeSet<String>,
    completed: Vec<GraphStep>,
    failure: Option<Failure>,
    cancelled: bool,
    stopped: bool,
    admitted: bool,
    terminal: bool,
    pending_allowed: bool,
}

impl GraphExecution<'_> {
    /// Trusted same-root control, using the existing selected-operation API.
    pub fn invoke_selected(
        &self,
        selection: &Selection,
        request: &Value,
    ) -> Result<Outcome, Failure> {
        self.root.invoke_selected(selection, request)
    }
    pub fn steps(&self) -> &[GraphStep] {
        &self.completed
    }
    pub fn completion(&self) -> Option<GraphCompletion<'_>> {
        self.terminal.then_some({
            if let Some(failure) = &self.failure {
                GraphCompletion::Failure(failure)
            } else if self.cancelled {
                GraphCompletion::Cancelled
            } else if self.unsuccessful.is_empty() {
                GraphCompletion::Success
            } else {
                GraphCompletion::Domain
            }
        })
    }
    fn emit(&self, event: GraphEvent<'_>, observe: &mut impl FnMut(&GraphUpdate<'_>)) {
        observe(&GraphUpdate {
            root: self.root.id,
            event,
        });
    }
    fn finish_node(
        &mut self,
        node: String,
        result: Result<Outcome, Failure>,
        observe: &mut impl FnMut(&GraphUpdate<'_>),
    ) {
        match result {
            Ok(outcome) => {
                if matches!(outcome, Outcome::Domain(_)) {
                    self.unsuccessful.insert(node.clone());
                    if self.root.kernel.prepared.graph.as_ref().unwrap().nodes[&node]
                        .definition
                        .failure
                        == phenix_contract::FailurePolicy::FailRoot
                    {
                        self.stopped = true;
                    }
                }
                self.completed.push(GraphStep { node, outcome });
                self.emit(
                    GraphEvent::NodeFinished(self.completed.last().unwrap()),
                    observe,
                );
            }
            Err(failure) => {
                self.failure = Some(failure);
                self.stopped = true;
            }
        }
    }
    fn quiescent(&self, ticket: &Ticket) -> bool {
        self.root.kernel.state.borrow().tickets[&ticket.id].quiescent
    }
    fn accept_poll(
        &mut self,
        node: String,
        ticket: Ticket,
        observe: &mut impl FnMut(&GraphUpdate<'_>),
    ) -> Result<(), Failure> {
        match self.root.poll(&ticket)? {
            Poll::Pending => {
                self.pending.insert(node, ticket);
            }
            Poll::Ready(result) => {
                if self.quiescent(&ticket) {
                    self.root.release(&ticket)?;
                } else {
                    self.draining.push(ticket);
                }
                self.finish_node(node, result, observe);
            }
            Poll::Cancelled { quiescent } => {
                if quiescent {
                    self.root.release(&ticket)?;
                } else {
                    self.draining.push(ticket);
                }
                self.cancelled = true;
                self.stopped = true;
            }
            Poll::Draining => {
                self.draining.push(ticket);
            }
        }
        Ok(())
    }
    fn cancel_pending(&mut self) -> Result<(), Failure> {
        for (_, ticket) in std::mem::take(&mut self.pending) {
            if !self.quiescent(&ticket) {
                self.root.cancel(&ticket)?;
            }
            if self.quiescent(&ticket) {
                self.root.release(&ticket)?;
            } else {
                self.draining.push(ticket);
            }
        }
        Ok(())
    }
    /// Revoke future graph work and cancel the existing ticket trees immediately.
    pub fn cancel(&mut self) -> Result<(), Failure> {
        if !self.terminal {
            self.cancelled = true;
            self.stopped = true;
            self.cancel_pending()?;
        }
        Ok(())
    }
    pub fn pump(
        &mut self,
        mut observe: impl FnMut(&GraphUpdate<'_>),
    ) -> Result<GraphStatus, Failure> {
        self.root.kernel.check_thread()?;
        if !self.admitted {
            let graph = self.root.kernel.prepared.graph.as_ref().unwrap();
            let action = &graph.actions[&self.action];
            self.emit(
                GraphEvent::Admitted {
                    action: &action.id,
                    event: &action.event,
                    graph: &graph.identity,
                    resource: self.root.scope.resource.as_deref(),
                },
                &mut observe,
            );
            self.admitted = true;
        }
        // Poll each retained physical lease once; cleanup cannot admit nodes.
        for ticket in std::mem::take(&mut self.draining) {
            if !self.quiescent(&ticket) {
                let _ = self.root.poll(&ticket)?;
            }
            if self.quiescent(&ticket) {
                self.root.release(&ticket)?;
            } else {
                self.draining.push(ticket);
            }
        }
        if !self.stopped {
            for node in self.pending.keys().cloned().collect::<Vec<_>>() {
                let ticket = self.pending.remove(&node).unwrap();
                self.accept_poll(node, ticket, &mut observe)?;
                if self.stopped {
                    break;
                }
            }
            for node in self.active.clone() {
                if self.stopped {
                    break;
                }
                if !self.waiting.contains(&node) {
                    continue;
                }
                let graph = self.root.kernel.prepared.graph.as_ref().unwrap();
                let predecessors = &graph.predecessors[&node];
                if predecessors.iter().any(|id| self.unsuccessful.contains(id)) {
                    self.waiting.remove(&node);
                    self.unsuccessful.insert(node.clone());
                    self.emit(GraphEvent::NodeSkipped { node: &node }, &mut observe);
                    continue;
                }
                if predecessors
                    .iter()
                    .any(|id| self.waiting.contains(id) || self.pending.contains_key(id))
                {
                    continue;
                }
                let compiled = &graph.nodes[&node];
                if self
                    .root
                    .kernel
                    .state
                    .borrow()
                    .tickets
                    .values()
                    .any(|r| !r.quiescent && r.selection.provider == compiled.provider)
                {
                    continue;
                }
                let (interface, operation) = compiled.definition.operation.split_once('/').unwrap();
                let target = Selection {
                    interface: interface.into(),
                    operation: operation.into(),
                    provider: compiled.provider.clone(),
                };
                let owner = compiled.owner.clone();
                self.waiting.remove(&node);
                match self
                    .root
                    .call_graph(&owner, &target, &self.payload, self.pending_allowed)
                {
                    Ok(Call::Ready(outcome)) => self.finish_node(node, Ok(outcome), &mut observe),
                    Ok(Call::Pending(ticket)) => {
                        let ready = self.root.kernel.state.borrow().tickets[&ticket.id].logical
                            != crate::pending::Logical::Pending;
                        if ready {
                            self.accept_poll(node, ticket, &mut observe)?;
                        } else {
                            self.emit(GraphEvent::NodePending { node: &node }, &mut observe);
                            self.pending.insert(node, ticket);
                        }
                    }
                    Err(failure) => self.finish_node(node, Err(failure), &mut observe),
                }
            }
        }
        if self.stopped {
            self.cancel_pending()?;
        }
        if !self.terminal && (self.stopped || (self.waiting.is_empty() && self.pending.is_empty()))
        {
            self.terminal = true;
            self.emit(
                GraphEvent::Finished(self.completion().unwrap()),
                &mut observe,
            );
        }
        Ok(if self.terminal {
            if self.draining.is_empty() {
                GraphStatus::Finished
            } else {
                GraphStatus::Draining
            }
        } else {
            GraphStatus::Pending
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_contract::{FailurePolicy, JoinPolicy};

    fn node(id: &str, trigger: &[&str], after: &[&str], writes: &[&str]) -> GraphNode {
        GraphNode {
            id: id.into(),
            operation: format!("examples.graph@1/{id}"),
            selector: "source".into(),
            triggers: trigger.iter().map(|v| (*v).into()).collect(),
            after: after.iter().map(|v| (*v).into()).collect(),
            writes: writes.iter().map(|v| (*v).into()).collect(),
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        }
    }
    fn contribution(owner: &str, events: bool, nodes: Vec<GraphNode>) -> GraphContribution {
        GraphContribution {
            plugin: owner.into(),
            events: if events {
                vec![EventDefinition {
                    id: "event.prompt".into(),
                    payload: ValueSchema::String,
                }]
            } else {
                vec![]
            },
            actions: if events {
                vec![ActionDefinition {
                    id: "submit".into(),
                    event: "event.prompt".into(),
                    description: "Submit".into(),
                    requires: vec!["agent.input@1/submit".into()],
                }]
            } else {
                vec![]
            },
            nodes,
        }
    }
    fn compile(input: &[GraphContribution]) -> Result<CompiledGraph, Failure> {
        CompiledGraph::compile(input, |owner, selector, key| {
            if selector == "source" && owner != "forbidden" && key.starts_with("examples.graph@1/")
            {
                Ok("provider".into())
            } else {
                Err(Failure::new(
                    Code::Denied,
                    "graph target has no selected grant",
                ))
            }
        })
    }
    fn fixtures() -> Vec<GraphContribution> {
        vec![
            contribution("alpha", true, vec![node("a", &["event.prompt"], &[], &[])]),
            contribution("beta", false, vec![node("b", &["event.prompt"], &[], &[])]),
            contribution(
                "gamma",
                false,
                vec![node("c", &["event.prompt"], &["alpha/a", "beta/b"], &[])],
            ),
            contribution("delta", false, vec![node("d", &[], &["beta/b"], &[])]),
        ]
    }
    #[test]
    fn independently_contributed_multi_root_graph_is_permutation_invariant() {
        let nodes = fixtures();
        let graph = compile(&nodes).unwrap();
        let mut reverse = nodes;
        reverse.reverse();
        let other = compile(&reverse).unwrap();
        assert_eq!(graph.identity, other.identity);
        assert_eq!(graph.order, other.order);
        assert_eq!(graph.order, ["alpha/a", "beta/b", "delta/d", "gamma/c"]);
        assert_eq!(graph.predecessors["gamma/c"].len(), 2);
        assert_eq!(
            graph.discover().next().unwrap().payload,
            ValueSchema::String
        );
    }
    #[test]
    fn malformed_anchors_cycles_triggers_and_conflicts_fail_offline() {
        let mut bad = fixtures();
        bad[0].nodes[0].after.push("absent/anchor".into());
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[0].nodes[0].after.push("gamma/c".into());
        assert!(compile(&bad).unwrap_err().message.contains("cycle"));
        let mut bad = fixtures();
        bad[0].nodes[0].triggers.push("unknown".into());
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[0].nodes[0].writes = vec!["owner/cell".into()];
        bad[1].nodes[0].writes = vec!["owner/cell".into()];
        assert!(compile(&bad)
            .unwrap_err()
            .message
            .contains("unordered writers"));
    }
    #[test]
    fn duplicate_event_action_node_and_denied_target_fail() {
        let mut bad = fixtures();
        bad[1].events = bad[0].events.clone();
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[1].actions = bad[0].actions.clone();
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        let duplicate = bad[0].nodes[0].clone();
        bad[0].nodes.push(duplicate);
        assert!(compile(&bad).is_err());
        let mut bad = fixtures();
        bad[1].plugin = "forbidden".into();
        assert_eq!(compile(&bad).unwrap_err().code, Code::Denied);
    }
    #[test]
    fn after_edges_do_not_trigger_and_dormant_predecessors_never_block() {
        let input = vec![
            contribution("alpha", true, vec![node("a", &["event.prompt"], &[], &[])]),
            contribution("beta", false, vec![node("b", &[], &[], &[])]),
            contribution(
                "gamma",
                false,
                vec![node("c", &["event.prompt"], &["alpha/a", "beta/b"], &[])],
            ),
            contribution("delta", false, vec![node("d", &[], &["alpha/a"], &[])]),
        ];
        let graph = compile(&input).unwrap();
        let active = BTreeSet::from(["event.prompt".into()]);
        assert_eq!(
            graph.activation_order(&active).unwrap(),
            ["alpha/a", "gamma/c"]
        );
        assert!(graph
            .activation_order(&BTreeSet::from(["unknown".into()]))
            .is_err());
        assert!(graph.activation_order(&BTreeSet::new()).unwrap().is_empty());
    }

    #[test]
    fn ordered_writers_are_permitted() {
        let input = vec![contribution(
            "alpha",
            true,
            vec![
                node("a", &["event.prompt"], &[], &["alpha/value"]),
                node("b", &["event.prompt"], &["alpha/a"], &["alpha/value"]),
            ],
        )];
        assert!(compile(&input).is_ok());
    }
}
