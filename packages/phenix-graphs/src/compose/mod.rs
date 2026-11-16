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
//! a registration is selected exactly when its direct dependencies are selected
//! and no independent, overlapping, higher-priority registration is selected.
//! Selection must have exactly one consistent solution; no solution, ambiguity,
//! or exhaustion of the finite selection budget rejects the entire update.
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
use crate::GraphOrigin;
use crate::network::{CompiledNetwork, NetworkDef, NetworkError, normalize_requirements};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod edit;
mod planning;
mod replay;
mod selection;

pub use edit::{ConnectionKey, Edit, Footprint, NetworkPatch};

/// Finite work allowance for exact registration selection on each update.
///
/// The default is 1,000,000 work units. Units cover selector index and adjacency
/// construction, propagation, and the complete search proving uniqueness. Each
/// initialized equation slot or ID-index entry, dependency lookup, constructed
/// edge, seed inspection, queue push/pop (including discarded entries), assignment,
/// traversed forward/reverse edge, undone trail entry, inspected search slot,
/// search-frame creation/visit, and captured model entry costs one unit.
/// Registry cloning, dependency/ancestor planning, footprint and pairwise conflict
/// validation/blocker collection, selected-ID materialization, replay, compilation,
/// and allocation failure are outside this allowance. It is not a bound on total
/// composition work. An empty selection costs zero units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionLimits {
    pub max_work: usize,
}
impl Default for SelectionLimits {
    fn default() -> Self {
        Self {
            max_work: 1_000_000,
        }
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
    NoConsistentSelection,
    AmbiguousSelection,
    SelectionBudgetExceeded {
        limit: usize,
    },
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
impl std::error::Error for ComposeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(error) => Some(error),
            _ => None,
        }
    }
}

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
    selection_limits: SelectionLimits,
}
impl Composer {
    pub fn new(base: NetworkDef) -> Result<Self, ComposeError> {
        let selection_limits = SelectionLimits::default();
        let current = guarded_compose(&base, &BTreeMap::new(), selection_limits)?;
        Ok(Self {
            base,
            registrations: BTreeMap::new(),
            current,
            selection_limits,
        })
    }
    pub fn current(&self) -> &Composition {
        &self.current
    }
    /// Apply limits to future updates only, without recomposing or publishing.
    pub fn set_selection_limits(&mut self, limits: SelectionLimits) {
        self.selection_limits = limits;
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
        let current = guarded_compose(&self.base, &candidate, self.selection_limits)?;
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
        let current = guarded_compose(&self.base, &candidate, self.selection_limits)?;
        self.registrations = candidate;
        self.current = current;
        Ok(removed)
    }
}

fn guarded_compose(
    base: &NetworkDef,
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
) -> Result<Composition, ComposeError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compose(base, registrations, selection_limits)
    })) {
        Ok(result) => result,
        Err(payload) => {
            crate::discard_panic_payload(payload);
            Err(ComposeError::Panicked)
        }
    }
}

fn compose(
    base: &NetworkDef,
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
) -> Result<Composition, ComposeError> {
    let plan = planning::plan(registrations, selection_limits)?;
    let mut network = replay::replay_selected(base, registrations, &plan)?;
    network
        .connections
        .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    network.sources.sort();
    normalize_requirements(&mut network.requirements);
    let compiled = network.compile()?;
    let planning::SelectedPlan {
        active,
        mut footprints,
        ..
    } = plan;
    footprints.retain(|id, _| active.contains(id));
    Ok(Composition {
        network,
        compiled,
        active,
        footprints,
    })
}
