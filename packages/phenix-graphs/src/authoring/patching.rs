//! Pre-compilation patch planning. A contributor supplies symbolic write claims
//! plus a closure editing a GraphBuilder. No callback enters the compiled graph.
//! This is deliberately independent of plugin loading and authorization.
use super::*;
mod builder;
mod route;
pub use builder::PatchBuilder;
pub use route::{ConnectionRoute, PortSelector};

/// The smallest semantic unit a contributor may change. Physical node/edge IDs
/// are never used to identify patch targets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatchTarget {
    Graph,
    Node(String),
    Instance(String),
    Connection(String),
    ConnectionPattern(ConnectionRoute),
    /// Connections whose source or destination is this direct operation.
    /// Distinct operation claims commute even when they retarget opposite ends
    /// of the same edge; explicit topology edits still conflict with either.
    IncidentConnections(String),
    /// Ordered insertions on one *original* edge compose, not compete.
    Splice(PatchAnchor),
    /// Explicit broad claim: all connections. Useful for wholesale rewrites,
    /// but conflicts with every more selective connection patch.
    Connections,
    PublicInput(String),
    PublicOutput(String),
    Trigger(String),
}
impl PatchTarget {
    /// Name a connection using the same symbolic endpoints the builder uses.
    pub fn connection_pattern(from: &str, to: &str) -> Result<Self, PatchError> {
        Ok(Self::ConnectionPattern(ConnectionRoute::parse(from, to)?))
    }
    pub fn edge_nodes(from: &str, output: &str, to: &str, port: &str, slot: &str) -> Self {
        Self::connection(
            ModuleOutputSource::NodeNamed {
                node: from.into(),
                output: output.into(),
            },
            ModuleInputTarget::NodeNamed {
                node: to.into(),
                port: port.into(),
                slot: slot.into(),
            },
        )
    }
    pub fn edge_node_to_instance(from: &str, output: &str, to: &str, port: &str) -> Self {
        Self::connection(
            ModuleOutputSource::NodeNamed {
                node: from.into(),
                output: output.into(),
            },
            ModuleInputTarget::InstanceNamed {
                name: to.into(),
                port: port.into(),
            },
        )
    }
    pub fn edge_instance_to_node(
        from: &str,
        output: &str,
        to: &str,
        port: &str,
        slot: &str,
    ) -> Self {
        Self::connection(
            ModuleOutputSource::InstanceNamed {
                name: from.into(),
                port: output.into(),
            },
            ModuleInputTarget::NodeNamed {
                node: to.into(),
                port: port.into(),
                slot: slot.into(),
            },
        )
    }
    pub fn edge_instances(from: &str, output: &str, to: &str, port: &str) -> Self {
        Self::connection(
            ModuleOutputSource::InstanceNamed {
                name: from.into(),
                port: output.into(),
            },
            ModuleInputTarget::InstanceNamed {
                name: to.into(),
                port: port.into(),
            },
        )
    }
    fn connection(from: ModuleOutputSource, to: ModuleInputTarget) -> Self {
        Self::Connection(patch_connection_key(&from, &to))
    }
    fn overlaps(&self, other: &Self) -> bool {
        use PatchTarget::*;
        match (self, other) {
            (Splice(_), Splice(_)) => false, // orderable, validated separately
            (Graph, _) | (_, Graph) => true,
            (IncidentConnections(_), Connections) | (Connections, IncidentConnections(_)) => true,
            (IncidentConnections(name), Connection(edge))
            | (Connection(edge), IncidentConnections(name)) => {
                ConnectionRoute::from_exact_key(edge)
                    .is_some_and(|route| route.from.node == *name || route.to.node == *name)
            }
            (IncidentConnections(name), ConnectionPattern(route))
            | (ConnectionPattern(route), IncidentConnections(name)) => {
                route.from.node == *name || route.to.node == *name
            }
            (incident @ IncidentConnections(_), Splice(anchor))
            | (Splice(anchor), incident @ IncidentConnections(_)) => {
                incident.overlaps(&anchor.edge_target())
            }
            (Connections, Connection(_))
            | (Connection(_), Connections)
            | (Connections, ConnectionPattern(_))
            | (ConnectionPattern(_), Connections)
            | (Connections, Splice(_))
            | (Splice(_), Connections) => true,
            (Splice(anchor), Connection(edge)) | (Connection(edge), Splice(anchor)) => {
                anchor.edge_target() == Connection(edge.clone())
            }
            (ConnectionPattern(a), ConnectionPattern(b)) => a.pattern_overlaps(b),
            (ConnectionPattern(a), Connection(b)) | (Connection(b), ConnectionPattern(a)) => {
                ConnectionRoute::from_exact_key(b).is_some_and(|b| a.pattern_overlaps(&b))
            }
            (Splice(anchor), ConnectionPattern(route))
            | (ConnectionPattern(route), Splice(anchor)) => anchor
                .edge_target()
                .overlaps(&ConnectionPattern(route.clone())),
            (Node(a), Instance(b)) | (Instance(a), Node(b)) => a == b,
            _ => self == other,
        }
    }
    // Write authority is directional; conflict overlap must remain symmetric.
    fn covers(&self, observed: &Self) -> bool {
        use PatchTarget::*;
        match (self, observed) {
            (Graph, _) => true,
            (_, Graph) => false,
            (Node(a) | Instance(a), Node(b) | Instance(b)) => a == b,
            (Connections, Connection(_)) => true,
            (ConnectionPattern(pattern), Connection(key)) => ConnectionRoute::from_exact_key(key)
                .is_some_and(|exact| {
                    let matches = |a: &PortSelector, b: &PortSelector| {
                        a.node == b.node
                            && a.port.as_ref().is_none_or(|p| Some(p) == b.port.as_ref())
                            && a.slot.as_ref().is_none_or(|s| Some(s) == b.slot.as_ref())
                    };
                    matches(&pattern.from, &exact.from) && matches(&pattern.to, &exact.to)
                }),
            (IncidentConnections(name), Connection(key)) => ConnectionRoute::from_exact_key(key)
                .is_some_and(|r| r.from.node == *name || r.to.node == *name),
            (Connection(a), Connection(b))
            | (PublicInput(a), PublicInput(b))
            | (PublicOutput(a), PublicOutput(b))
            | (Trigger(a), Trigger(b)) => a == b,
            _ => false,
        }
    }
}

impl From<observer::Requirement> for PatchTarget {
    fn from(value: observer::Requirement) -> Self {
        use observer::Requirement as R;
        match value {
            R::Graph => Self::Graph,
            R::Node(n) => Self::Node(n),
            R::Instance(n) => Self::Instance(n),
            R::Connection(n) => Self::Connection(n),
            R::PublicInput(n) => Self::PublicInput(n),
            R::PublicOutput(n) => Self::PublicOutput(n),
            R::Trigger(n) => Self::Trigger(n),
        }
    }
}

/// Symbolic source and target names, independent of physical graph IDs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatchSource {
    Node { name: String, output: String },
    Instance { name: String, output: String },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PatchDestination {
    Node {
        name: String,
        port: String,
        slot: String,
    },
    Instance {
        name: String,
        input: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PatchAnchor {
    pub from: PatchSource,
    pub to: PatchDestination,
}
impl PatchAnchor {
    pub fn nodes(from: &str, output: &str, to: &str, port: &str, slot: &str) -> Self {
        Self {
            from: PatchSource::Node {
                name: from.into(),
                output: output.into(),
            },
            to: PatchDestination::Node {
                name: to.into(),
                port: port.into(),
                slot: slot.into(),
            },
        }
    }
    pub fn node_to_instance(from: &str, output: &str, to: &str, input: &str) -> Self {
        Self {
            from: PatchSource::Node {
                name: from.into(),
                output: output.into(),
            },
            to: PatchDestination::Instance {
                name: to.into(),
                input: input.into(),
            },
        }
    }
    pub fn instance_to_node(from: &str, output: &str, to: &str, port: &str, slot: &str) -> Self {
        Self {
            from: PatchSource::Instance {
                name: from.into(),
                output: output.into(),
            },
            to: PatchDestination::Node {
                name: to.into(),
                port: port.into(),
                slot: slot.into(),
            },
        }
    }
    pub fn instances(from: &str, output: &str, to: &str, input: &str) -> Self {
        Self {
            from: PatchSource::Instance {
                name: from.into(),
                output: output.into(),
            },
            to: PatchDestination::Instance {
                name: to.into(),
                input: input.into(),
            },
        }
    }
    fn source(&self) -> Source {
        match &self.from {
            PatchSource::Node { name, output } => Source::Node {
                node: name.clone(),
                output: output.clone(),
            },
            PatchSource::Instance { name, output } => Source::Instance {
                name: name.clone(),
                port: output.clone(),
            },
        }
    }
    fn target(&self) -> Target {
        match &self.to {
            PatchDestination::Node { name, port, slot } => Target::Node {
                node: name.clone(),
                port: port.clone(),
                slot: slot.clone(),
            },
            PatchDestination::Instance { name, input } => Target::Instance {
                name: name.clone(),
                port: input.clone(),
            },
        }
    }
    fn edge_target(&self) -> PatchTarget {
        let src = match self.source() {
            Source::Node { node, output } => ModuleOutputSource::NodeNamed { node, output },
            Source::Instance { name, port } => ModuleOutputSource::InstanceNamed { name, port },
        };
        let dst = match self.target() {
            Target::Node { node, port, slot } => ModuleInputTarget::NodeNamed { node, port, slot },
            Target::Instance { name, port } => ModuleInputTarget::InstanceNamed { name, port },
        };
        PatchTarget::connection(src, dst)
    }
}

/// Exact insertion coordinate on an existing edge, strictly between 0 and 1.
/// Fractions are reduced; comparisons are exact with u128 intermediates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplicePosition {
    numerator: u64,
    denominator: u64,
}
impl SplicePosition {
    pub fn numerator(self) -> u64 {
        self.numerator
    }
    pub fn denominator(self) -> u64 {
        self.denominator
    }
    pub fn new(mut numerator: u64, mut denominator: u64) -> Result<Self, PatchError> {
        if denominator == 0 || numerator == 0 || numerator >= denominator {
            return Err(PatchError::InvalidPosition);
        }
        let (mut a, mut b) = (numerator, denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        numerator /= a;
        denominator /= a;
        Ok(Self {
            numerator,
            denominator,
        })
    }
}
impl Ord for SplicePosition {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.numerator as u128 * other.denominator as u128)
            .cmp(&(other.numerator as u128 * self.denominator as u128))
    }
}
impl PartialOrd for SplicePosition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedSplice {
    pub contributor: String,
    pub anchor: PatchAnchor,
    pub position: SplicePosition,
    pub splice_order: Option<i32>,
    pub node: String,
}

impl GraphBuilder {
    /// Lower a group of selected, ordered insertions into a single chain.
    /// The original edge is removed once, after the inserted nodes exist.
    /// The full change occurs inside one assembly transaction.
    fn apply_splice_chain(&mut self, anchor: &PatchAnchor, nodes: &[String]) -> Result<(), String> {
        if nodes.is_empty() {
            return Ok(());
        }
        let source = anchor.source();
        let target = anchor.target();
        let actual = match &source {
            Source::Node { node, output } => {
                let n = self
                    .module
                    .nodes
                    .iter()
                    .find(|n| n.name.as_deref() == Some(node))
                    .ok_or_else(|| format!("splice unknown node {node:?}"))?;
                let addr = n.output(output)?;
                debug_assert_eq!(n.output.id, addr.slot);
                n.output.identity()
            }
            Source::Instance { name, port } => {
                let module = self.dependency_for_instance(name)?;
                module
                    .definition
                    .output
                    .iter()
                    .find(|p| &p.name == port)
                    .ok_or_else(|| format!("splice unknown instance output {name}.{port}"))?
                    .identity()
            }
        };
        let src = self.verify_source(&source, actual.clone())?;
        let dst = self.verify_target(&target, actual.clone())?;
        if !self
            .module
            .edges
            .iter()
            .any(|e| e.from == src && e.to == dst)
        {
            return Err(format!("splice anchor absent: {anchor:?}"));
        }
        let mut next_edges = Vec::new();
        let mut prev = src.clone();
        for name in nodes {
            let node_input = self.verify_target(
                &Target::Node {
                    node: name.clone(),
                    port: "run".into(),
                    slot: "value".into(),
                },
                actual.clone(),
            )?;
            let node_output = self.verify_source(
                &Source::Node {
                    node: name.clone(),
                    output: "value".into(),
                },
                actual.clone(),
            )?;
            next_edges.push((prev, node_input));
            prev = node_output;
        }
        next_edges.push((prev, dst.clone()));
        for (from, to) in &next_edges {
            if self
                .module
                .edges
                .iter()
                .any(|edge| edge.from == *from && edge.to == *to)
            {
                return Err(format!("splice would duplicate edge {from:?} -> {to:?}"));
            }
        }
        self.module
            .edges
            .retain(|edge| !(edge.from == src && edge.to == dst));
        for (from, to) in next_edges {
            self.module.connect(from, to);
        }
        Ok(())
    }
}

/// Metadata supplied by the graph-building contributor. `after` describes
/// semantic dependencies, not an implicit plugin-load order. Concurrent edits
/// to the same target either depend on one another or compete by explicit
/// unequal priority. No priority is inferred from contributor identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchSpec {
    pub id: String,
    pub after: BTreeSet<String>,
    pub priority: Option<i32>,
    pub writes: BTreeSet<PatchTarget>,
}
impl PatchSpec {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            after: BTreeSet::new(),
            priority: None,
            writes: BTreeSet::new(),
        }
    }
    pub fn after(mut self, id: impl Into<String>) -> Self {
        self.after.insert(id.into());
        self
    }
    pub fn priority(mut self, value: i32) -> Self {
        self.priority = Some(value);
        self
    }
    pub fn writes(mut self, target: PatchTarget) -> Self {
        self.writes.insert(target);
        self
    }
    pub fn writes_all(mut self, targets: impl IntoIterator<Item = PatchTarget>) -> Self {
        self.writes.extend(targets);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchError {
    InvalidId(String),
    Duplicate(String),
    MissingDependency {
        patch: String,
        dependency: String,
    },
    DependencyCycle(Vec<String>),
    Conflict {
        first: String,
        second: String,
        target: Box<PatchTarget>,
    },
    SupersededDependency {
        patch: String,
        dependency: String,
    },
    UndeclaredWrite {
        patch: String,
        target: Box<PatchTarget>,
    },
    Apply {
        patch: String,
        reason: String,
    },
    Compile(CompileDiagnostic),
    InvalidPosition,
    InvalidEndpoint(String),
    SpliceCollision {
        first: String,
        second: String,
        anchor: Box<PatchAnchor>,
    },
}
impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(f, "invalid contribution ID {id:?}"),
            Self::Duplicate(id) => write!(f, "duplicate contribution {id:?}"),
            Self::MissingDependency { patch, dependency } => {
                write!(f, "{patch:?} requires absent contribution {dependency:?}")
            }
            Self::DependencyCycle(ids) => write!(f, "patch dependency cycle: {}", ids.join(" -> ")),
            Self::Conflict {
                first,
                second,
                target,
            } => write!(
                f,
                "unresolved patch conflict between {first:?} and {second:?} at {target:?}"
            ),
            Self::SupersededDependency { patch, dependency } => write!(
                f,
                "{patch:?} requires superseded contribution {dependency:?}"
            ),
            Self::UndeclaredWrite { patch, target } => {
                write!(f, "{patch:?} modified undeclared target {target:?}")
            }
            Self::Apply { patch, reason } => write!(f, "contribution {patch:?} failed: {reason}"),
            Self::Compile(reason) => write!(f, "compiled assembly rejected: {reason}"),
            Self::InvalidPosition => {
                write!(f, "splice position must be strictly between zero and one")
            }
            Self::InvalidEndpoint(path) => write!(f, "invalid symbolic endpoint {path:?}"),
            Self::SpliceCollision {
                first,
                second,
                anchor,
            } => write!(
                f,
                "unordered same-position splices {first:?} and {second:?} on {anchor:?}"
            ),
        }
    }
}
impl std::error::Error for PatchError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupersededPatch {
    pub loser: String,
    pub winner: String,
    pub target: PatchTarget,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchResolution {
    /// Deterministic dependency-topological application order.
    pub selected: Vec<String>,
    pub superseded: Vec<SupersededPatch>,
    /// Position-ordered insertions, resolved against original symbolic edges.
    pub splices: Vec<PlannedSplice>,
}

type Edit = Box<dyn FnOnce(&mut GraphBuilder) -> Result<(), String> + 'static>;
struct Contribution {
    spec: PatchSpec,
    edit: Option<Edit>,
    splice: Option<PlannedSplice>,
}

/// Deterministic conflict planning over an arbitrary set of contributors.
/// `apply` atomically commits the whole selected plan's candidate-owned state.
/// Captured mutable state and handles into other builders are not isolated.
#[derive(Default)]
pub struct PatchPlanner {
    entries: BTreeMap<String, Contribution>,
}
impl PatchPlanner {
    pub fn new() -> Self {
        Self::default()
    }

    fn register<F>(&mut self, spec: PatchSpec, edit: F) -> Result<(), PatchError>
    where
        F: FnOnce(&mut GraphBuilder) -> Result<(), String> + 'static,
    {
        validate_name_segment(&spec.id).map_err(|_| PatchError::InvalidId(spec.id.clone()))?;
        if spec.id == "graph.splice.lowering" {
            return Err(PatchError::InvalidId(spec.id));
        }
        for dep in &spec.after {
            validate_name_segment(dep).map_err(|_| PatchError::InvalidId(dep.clone()))?;
        }
        if self.entries.contains_key(&spec.id) {
            return Err(PatchError::Duplicate(spec.id));
        }
        self.entries.insert(
            spec.id.clone(),
            Contribution {
                spec,
                edit: Some(Box::new(edit)),
                splice: None,
            },
        );
        Ok(())
    }

    pub fn resolve(&self) -> Result<PatchResolution, PatchError> {
        // Step 1: ensure the dependency DAG is complete and acyclic. A sorted
        // Kahn traversal is deterministic regardless of registration order.
        let mut pending = BTreeMap::<String, usize>::new();
        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        for (id, entry) in &self.entries {
            pending.insert(id.clone(), entry.spec.after.len());
            for dep in &entry.spec.after {
                if !self.entries.contains_key(dep) {
                    return Err(PatchError::MissingDependency {
                        patch: id.clone(),
                        dependency: dep.clone(),
                    });
                }
                children.entry(dep.clone()).or_default().insert(id.clone());
            }
        }
        let mut ready: BTreeSet<String> = pending
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut order = Vec::with_capacity(pending.len());
        while let Some(id) = ready.pop_first() {
            order.push(id.clone());
            for child in children.get(&id).into_iter().flatten() {
                let count = pending.get_mut(child).expect("registered child");
                *count -= 1;
                if *count == 0 {
                    ready.insert(child.clone());
                }
            }
        }
        if order.len() != self.entries.len() {
            return Err(PatchError::DependencyCycle(
                pending
                    .into_iter()
                    .filter_map(|(id, n)| (n > 0).then_some(id))
                    .collect(),
            ));
        }
        // Step 2: transitive dependency closure. Explicitly dependent edits
        // are layered in topology order, not treated as competing alternatives.
        let mut ancestors = BTreeMap::<String, BTreeSet<String>>::new();
        for id in &order {
            let mut deps = BTreeSet::new();
            for dep in &self.entries[id].spec.after {
                deps.insert(dep.clone());
                deps.extend(ancestors[dep].iter().cloned());
            }
            ancestors.insert(id.clone(), deps);
        }
        // Step 3: select competing independent contributions by *explicit*
        // priority; ties/missing priorities fail closed. Higher-priority
        // selected patches are evaluated first, preventing a losing patch
        // from incorrectly excluding a third independent patch.
        let mut candidates = order.clone();
        candidates.sort_by(|a, b| {
            self.entries[b]
                .spec
                .priority
                .cmp(&self.entries[a].spec.priority)
                .then_with(|| a.cmp(b))
        });
        let mut selected = BTreeSet::<String>::new();
        let mut superseded = Vec::new();
        for id in candidates {
            let spec = &self.entries[&id].spec;
            let mut lost_to = None;
            for winner in &selected {
                if ancestors[&id].contains(winner) || ancestors[winner].contains(&id) {
                    continue;
                }
                let other = &self.entries[winner].spec;
                let target = spec.writes.iter().find_map(|left| {
                    other
                        .writes
                        .iter()
                        .find(|right| left.overlaps(right))
                        .map(|_| left.clone())
                });
                if let Some(target) = target {
                    match (spec.priority, other.priority) {
                        (Some(lo), Some(hi)) if lo < hi => {
                            lost_to = Some(SupersededPatch {
                                loser: id.clone(),
                                winner: winner.clone(),
                                target,
                            });
                            break;
                        }
                        _ => {
                            return Err(PatchError::Conflict {
                                first: id.clone(),
                                second: winner.clone(),
                                target: Box::new(target),
                            });
                        }
                    }
                }
            }
            if let Some(lost) = lost_to {
                superseded.push(lost);
            } else {
                selected.insert(id);
            }
        }
        // Step 4: a contribution may never be applied without all its
        // explicit prerequisites. Do not silently skip its dependency.
        for id in &selected {
            for dep in &self.entries[id].spec.after {
                if !selected.contains(dep) {
                    return Err(PatchError::SupersededDependency {
                        patch: id.clone(),
                        dependency: dep.clone(),
                    });
                }
            }
        }
        superseded.sort_by(|a, b| a.loser.cmp(&b.loser));
        let mut splices = selected
            .iter()
            .filter_map(|id| self.entries[id].splice.clone())
            .collect::<Vec<_>>();
        splices.sort_by(|a, b| {
            a.anchor
                .cmp(&b.anchor)
                .then_with(|| a.position.cmp(&b.position))
                .then_with(|| a.splice_order.cmp(&b.splice_order))
        });
        for pair in splices.windows(2) {
            if pair[0].anchor == pair[1].anchor
                && pair[0].position == pair[1].position
                && (pair[0].splice_order.is_none()
                    || pair[1].splice_order.is_none()
                    || pair[0].splice_order == pair[1].splice_order)
            {
                return Err(PatchError::SpliceCollision {
                    first: pair[0].contributor.clone(),
                    second: pair[1].contributor.clone(),
                    anchor: Box::new(pair[0].anchor.clone()),
                });
            }
        }
        Ok(PatchResolution {
            selected: order
                .into_iter()
                .filter(|id| selected.contains(id))
                .collect(),
            superseded,
            splices,
        })
    }

    pub fn apply(mut self, assembly: &mut GraphAssembly) -> Result<PatchResolution, PatchError> {
        let plan = self.resolve()?;
        // No partial assembly mutations if a selected edit fails, including
        // an attempt to write outside its declared symbolic footprint.
        let mut draft = assembly.clone();
        for id in &plan.selected {
            let mut contribution = self.entries.remove(id).expect("selected patch exists");
            let edit = contribution.edit.take().expect("patch closure exists");
            let required = draft
                .contribute(id.clone(), |builder| edit(builder))
                .map_err(|reason| PatchError::Apply {
                    patch: id.clone(),
                    reason,
                })?;
            for requirement in required {
                let target = PatchTarget::from(requirement);
                if !contribution
                    .spec
                    .writes
                    .iter()
                    .any(|allowed| allowed.covers(&target))
                {
                    return Err(PatchError::UndeclaredWrite {
                        patch: id.clone(),
                        target: Box::new(target),
                    });
                }
            }
        }
        // Compose all selected insertions on each original anchor. They must
        // not race to disconnect/reconnect it individually.
        let mut groups = BTreeMap::<PatchAnchor, Vec<String>>::new();
        for splice in &plan.splices {
            groups
                .entry(splice.anchor.clone())
                .or_default()
                .push(splice.node.clone());
        }
        if !groups.is_empty() {
            draft
                .contribute("graph.splice.lowering", |graph| {
                    for (anchor, nodes) in &groups {
                        graph.apply_splice_chain(anchor, nodes)?;
                    }
                    Ok(())
                })
                .map_err(|reason| PatchError::Apply {
                    patch: "graph.splice.lowering".into(),
                    reason,
                })?;
        }
        *assembly = draft;
        Ok(plan)
    }
    /// One-shot assembly: resolve once, atomically apply selected contributions,
    /// compile exactly once, and return an immutable graph ready for execution.
    pub fn build(
        self,
        name: impl Into<String>,
    ) -> Result<(AssembledGraph, PatchResolution), PatchError> {
        let mut assembly = GraphAssembly::new(name);
        let plan = self.apply(&mut assembly)?;
        let built = assembly.finish().map_err(PatchError::Compile)?;
        Ok((built, plan))
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[test]
    fn directional_coverage_is_not_conflict_overlap() {
        let node = PatchTarget::Node("worker".into());
        assert!(node.overlaps(&PatchTarget::Graph));
        assert!(!node.covers(&PatchTarget::Graph));
        assert!(PatchTarget::Graph.covers(&node));
        assert!(node.covers(&PatchTarget::Instance("worker".into())));
        assert!(!node.covers(&PatchTarget::Node("other".into())));
        let exact = PatchTarget::edge_nodes("a.->:b", "value", "sink", "run", "value");
        let pattern = ConnectionRoute {
            from: PortSelector {
                node: "a.->:b".into(),
                port: Some("value".into()),
                slot: None,
            },
            to: PortSelector {
                node: "sink".into(),
                port: Some("run".into()),
                slot: Some("value".into()),
            },
        };
        assert!(PatchTarget::ConnectionPattern(pattern.clone()).covers(&exact));
        assert!(PatchTarget::IncidentConnections("a.->:b".into()).covers(&exact));
        assert!(!PatchTarget::IncidentConnections("other".into()).covers(&exact));
        let boundary = PatchTarget::edge_node_to_instance("a.->:b", "value", "sink", "run");
        assert!(!PatchTarget::ConnectionPattern(pattern.clone()).covers(&boundary));
        let mut source_slot = pattern;
        source_slot.from.slot = Some("value".into());
        assert!(!PatchTarget::ConnectionPattern(source_slot).covers(&exact));
        let numeric = PatchTarget::connection(
            ModuleOutputSource::Node(OutputAddr {
                node: NodeId(1),
                slot: SlotId(0),
            }),
            ModuleInputTarget::Node(InputAddr {
                node: NodeId(2),
                port: PortId(0),
                slot: SlotId(0),
            }),
        );
        assert!(numeric.covers(&numeric));
        assert!(PatchTarget::Connections.covers(&numeric));
        assert!(!PatchTarget::IncidentConnections("1".into()).covers(&numeric));
        assert!(
            !PatchTarget::Splice(PatchAnchor::nodes(
                "a.->:b", "value", "sink", "run", "value"
            ))
            .covers(&exact)
        );
    }
}
