//! Offline graph composition and serial action execution through the dispatcher.
//! The graph uses the shared DAG ordering algorithm but owns graph semantics.
use crate::{
    dag,
    resolve::Selection,
    runtime::{Kernel, Outcome, QueuedEvent, Root},
    Call, Poll, Ticket,
};
use phenix_contract::{
    graph_node_key, standard_kernel_events, state_change_property, state_change_schema,
    ActionDefinition, Code, EventDefinition, Failure, GraphContribution, GraphNode, KernelEvent,
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
    /// The declaring plugin alone can emit each custom event from a live call.
    pub event_owners: BTreeMap<String, String>,
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
        // Intrinsic source vertices are present in every graph. Plugins may
        // subscribe but cannot redefine their identity or request schema.
        let mut events: BTreeMap<String, EventDefinition> = standard_kernel_events()
            .into_iter()
            .map(|event| (event.id.clone(), event))
            .collect();
        let mut event_owners = BTreeMap::<String, String>::new();
        let mut action_defs = BTreeMap::<String, ActionDefinition>::new();
        let mut nodes = BTreeMap::new();
        let mut predecessors = BTreeMap::new();

        for declaration in &normalized {
            for event in &declaration.events {
                event_owners.insert(event.id.clone(), declaration.plugin.clone());
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
                if state_change_property(trigger).is_none() && !events.contains_key(trigger) {
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
            event_owners,
            actions,
            predecessors,
            order,
        })
    }

    /// Derive one active projection. Ordering edges never activate nodes.
    /// The event and committed-property paths share the same DAG algorithm.
    fn project_triggers(&self, triggers: &BTreeSet<String>) -> Result<Vec<String>, Failure> {
        let active: BTreeSet<_> = self
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.definition
                    .triggers
                    .iter()
                    .any(|id| triggers.contains(id))
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

    /// Action event projection; unknown event identities fail closed.
    pub fn activation_order(&self, event_ids: &BTreeSet<String>) -> Result<Vec<String>, Failure> {
        for id in event_ids {
            if !self.events.contains_key(id) {
                return Err(invalid(format!("unknown admitted event {id}")));
            }
        }
        self.project_triggers(event_ids)
    }

    /// Plan committed-state hooks from verified property identities.
    /// This is graph preparation, not a second runner or implicit root.
    pub fn state_change_activation_order(
        &self,
        changed: &BTreeSet<String>,
    ) -> Result<Vec<String>, Failure> {
        let mut triggers = BTreeSet::new();
        for property in changed {
            if !phenix_contract::valid_node_key(property) {
                return Err(invalid(format!("invalid committed property {property}")));
            }
            triggers.insert(format!("state:{property}"));
        }
        self.project_triggers(&triggers)
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
            for trigger in &node.definition.triggers {
                if let Some(property_key) = state_change_property(trigger) {
                    let (owner, property) = property_key
                        .split_once('/')
                        .expect("validated property trigger");
                    let field = self
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
                                format!("state trigger {trigger} has no selected property"),
                            )
                        })?;
                    if !field.watch {
                        return Err(Failure::new(
                            Code::Denied,
                            format!("state trigger {trigger} targets an unwatched property"),
                        ));
                    }
                    let key = format!(
                        "{}/watch",
                        phenix_contract::property_contract(owner, property)?
                    );
                    if !self.permits(&node.owner, &key)
                        || !self.grants.contains_key(&(node.owner.clone(), key))
                    {
                        return Err(Failure::new(
                            Code::Denied,
                            format!("state trigger {trigger} lacks watch grant"),
                        ));
                    }
                    if operation.schema.request != state_change_schema() {
                        return Err(Failure::new(
                            Code::SchemaRejected,
                            format!("state trigger {trigger} request schema mismatch"),
                        ));
                    }
                } else if operation.schema.request != graph.events[trigger].payload {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        format!("node {id} request schema does not match event {trigger}"),
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
                // A read-only published field can only be written by the
                // *executing provider* of the node, which must be the
                // property owner. A different graph plugin may contribute
                // the node by selecting that provider through a typed import.
                // Runtime separately checks the live frame, effective scope,
                // and owner identity; this admission cannot grant them.
                if !target.set {
                    let property_contract = phenix_contract::property_contract(owner, property)?;
                    let resource = property_contract.trim_end_matches("@1");
                    let node_grant = self
                        .grants
                        .get(&(node.owner.clone(), node.definition.operation.clone()));
                    if node.provider != owner
                        || (!target.get && !target.watch)
                        || !self.authority.resources.contains(resource)
                        || !node_grant.is_some_and(|resources| resources.contains(resource))
                    {
                        return Err(Failure::new(
                            Code::Denied,
                            format!("graph property {write} is not owner-publishable"),
                        ));
                    }
                    continue;
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

/// Synchronous compatibility calls have no external cancellation opportunity.
/// Explicit callers can use begin_action_scoped/pump for longer workflows.
const MAX_SYNC_GRAPH_PUMPS: usize = 1_024;

impl Kernel {
    /// Stage only an event authored by the currently executing plugin. The
    /// current GraphExecution will handle it as a subsequent same-root wave.
    /// No user action, provider registration, credential, or authority is
    /// inferred from the event identity or payload.
    pub(super) fn queue_graph_event(
        &self,
        root: u64,
        caller: &str,
        event: &str,
        payload: Value,
    ) -> Result<(), Failure> {
        self.check_thread()?;
        let graph = self
            .prepared
            .graph
            .as_ref()
            .ok_or_else(|| Failure::new(Code::MissingBinding, "no prepared graph"))?;
        if graph.event_owners.get(event).map(String::as_str) != Some(caller) {
            return Err(Failure::new(
                Code::Denied,
                "custom event is not declared by the executing plugin",
            ));
        }
        if !graph.events[event].payload.accepts(&payload) {
            return Err(Failure::new(
                Code::SchemaRejected,
                "custom event payload does not match declared schema",
            ));
        }
        if payload.encode()?.len() > 65_536 {
            return Err(Failure::new(
                Code::SchemaRejected,
                "custom event payload exceeds bounded queue size",
            ));
        }
        let mut state = self.state.borrow_mut();
        if state.active_root != Some(root) || state.graph_root != Some(root) {
            return Err(Failure::new(
                Code::ScopeRejected,
                "custom events require a live graph execution root",
            ));
        }
        if state.graph_events.len() >= 128 || state.graph_emission_count >= 1_024 {
            return Err(Failure::new(
                Code::ScopeRejected,
                "custom graph event queue or wave budget exhausted",
            ));
        }
        state.graph_emission_count += 1;
        state.graph_events.push_back(QueuedEvent {
            id: event.into(),
            payload,
        });
        Ok(())
    }

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
        // Unlike the host-driven pump, this synchronous entrypoint cannot
        // accept cancellation while a hook keeps committing its own trigger.
        // Bound consecutive pump turns rather than introducing a timeout.
        let mut finished = false;
        for _ in 0..MAX_SYNC_GRAPH_PUMPS {
            if execution.pump(&mut observe)? == GraphStatus::Finished {
                finished = true;
                break;
            }
        }
        if !finished {
            execution.cancel()?;
            return Err(Failure::new(
                Code::InfrastructureFailure,
                "synchronous graph exceeded reactive wave limit; use the host-driven pump",
            ));
        }
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

    /// Host-only admission of a fixed lifecycle source. It uses the same
    /// graph executor and grants as every action, but cannot be submitted via
    /// the public action catalog. Begin may *initialize* the agent's main-loop
    /// state; with one serial root it must finish or be cancelled before other
    /// actions (including shutdown) can be admitted. The trusted host owns
    /// pumping and orderly lifecycle sequencing.
    pub fn begin_kernel_event(&self, event: KernelEvent) -> Result<GraphExecution<'_>, Failure> {
        self.check_thread()?;
        match &event {
            KernelEvent::PluginLoaded { plugin } | KernelEvent::PluginReloaded { plugin, .. } => {
                if !self.images.borrow().contains_key(plugin) {
                    return Err(Failure::new(
                        Code::MissingBinding,
                        "plugin lifecycle event references an unloaded plugin",
                    ));
                }
            }
            KernelEvent::PluginUnloaded { plugin } => {
                // A genuine unloaded notification is only valid after a
                // corresponding loaded notification and native removal.
                let state = self.state.borrow();
                if self.images.borrow().contains_key(plugin)
                    || !state.kernel_loaded.contains(plugin)
                {
                    return Err(Failure::new(
                        Code::ScopeRejected,
                        "plugin-unloaded requires completed native unload",
                    ));
                }
            }
            KernelEvent::Begin | KernelEvent::Shutdown => {}
        }
        if let KernelEvent::PluginReloaded {
            previous_generation,
            generation,
            ..
        } = &event
        {
            if previous_generation.is_empty()
                || generation.is_empty()
                || previous_generation == generation
            {
                return Err(Failure::new(
                    Code::SchemaRejected,
                    "plugin-reloaded requires distinct generation identities",
                ));
            }
        }
        {
            let state = self.state.borrow();
            let already_emitted = match &event {
                KernelEvent::Begin => state.kernel_begin,
                KernelEvent::PluginLoaded { plugin } => state.kernel_loaded.contains(plugin),
                KernelEvent::PluginUnloaded { plugin } => state.kernel_unloaded.contains(plugin),
                KernelEvent::PluginReloaded { plugin, .. } => {
                    state.kernel_reloaded.contains(plugin)
                }
                KernelEvent::Shutdown => state.kernel_shutdown,
            };
            if already_emitted {
                return Err(Failure::new(
                    Code::ScopeRejected,
                    "lifecycle source already admitted in this generation",
                ));
            }
        }
        let id = event.id();
        let scope = ScopedRequest {
            authority: self.prepared.authority.clone(),
            resource: self.prepared.resource.clone(),
        };
        let execution = self.begin_event_inner(id, id, &event.payload(), scope, true)?;
        // Admit before committing the one-shot marker. A denied/busy attempt
        // must remain retryable with a correct scope after its root is dropped.
        let mut state = self.state.borrow_mut();
        match event {
            KernelEvent::Begin => state.kernel_begin = true,
            KernelEvent::PluginLoaded { plugin } => {
                state.kernel_loaded.insert(plugin);
            }
            KernelEvent::PluginUnloaded { plugin } => {
                state.kernel_unloaded.insert(plugin);
            }
            KernelEvent::PluginReloaded { plugin, .. } => {
                state.kernel_reloaded.insert(plugin);
            }
            KernelEvent::Shutdown => state.kernel_shutdown = true,
        }
        Ok(execution)
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

        self.begin_event_inner(action_id, &action.event, payload, scope, pending_allowed)
    }

    fn begin_event_inner(
        &self,
        action_id: &str,
        event: &str,
        payload: &Value,
        scope: ScopedRequest,
        pending_allowed: bool,
    ) -> Result<GraphExecution<'_>, Failure> {
        let graph = self
            .prepared
            .graph
            .as_ref()
            .ok_or_else(|| Failure::new(Code::MissingBinding, "no prepared graph"))?;
        if !graph
            .events
            .get(event)
            .is_some_and(|declared| declared.payload.accepts(payload))
        {
            return Err(Failure::new(
                Code::SchemaRejected,
                "graph event payload does not match its schema",
            ));
        }
        let active = graph.activation_order(&BTreeSet::from([event.into()]))?;
        let epoch_cursor = self.cells.borrow().epoch().0;
        let root = self.admit_scoped_root(self.prepared.entry.clone(), scope)?;
        {
            let mut state = self.state.borrow_mut();
            state.graph_root = Some(root.id);
            state.graph_events.clear();
            state.graph_emission_count = 0;
        }
        Ok(GraphExecution {
            root,
            action: action_id.into(),
            event: event.into(),
            payload: payload.clone(),
            node_requests: BTreeMap::new(),
            state_cursor: epoch_cursor,
            had_domain: false,
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

/// A node receives only the changed properties it declared. Multiple
/// committed properties activate it once, with the same epoch stamp.
fn state_hook_requests(
    graph: &CompiledGraph,
    active: &[String],
    changed: &BTreeSet<String>,
    epoch: u64,
) -> Result<BTreeMap<String, Value>, Failure> {
    let mut requests = BTreeMap::new();
    for node in active {
        let matching = graph.nodes[node]
            .definition
            .triggers
            .iter()
            .filter_map(|trigger| state_change_property(trigger))
            .filter(|property| changed.contains(*property))
            .map(|property| Value::String(property.into()))
            .collect::<Vec<_>>();
        if matching.is_empty() {
            return Err(Failure::new(
                Code::InfrastructureFailure,
                "state hook activated without a changed property",
            ));
        }
        requests.insert(
            node.clone(),
            Value::map([
                ("epoch", Value::U64(epoch)),
                ("properties", Value::List(matching)),
            ]),
        );
    }
    Ok(requests)
}

/// Readiness bookkeeping around one existing Root and its Ticket handles.
/// The host controls polling; no timeout or busy loop is imposed by the kernel.
pub struct GraphExecution<'a> {
    root: Root<'a>,
    action: String,
    event: String,
    payload: Value,
    /// Event nodes use the action payload; state hooks get scoped epoch data.
    node_requests: BTreeMap<String, Value>,
    state_cursor: u64,
    had_domain: bool,
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
    /// Read the latest watchable properties committed after a graph epoch.
    /// Changes coalesce by property. A caller can advance its epoch cursor
    /// only after processing the returned set. Hook admission follows in G4.
    pub fn committed_changes_since(&self, after: u64) -> (u64, Vec<String>) {
        let (epoch, changed) = self
            .root
            .kernel
            .cells
            .borrow()
            .watched_changes_since(phenix_contract::Epoch(after));
        (epoch.0, changed)
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
            } else if self.had_domain {
                GraphCompletion::Domain
            } else {
                GraphCompletion::Success
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
                    self.had_domain = true;
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
    /// Admit one committed epoch through the prepared state-trigger DAG.
    /// This uses the existing root, provider selection and ticket lifecycle.
    /// The caller pumps the new wave just like the original event wave.
    fn admit_state_wave(&mut self) -> Result<bool, Failure> {
        let (epoch, changed) = self
            .root
            .kernel
            .cells
            .borrow()
            .watched_changes_since(phenix_contract::Epoch(self.state_cursor));
        self.state_cursor = epoch.0;
        if changed.is_empty() {
            return Ok(false);
        }
        let changed: BTreeSet<String> = changed.into_iter().collect();
        let graph = self.root.kernel.prepared.graph.as_ref().unwrap();
        let active = graph.state_change_activation_order(&changed)?;
        if active.is_empty() {
            return Ok(false);
        }
        let node_requests = state_hook_requests(graph, &active, &changed, epoch.0)?;
        self.waiting = active.iter().cloned().collect();
        self.active = active;
        self.unsuccessful.clear();
        self.node_requests = node_requests;
        Ok(true)
    }

    pub fn pump(
        &mut self,
        mut observe: impl FnMut(&GraphUpdate<'_>),
    ) -> Result<GraphStatus, Failure> {
        self.root.kernel.check_thread()?;
        if !self.admitted {
            let graph = self.root.kernel.prepared.graph.as_ref().unwrap();
            // Intrinsic kernel sources are deliberately NOT public actions.
            // The execution records both identities at admission, rather
            // than indexing the user-action map for an intrinsic event.
            self.emit(
                GraphEvent::Admitted {
                    action: &self.action,
                    event: &self.event,
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
                    self.had_domain = true;
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
                let request = self.node_requests.get(&node).unwrap_or(&self.payload);
                match self
                    .root
                    .call_graph(&owner, &target, request, self.pending_allowed)
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
        // A finished wave may have published watched cells. Activate their
        // hooks before declaring the admitted root terminal. Hooks can publish
        // further epochs; each is observed after its own wave settles.
        if !self.terminal && !self.stopped && self.waiting.is_empty() && self.pending.is_empty() {
            self.admit_state_wave()?;
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
    fn intrinsic_lifecycle_sources_are_not_public_actions() {
        let graph = compile(&[contribution(
            "alpha",
            false,
            vec![node("a", &[phenix_contract::KERNEL_BEGIN], &[], &[])],
        )])
        .unwrap();
        assert_eq!(
            graph
                .activation_order(&BTreeSet::from([phenix_contract::KERNEL_BEGIN.into(),]))
                .unwrap(),
            ["alpha/a"]
        );
        assert!(graph.discover().next().is_none());
        assert!(graph.actions.is_empty());
    }

    #[test]
    fn intrinsic_lifecycle_sources_activate_without_plugin_event_definitions() {
        let declarations = [
            contribution(
                "alpha",
                false,
                vec![node("a", &[phenix_contract::KERNEL_BEGIN], &[], &[])],
            ),
            contribution(
                "beta",
                false,
                vec![node(
                    "b",
                    &[phenix_contract::KERNEL_PLUGIN_LOADED],
                    &[],
                    &[],
                )],
            ),
            contribution(
                "gamma",
                false,
                vec![node("c", &[phenix_contract::KERNEL_SHUTDOWN], &[], &[])],
            ),
            contribution(
                "delta",
                false,
                vec![node(
                    "d",
                    &[phenix_contract::KERNEL_PLUGIN_UNLOADED],
                    &[],
                    &[],
                )],
            ),
            contribution(
                "epsilon",
                false,
                vec![node(
                    "e",
                    &[phenix_contract::KERNEL_PLUGIN_RELOADED],
                    &[],
                    &[],
                )],
            ),
        ];
        let graph = compile(&declarations).unwrap();
        assert_eq!(graph.events.len(), 5);
        assert!(graph.actions.is_empty());
        for (event, expected) in [
            (phenix_contract::KERNEL_BEGIN, "alpha/a"),
            (phenix_contract::KERNEL_PLUGIN_LOADED, "beta/b"),
            (phenix_contract::KERNEL_SHUTDOWN, "gamma/c"),
            (phenix_contract::KERNEL_PLUGIN_UNLOADED, "delta/d"),
            (phenix_contract::KERNEL_PLUGIN_RELOADED, "epsilon/e"),
        ] {
            assert_eq!(
                graph
                    .activation_order(&BTreeSet::from([event.into()]))
                    .unwrap(),
                [expected]
            );
        }
        let mut invalid = declarations.to_vec();
        invalid[0].events.push(EventDefinition {
            id: phenix_contract::KERNEL_BEGIN.into(),
            payload: ValueSchema::String,
        });
        assert!(compile(&invalid)
            .unwrap_err()
            .message
            .contains("duplicate event"));
    }

    #[test]
    fn arbitrary_plugin_events_preserve_existing_graph_contracts() {
        let declarations = vec![
            GraphContribution {
                plugin: "phenix.providers".into(),
                events: vec![EventDefinition {
                    id: "phenix.providers.populate".into(),
                    payload: ValueSchema::String,
                }],
                actions: vec![],
                nodes: vec![],
            },
            contribution(
                "openai",
                false,
                vec![node("register", &["phenix.providers.populate"], &[], &[])],
            ),
        ];
        let graph = compile(&declarations).unwrap();
        assert_eq!(
            graph
                .activation_order(&BTreeSet::from(["phenix.providers.populate".into(),]))
                .unwrap(),
            ["openai/register"]
        );
        assert!(graph.actions.is_empty());
        assert_eq!(graph.event_owners["phenix.providers.populate"], "phenix.providers");
        let mut bad = declarations;
        bad[0].events[0].id = "phenix.kernel.fabricated".into();
        assert!(compile(&bad).is_err());
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
    fn committed_state_projection_activates_only_watching_nodes() {
        let declarations = vec![
            contribution(
                "alpha",
                true,
                vec![node("a", &["state:skills.api/items"], &[], &[])],
            ),
            contribution(
                "beta",
                false,
                vec![node("b", &["state:context.api/items"], &[], &[])],
            ),
            contribution(
                "gamma",
                false,
                vec![node(
                    "c",
                    &["state:skills.api/items"],
                    &["alpha/a", "beta/b"],
                    &[],
                )],
            ),
            contribution("delta", false, vec![node("d", &[], &["alpha/a"], &[])]),
        ];
        let graph = compile(&declarations).unwrap();
        let mut reverse = declarations;
        reverse.reverse();
        assert_eq!(graph.identity, compile(&reverse).unwrap().identity);
        assert_eq!(
            graph
                .state_change_activation_order(&BTreeSet::from(["skills.api/items".into()]))
                .unwrap(),
            ["alpha/a", "gamma/c"]
        );
        assert_eq!(
            graph
                .state_change_activation_order(&BTreeSet::from([
                    "skills.api/items".into(),
                    "context.api/items".into(),
                ]))
                .unwrap(),
            ["alpha/a", "beta/b", "gamma/c"]
        );
        assert!(graph
            .state_change_activation_order(&BTreeSet::from(["missing/api/id".into()]))
            .is_err());
        assert!(graph
            .state_change_activation_order(&BTreeSet::from(["skills.api/absent".into()]))
            .unwrap()
            .is_empty());
        assert!(graph
            .activation_order(&BTreeSet::from(["state:skills.api/items".into()]))
            .is_err());
    }

    #[test]
    fn state_hook_payload_filters_unwatched_changes_per_node() {
        let graph = compile(&[
            contribution(
                "alpha",
                true,
                vec![node(
                    "a",
                    &["state:skills.api/items", "state:skills.api/revision"],
                    &[],
                    &[],
                )],
            ),
            contribution(
                "beta",
                false,
                vec![node("b", &["state:context.api/items"], &[], &[])],
            ),
        ])
        .unwrap();
        let changed = BTreeSet::from([
            "skills.api/items".into(),
            "skills.api/revision".into(),
            "context.api/items".into(),
        ]);
        let active = graph.state_change_activation_order(&changed).unwrap();
        let requests = state_hook_requests(&graph, &active, &changed, 5).unwrap();
        assert_eq!(
            requests["alpha/a"],
            Value::map([
                ("epoch", Value::U64(5)),
                (
                    "properties",
                    Value::List(vec![
                        Value::String("skills.api/items".into()),
                        Value::String("skills.api/revision".into()),
                    ]),
                ),
            ])
        );
        assert_eq!(
            requests["beta/b"],
            Value::map([
                ("epoch", Value::U64(5)),
                (
                    "properties",
                    Value::List(vec![Value::String("context.api/items".into())]),
                ),
            ])
        );
        assert!(state_hook_requests(
            &graph,
            &["alpha/a".into()],
            &BTreeSet::from(["context.api/items".into()]),
            6,
        )
        .is_err());
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
