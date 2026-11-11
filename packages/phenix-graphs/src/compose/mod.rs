//! Owned declarative composition, independent of plugin and execution lifecycles.
//!
//! Grants authorize exact writes, not reads or semantic claims. The host must
//! construct registrations itself; accepting a registration from a plugin is not
//! a security boundary. Root topology edits remain public-only. A composite may
//! explicitly expose child replacement seams, still requiring exact host grants.
//!
//! # Selection and replay
//! Dependencies must exist and form a DAG. Transitive dependencies order writes;
//! independent overlapping writes with equal priorities are errors. Otherwise,
//! the lower-priority registration is defeated as a whole. Higher priorities
//! are selected first, so a loser cannot suppress another independent patch.
//! Missing selected prerequisites deactivate dependents for this resolution;
//! selection repeats without them so they cannot suppress eligible patches.
//! Priority is an explicit integer, not an insertion index or fractional splice
//! position. Independent splices of the same anchor are always ambiguous.
//!
//! # Exact authority
//! Component grants cover a single root declaration, connection grants cover
//! both symbolic endpoints (capacity is the value, not part of the key), export
//! grants cover one name and direction, and source grants cover one start name.
//! There are no implicit namespace, incident-edge, descendant, or claim grants.
//! Splices require the original anchor plus every removed/generated edge key;
//! `Composition::footprints` exposes the fully resolved selected writes.
//! `NetworkPatch::footprints` is context-free and cannot include a dependent
//! splice's predecessor edges. The host must approve these explicitly.
//! Payload origins must be absent or exactly the registration origin, including
//! inside composite definitions. Add/Replace stamp that exact origin only on the
//! declaration being written. Claims are never inferred, copied from the old
//! declaration, or treated as edit authority; canonical compilation validates
//! the complete candidate's contracts. Every applied Add/Replace checkpoints its
//! claims and private requests as final-state obligations, even when a later
//! edit in the same patch replaces that declaration again. Recomposition starts
//! these checkpoints afresh from the frozen base and selected edits.
use crate::component::{Body, ComponentDef};
use crate::network::{
    CompiledNetwork, ConnectionDef, InputProjection, NetworkDef, NetworkError, PortPath,
    collect_component_obligations,
};
use crate::{GraphOrigin, Interface};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionKey {
    pub from: PortPath,
    pub to: PortPath,
}
impl ConnectionKey {
    pub fn new(from: PortPath, to: PortPath) -> Self {
        Self { from, to }
    }
}

/// Replacement writes only `Component`, never its incident connections.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Footprint {
    Component(String),
    Connection(ConnectionKey),
    InputExport(String),
    OutputExport(String),
    Source(String),
}

#[derive(Clone, Debug)]
pub enum Edit {
    Add {
        path: String,
        definition: Arc<ComponentDef>,
    },
    Replace {
        path: String,
        definition: Arc<ComponentDef>,
    },
    Connect(ConnectionDef),
    Disconnect(ConnectionKey),
    ExposeInput {
        name: String,
        targets: Vec<PortPath>,
        capacity: NonZeroUsize,
    },
    ExposeOutput {
        name: String,
        from: PortPath,
    },
    AddSource(String),
    /// Insert an existing component between the original edge's endpoints.
    /// Dependent splices append in dependency order, retaining the original key.
    Splice {
        anchor: ConnectionKey,
        input: PortPath,
        output: PortPath,
    },
}
impl Edit {
    fn validate_payload(&self, origin: &GraphOrigin) -> Result<(), ComposeError> {
        match self {
            Self::Add { path, definition } => {
                public_path(path)?;
                definition_origin(definition, origin, path)?;
            }
            Self::Replace { path, definition } => {
                for part in path.split('/') {
                    public_path(part)?;
                }
                definition_origin(definition, origin, path)?;
            }
            Self::Connect(edge) => {
                public_path(&edge.from.process)?;
                public_path(&edge.to.process)?;
            }
            Self::Disconnect(key) => {
                public_path(&key.from.process)?;
                public_path(&key.to.process)?;
            }
            Self::ExposeInput { targets, .. } => {
                for target in targets {
                    public_path(&target.process)?;
                }
            }
            Self::ExposeOutput { from, .. } => public_path(&from.process)?,
            Self::AddSource(path) => public_path(path)?,
            Self::Splice {
                anchor,
                input,
                output,
            } => {
                for path in [&anchor.from, &anchor.to, input, output] {
                    public_path(&path.process)?;
                }
            }
        }
        Ok(())
    }

    pub fn footprints(&self) -> BTreeSet<Footprint> {
        use Footprint::*;
        match self {
            Self::Add { path, .. } | Self::Replace { path, .. } => [Component(path.clone())].into(),
            Self::Connect(edge) => [Connection(ConnectionKey::new(
                edge.from.clone(),
                edge.to.clone(),
            ))]
            .into(),
            Self::Disconnect(key) => [Connection(key.clone())].into(),
            Self::ExposeInput { name, .. } => [InputExport(name.clone())].into(),
            Self::ExposeOutput { name, .. } => [OutputExport(name.clone())].into(),
            Self::AddSource(path) => [Source(path.clone())].into(),
            // The planner adds the context-dependent predecessor edges.
            Self::Splice { anchor, output, .. } => [
                Connection(anchor.clone()),
                Connection(ConnectionKey::new(output.clone(), anchor.to.clone())),
            ]
            .into(),
        }
    }
}

/// Immutable owned payload. It contains no host authority or replay callbacks.
#[derive(Clone, Debug)]
pub struct NetworkPatch {
    edits: Arc<[Edit]>,
}
impl NetworkPatch {
    pub fn new(edits: impl IntoIterator<Item = Edit>) -> Self {
        Self {
            edits: edits.into_iter().collect(),
        }
    }
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }
    pub fn footprints(&self) -> BTreeSet<Footprint> {
        self.edits.iter().flat_map(Edit::footprints).collect()
    }
}

/// Trusted host envelope, deliberately separate from the patch payload.
#[derive(Clone, Debug)]
pub struct Registration {
    id: String,
    dependencies: BTreeSet<String>,
    priority: i64,
    origin: GraphOrigin,
    grants: BTreeSet<Footprint>,
    patch: NetworkPatch,
}
impl Registration {
    pub fn new(
        id: impl Into<String>,
        dependencies: impl IntoIterator<Item = String>,
        priority: i64,
        origin: GraphOrigin,
        grants: impl IntoIterator<Item = Footprint>,
        patch: NetworkPatch,
    ) -> Self {
        Self {
            id: id.into(),
            dependencies: dependencies.into_iter().collect(),
            priority,
            origin,
            grants: grants.into_iter().collect(),
            patch,
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn patch(&self) -> &NetworkPatch {
        &self.patch
    }
    pub fn dependencies(&self) -> &BTreeSet<String> {
        &self.dependencies
    }
    pub fn priority(&self) -> i64 {
        self.priority
    }
    pub fn origin(&self) -> &GraphOrigin {
        &self.origin
    }
    pub fn grants(&self) -> &BTreeSet<Footprint> {
        &self.grants
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposeError {
    DuplicateRegistration(String),
    MissingDependency {
        registration: String,
        dependency: String,
    },
    DependencyCycle,
    Conflict {
        first: String,
        second: String,
        footprint: Box<Footprint>,
    },
    Unauthorized {
        registration: String,
        footprint: Footprint,
    },
    PrivateEditScope(String),
    ForgedOrigin(String),
    InvalidEdit(String),
    Network(NetworkError),
    Panicked,
}
impl From<NetworkError> for ComposeError {
    fn from(error: NetworkError) -> Self {
        Self::Network(error)
    }
}
impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "composition: {self:?}")
    }
}
impl std::error::Error for ComposeError {}

/// Published immutable candidate and its complete compiled topology.
#[derive(Clone, Debug)]
pub struct Composition {
    network: NetworkDef,
    compiled: CompiledNetwork,
    active: BTreeSet<String>,
    footprints: BTreeMap<String, BTreeSet<Footprint>>,
}
impl Composition {
    pub fn network(&self) -> &NetworkDef {
        &self.network
    }
    /// Native topology with authoritative contract binding and `inspect(path)`
    /// exposing exact declared origins separately from inherited provenance.
    pub fn compiled(&self) -> &CompiledNetwork {
        &self.compiled
    }
    pub fn active(&self) -> &BTreeSet<String> {
        &self.active
    }
    /// Fully resolved exact writes of the selected plan, including splice edges.
    pub fn footprints(&self) -> &BTreeMap<String, BTreeSet<Footprint>> {
        &self.footprints
    }
}

/// Every update replays the base and selected plan. Failed updates retain both
/// registry and published topology. No factories are invoked, and no external
/// side-effect rollback is promised. Unwinding panics become `Panicked`.
pub struct Composer {
    base: NetworkDef,
    registrations: BTreeMap<String, Registration>,
    current: Composition,
}
impl Composer {
    pub fn new(base: NetworkDef) -> Result<Self, ComposeError> {
        let current = guarded_compose(&base, &BTreeMap::new())?;
        Ok(Self {
            base,
            registrations: BTreeMap::new(),
            current,
        })
    }
    pub fn current(&self) -> &Composition {
        &self.current
    }
    /// Batch admission permits dependencies to be supplied in any order.
    pub fn register(
        &mut self,
        registrations: impl IntoIterator<Item = Registration>,
    ) -> Result<&Composition, ComposeError> {
        let mut candidate = self.registrations.clone();
        for registration in registrations {
            if candidate.contains_key(&registration.id) {
                return Err(ComposeError::DuplicateRegistration(registration.id));
            }
            candidate.insert(registration.id.clone(), registration);
        }
        let current = guarded_compose(&self.base, &candidate)?;
        self.registrations = candidate;
        self.current = current;
        Ok(&self.current)
    }
    /// Remove the prerequisite and its transitive dependents, then rebuild.
    /// Returns all explicitly deactivated registration IDs, including losers.
    pub fn revoke(&mut self, id: &str) -> Result<BTreeSet<String>, ComposeError> {
        let mut removed = BTreeSet::new();
        if self.registrations.contains_key(id) {
            removed.insert(id.to_owned());
        }
        loop {
            let before = removed.len();
            for (id, registration) in &self.registrations {
                if !registration.dependencies.is_disjoint(&removed) {
                    removed.insert(id.clone());
                }
            }
            if before == removed.len() {
                break;
            }
        }
        let candidate = self
            .registrations
            .iter()
            .filter(|(id, _)| !removed.contains(*id))
            .map(|(id, registration)| (id.clone(), registration.clone()))
            .collect();
        let current = guarded_compose(&self.base, &candidate)?;
        self.registrations = candidate;
        self.current = current;
        Ok(removed)
    }
}

fn guarded_compose(
    base: &NetworkDef,
    registrations: &BTreeMap<String, Registration>,
) -> Result<Composition, ComposeError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compose(base, registrations)
    }))
    .unwrap_or(Err(ComposeError::Panicked))
}

fn compose(
    base: &NetworkDef,
    registrations: &BTreeMap<String, Registration>,
) -> Result<Composition, ComposeError> {
    let mut ancestors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut order = Vec::new();
    while order.len() < registrations.len() {
        let mut progress = false;
        for (id, registration) in registrations {
            for dependency in &registration.dependencies {
                if !registrations.contains_key(dependency) {
                    return Err(ComposeError::MissingDependency {
                        registration: id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
            if ancestors.contains_key(id)
                || !registration
                    .dependencies
                    .iter()
                    .all(|d| ancestors.contains_key(d))
            {
                continue;
            }
            let mut all = registration.dependencies.clone();
            for dependency in &registration.dependencies {
                all.extend(ancestors[dependency].iter().cloned());
            }
            ancestors.insert(id.clone(), all);
            order.push(id.clone());
            progress = true;
        }
        if !progress {
            return Err(ComposeError::DependencyCycle);
        }
    }
    let mut footprints = BTreeMap::new();
    for id in &order {
        for edit in registrations[id].patch.edits() {
            edit.validate_payload(&registrations[id].origin)?;
        }
        let mut writes = registrations[id].patch.footprints();
        let mut tails = BTreeMap::new();
        for predecessor in order.iter().filter(|other| ancestors[id].contains(*other)) {
            for edit in registrations[predecessor].patch.edits() {
                if let Edit::Splice { anchor, output, .. } = edit {
                    tails.insert(anchor.clone(), output.clone());
                }
            }
        }
        for edit in registrations[id].patch.edits() {
            if let Edit::Splice {
                anchor,
                input,
                output,
            } = edit
            {
                let from = tails.get(anchor).unwrap_or(&anchor.from).clone();
                writes.insert(Footprint::Connection(ConnectionKey::new(
                    from.clone(),
                    anchor.to.clone(),
                )));
                writes.insert(Footprint::Connection(ConnectionKey::new(
                    from,
                    input.clone(),
                )));
                tails.insert(anchor.clone(), output.clone());
            }
        }
        for footprint in &writes {
            if !registrations[id].grants.contains(footprint) {
                return Err(ComposeError::Unauthorized {
                    registration: id.clone(),
                    footprint: footprint.clone(),
                });
            }
        }
        footprints.insert(id.clone(), writes);
    }
    // Fail closed on ambiguities, even when a third registration could defeat
    // one participant. Dependency ordering is the only splice ordering key.
    for (i, a) in order.iter().enumerate() {
        for b in &order[i + 1..] {
            if ancestors[b].contains(a) || ancestors[a].contains(b) {
                continue;
            }
            let left = &footprints[a];
            let right = &footprints[b];
            // Check all anchors, not just the first colliding footprint: the
            // patches may also collide on an added component declaration.
            for edit in registrations[a].patch.edits() {
                if let Edit::Splice { anchor, .. } = edit
                    && registrations[b].patch.edits().iter().any(|other|
                        matches!(other, Edit::Splice { anchor: other, .. } if other == anchor)) {
                    return Err(ComposeError::Conflict {
                        first: a.clone(), second: b.clone(),
                        footprint: Box::new(Footprint::Connection(anchor.clone())),
                    });
                }
            }
            if let Some(footprint) = overlapping_write(left, right)
                && registrations[a].priority == registrations[b].priority
            {
                return Err(ComposeError::Conflict {
                    first: a.clone(),
                    second: b.clone(),
                    footprint: Box::new(footprint.clone()),
                });
            }
        }
    }
    let mut candidates = order.clone();
    candidates.sort_by(|a, b| {
        registrations[b]
            .priority
            .cmp(&registrations[a].priority)
            .then_with(|| a.cmp(b))
    });
    let mut deactivated = BTreeSet::new();
    let active = loop {
        let mut selected = BTreeSet::new();
        for id in &candidates {
            if deactivated.contains(id) {
                continue;
            }
            let loses = selected.iter().any(|winner| {
                !ancestors[id].contains(winner)
                    && !ancestors[winner].contains(id)
                    && overlapping_write(&footprints[id], &footprints[winner]).is_some()
            });
            if !loses {
                selected.insert(id.clone());
            }
        }
        let before = deactivated.len();
        for id in &order {
            if !ancestors[id].is_subset(&selected) {
                deactivated.insert(id.clone());
            }
        }
        if before == deactivated.len() {
            break selected;
        }
    };
    let mut network = base.clone();
    let mut splices = BTreeMap::new();
    for id in order.iter().filter(|id| active.contains(*id)) {
        let registration = &registrations[id];
        for edit in registration.patch.edits() {
            replay(&mut network, edit, registration, &mut splices)?;
        }
    }
    network
        .connections
        .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    network.sources.sort();
    let compiled = network.compile()?;
    footprints.retain(|id, _| active.contains(id));
    Ok(Composition {
        network,
        compiled,
        active,
        footprints,
    })
}

/// Exact grants remain separate. Selection conflict is broader: replacing an
/// ancestor composite changes the definition containing every nested child.
/// Independently selected ancestor and descendant replacements would otherwise
/// silently erase each other depending on lexical replay order.
fn overlapping_write<'a>(
    left: &'a BTreeSet<Footprint>,
    right: &BTreeSet<Footprint>,
) -> Option<&'a Footprint> {
    left.iter().find(|write| {
        right.iter().any(|other| {
            if write == other {
                return true;
            }
            match (write, other) {
                (Footprint::Component(a), Footprint::Component(b)) => {
                    b.strip_prefix(a).is_some_and(|suffix| suffix.starts_with('/'))
                        || a.strip_prefix(b).is_some_and(|suffix| suffix.starts_with('/'))
                }
                _ => false,
            }
        })
    })
}

fn public_path(path: &str) -> Result<(), ComposeError> {
    if path.is_empty()
        || path.contains('/')
        || matches!(path, "." | "..")
        || path.chars().any(char::is_control)
    {
        Err(ComposeError::PrivateEditScope(path.into()))
    } else {
        Ok(())
    }
}
fn definition_origin(
    definition: &ComponentDef,
    origin: &GraphOrigin,
    path: &str,
) -> Result<(), ComposeError> {
    let limits = crate::CompileLimits::default();
    let mut remaining = limits.max_instances;
    let mut pending = vec![(definition, path.to_owned(), 0)];
    while let Some((definition, path, depth)) = pending.pop() {
        if remaining == 0 || depth >= limits.max_depth {
            return Err(NetworkError::LimitExceeded {
                limit: "composition origin scan",
                path,
            }
            .into());
        }
        remaining -= 1;
        if definition.origin().is_some_and(|actual| actual != origin) {
            return Err(ComposeError::ForgedOrigin(path));
        }
        if let Body::Composite(child) = definition.body() {
            if child.instances().len() as u64 > remaining {
                return Err(NetworkError::LimitExceeded {
                    limit: "composition origin scan",
                    path,
                }
                .into());
            }
            for (name, child) in child.instances() {
                pending.push((child, format!("{path}/{name}"), depth + 1));
            }
        }
    }
    Ok(())
}
fn replay(
    network: &mut NetworkDef,
    edit: &Edit,
    registration: &Registration,
    splices: &mut BTreeMap<ConnectionKey, (PortPath, NonZeroUsize)>,
) -> Result<(), ComposeError> {
    let invalid = || ComposeError::InvalidEdit(format!("{}: {edit:?}", registration.id));
    match edit {
        Edit::Add { path, definition } | Edit::Replace { path, definition } => {
            if matches!(edit, Edit::Replace { .. }) && path.contains('/') {
                replace_nested(network, path, definition, &registration.origin)?;
                let root = path.split('/').next().unwrap();
                collect_component_obligations(
                    &network.instances[root],
                    root,
                    &mut network.requirements,
                );
                return Ok(());
            }
            if matches!(edit, Edit::Add { .. }) {
                if network.instances.contains_key(path) {
                    return Err(invalid());
                }
            } else {
                let old = network.instances.get(path).ok_or_else(invalid)?;
                if old.interface() != definition.interface() {
                    return Err(NetworkError::InterfaceMismatch(path.clone()).into());
                }
            }
            network.instances.insert(
                path.clone(),
                Arc::new(
                    (**definition)
                        .clone()
                        .with_origin(registration.origin.clone()),
                ),
            );
            collect_component_obligations(
                &network.instances[path],
                path,
                &mut network.requirements,
            );
        }
        Edit::Connect(edge) => {
            if network
                .connections
                .iter()
                .any(|e| e.from == edge.from && e.to == edge.to)
            {
                return Err(invalid());
            }
            network.connections.push(edge.clone());
        }
        Edit::Disconnect(key) => {
            let index = network
                .connections
                .iter()
                .position(|e| e.from == key.from && e.to == key.to)
                .ok_or_else(invalid)?;
            network.connections.remove(index);
        }
        Edit::ExposeInput {
            name,
            targets,
            capacity,
        } => {
            if network.inputs.contains_key(name) {
                return Err(invalid());
            }
            let target = targets.first().ok_or_else(invalid)?;
            let mut port = network
                .instances
                .get(&target.process)
                .and_then(|c| c.interface().input(&target.port))
                .ok_or_else(invalid)?
                .clone();
            port.name = name.clone();
            network.interface = Arc::new(
                Interface::new(
                    network.interface.inputs().values().cloned().chain([port]),
                    network.interface.outputs().values().cloned(),
                )
                .map_err(NetworkError::from)?,
            );
            network.inputs.insert(
                name.clone(),
                InputProjection {
                    targets: targets.clone(),
                    capacity: *capacity,
                },
            );
        }
        Edit::ExposeOutput { name, from } => {
            if network.outputs.contains_key(name) {
                return Err(invalid());
            }
            let mut port = network
                .instances
                .get(&from.process)
                .and_then(|c| c.interface().output(&from.port))
                .ok_or_else(invalid)?
                .clone();
            port.name = name.clone();
            network.interface = Arc::new(
                Interface::new(
                    network.interface.inputs().values().cloned(),
                    network.interface.outputs().values().cloned().chain([port]),
                )
                .map_err(NetworkError::from)?,
            );
            network.outputs.insert(name.clone(), from.clone());
        }
        Edit::AddSource(path) => {
            if network.sources.contains(path) {
                return Err(invalid());
            }
            network.sources.push(path.clone());
        }
        Edit::Splice {
            anchor,
            input,
            output,
        } => {
            let predecessor = splices
                .get(anchor)
                .map_or(&anchor.from, |(tail, _)| tail)
                .clone();
            let index = network
                .connections
                .iter()
                .position(|e| e.from == predecessor && e.to == anchor.to)
                .ok_or_else(invalid)?;
            let capacity = network.connections[index].capacity;
            for key in [
                ConnectionKey::new(predecessor.clone(), input.clone()),
                ConnectionKey::new(output.clone(), anchor.to.clone()),
            ] {
                let footprint = Footprint::Connection(key);
                if !registration.grants.contains(&footprint) {
                    return Err(ComposeError::Unauthorized {
                        registration: registration.id.clone(),
                        footprint,
                    });
                }
            }
            network.connections.remove(index);
            network.connections.push(ConnectionDef {
                from: predecessor,
                to: input.clone(),
                capacity,
            });
            network.connections.push(ConnectionDef {
                from: output.clone(),
                to: anchor.to.clone(),
                capacity,
            });
            splices.insert(anchor.clone(), (output.clone(), capacity));
        }
    }
    Ok(())
}

/// Copy-on-write replacement through explicit *immediate-child* seams. A
/// root-level host grant never bypasses a composite author's private boundary.
fn replace_nested(
    network: &mut NetworkDef,
    path: &str,
    definition: &Arc<ComponentDef>,
    origin: &GraphOrigin,
) -> Result<(), ComposeError> {
    let (parent, remainder) = path
        .split_once('/')
        .ok_or_else(|| ComposeError::PrivateEditScope(path.into()))?;
    let ancestor = network
        .instances
        .get(parent)
        .ok_or_else(|| ComposeError::InvalidEdit(format!("unknown composite {parent}")))?
        .clone();
    let Body::Composite(child) = ancestor.body() else {
        return Err(ComposeError::PrivateEditScope(path.into()));
    };
    let immediate = remainder.split('/').next().unwrap();
    if !child.replacement_points.contains(immediate) {
        return Err(ComposeError::PrivateEditScope(path.into()));
    }
    let mut updated = (**child).clone();
    if remainder.contains('/') {
        replace_nested(&mut updated, remainder, definition, origin)?;
    } else {
        let previous = updated.instances.get(remainder).ok_or_else(|| {
            ComposeError::InvalidEdit(format!("unknown nested component {path}"))
        })?;
        if previous.interface() != definition.interface() {
            return Err(NetworkError::InterfaceMismatch(path.into()).into());
        }
        updated.instances.insert(
            remainder.into(),
            Arc::new((**definition).clone().with_origin(origin.clone())),
        );
        collect_component_obligations(
            &updated.instances[remainder],
            remainder,
            &mut updated.requirements,
        );
    }
    network.instances.insert(
        parent.into(),
        Arc::new(ancestor.with_updated_network(updated)),
    );
    Ok(())
}
