//! Owned declarative composition, independent of plugin and execution lifecycles.
//!
//! Grants authorize exact writes, not reads or semantic claims. The host must
//! construct registrations itself; accepting a registration from a plugin is not
//! a security boundary. Root exports and source edits remain public-only.
//! A composite may separately opt into internal nested Add/Connect extensions
//! or expose specific child replacement seams; both require exact host grants.
//! Every ancestor on a nested extension path must explicitly opt in.
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

mod budget;
mod edit;
mod planning;
mod replay;
mod selection;

use budget::PlanningBudget;
pub use edit::{ConnectionKey, Edit, Footprint, NetworkPatch};

/// Finite logical composition planning allowance, independent of selection.
///
/// The default 1,000,000 units is a new host resource policy: previously valid
/// unbounded inputs may now require a larger finite allowance. Each explicit
/// planning visit/lookup and constructed scalar slot costs one unit; symbolic
/// query/copy/scan bytes cost one unit each plus one operation unit. Predicate
/// operands and possible diagnostic copies are reserved before the decision.
/// Blockers reserve two index cells; ancestor masks reserve their logical length
/// as Boolean cells. Reservations are cumulative and never refunded.
/// BTree-internal comparisons, allocation rounding/failure, opaque payloads,
/// incoming input construction/iterator code, replay, network cloning, compiler
/// and arbitrary native callbacks/destructors are not bounded. This is neither a
/// wall-clock/byte cap nor a total build-work allowance. Empty planning is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanningLimits {
    pub max_units: usize,
}
impl Default for PlanningLimits {
    fn default() -> Self {
        Self {
            max_units: 1_000_000,
        }
    }
}

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

/// Replay failures retain only the affected symbolic metadata, never an edit or factory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvalidEditReason {
    DuplicateComponent {
        path: String,
    },
    MissingReplacement {
        path: String,
    },
    DuplicateConnection {
        connection: ConnectionKey,
    },
    MissingDisconnection {
        connection: ConnectionKey,
    },
    DuplicateInputExport {
        name: String,
    },
    EmptyInputProjection {
        name: String,
    },
    MissingInputComponent {
        name: String,
        target: crate::network::PortPath,
    },
    MissingInputPort {
        name: String,
        target: crate::network::PortPath,
    },
    DuplicateOutputExport {
        name: String,
    },
    MissingOutputComponent {
        name: String,
        target: crate::network::PortPath,
    },
    MissingOutputPort {
        name: String,
        target: crate::network::PortPath,
    },
    DuplicateSource {
        path: String,
    },
    MissingSpliceConnection {
        anchor: ConnectionKey,
        connection: ConnectionKey,
    },
    UnknownComposite {
        path: String,
    },
    UnknownNestedComponent {
        path: String,
    },
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
    PlanningBudgetExceeded {
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
    InvalidEdit {
        registration: String,
        reason: Box<InvalidEditReason>,
    },
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

/// Every update validates a full candidate plan and issues a new compilation
/// identity. An unchanged selected topology may reuse compiled lowering, but
/// generation-bound handles are always invalidated on successful publication.
/// Failed updates retain the previous registry, topology, and identity. No
/// factories are invoked or external side-effect rollback promised. Panics are
/// converted to `Panicked` before publication. Retired-state cleanup panics after
/// commit are suppressed: a published update remains successful. Abort, double
/// panic and nontermination are outside the native unwind boundary.
pub struct Composer {
    base: NetworkDef,
    registrations: BTreeMap<String, Registration>,
    current: Composition,
    selection_limits: SelectionLimits,
    planning_limits: PlanningLimits,
}
impl Composer {
    pub fn new(base: NetworkDef) -> Result<Self, ComposeError> {
        let selection_limits = SelectionLimits::default();
        let planning_limits = PlanningLimits::default();
        let mut used = 0;
        let mut budget = PlanningBudget::new(planning_limits, &mut used)?;
        let current =
            match guarded(|| compose(&base, &BTreeMap::new(), selection_limits, None, &mut budget))
            {
                Ok(current) => current,
                Err(error) => {
                    dispose(base);
                    return Err(error);
                }
            };
        Ok(Self {
            base,
            registrations: BTreeMap::new(),
            current,
            selection_limits,
            planning_limits,
        })
    }
    pub fn current(&self) -> &Composition {
        &self.current
    }
    /// Apply limits to future updates only, without recomposing or publishing.
    pub fn set_selection_limits(&mut self, limits: SelectionLimits) {
        self.selection_limits = limits;
    }
    /// Apply the logical planning policy to future updates without publication.
    pub fn set_planning_limits(&mut self, limits: PlanningLimits) {
        self.planning_limits = limits;
    }
    /// Batch admission permits dependencies to be supplied in any order.
    pub fn register(
        &mut self,
        registrations: impl IntoIterator<Item = Registration>,
    ) -> Result<&Composition, ComposeError> {
        self.register_with_planning_usage(registrations, &mut 0)
    }

    /// Admit using a caller-owned cumulative planning counter. Consumed units
    /// survive failure; the borrow is not retained. All Composers in one host
    /// build (including self-previews) must use the same counter and limit.
    /// The first denied reservation wins; with sufficient allowance semantic
    /// error order is unchanged. Cleanup never replaces a conclusive error.
    pub fn register_with_planning_usage(
        &mut self,
        registrations: impl IntoIterator<Item = Registration>,
        used_units: &mut usize,
    ) -> Result<&Composition, ComposeError> {
        let mut input = Some(registrations);
        let mut iterator = None;
        let mut pending = None;
        let mut candidate = BTreeMap::new();
        // Keep user-owned cleanup outside the unwind closure. In particular,
        // iterator Drop must finish before either published field is replaced.
        let mut result = guarded(|| {
            let mut budget = PlanningBudget::new(self.planning_limits, used_units)?;
            iterator = Some(input.take().expect("input").into_iter());
            for (id, registration) in &self.registrations {
                budget.text(id)?;
                budget.text(id)?;
                budget.registration_copy(registration)?;
                candidate.insert(id.clone(), registration.clone());
            }
            loop {
                pending = iterator.as_mut().expect("iterator").next();
                let Some(registration) = pending.as_ref() else {
                    break;
                };
                // Reserve query, possible diagnostic and inserted key before
                // deciding duplicate admission.
                budget.text(&registration.id)?;
                budget.text(&registration.id)?;
                budget.text(&registration.id)?;
                budget.charge(1)?;
                if candidate.contains_key(&registration.id) {
                    return Err(ComposeError::DuplicateRegistration(registration.id.clone()));
                }
                let registration = pending.take().expect("pending registration");
                candidate.insert(registration.id.clone(), registration);
            }
            compose(
                &self.base,
                &candidate,
                self.selection_limits,
                Some(&self.current),
                &mut budget,
            )
        });
        let clean = dispose(iterator) & dispose(input) & dispose(pending);
        if !clean && result.is_ok() {
            dispose(result);
            result = Err(ComposeError::Panicked);
        }
        match result {
            Ok(current) => {
                self.commit(candidate, current);
                Ok(&self.current)
            }
            Err(error) => {
                dispose(candidate);
                Err(error)
            }
        }
    }
    /// Remove the prerequisite and its transitive dependents, then rebuild.
    /// Returns all explicitly deactivated registration IDs, including losers.
    pub fn revoke(&mut self, id: &str) -> Result<BTreeSet<String>, ComposeError> {
        let mut candidate = BTreeMap::new();
        let result = guarded(|| {
            let mut used = 0;
            let mut budget = PlanningBudget::new(self.planning_limits, &mut used)?;
            let removed = dependent_closure(&self.registrations, id, &mut budget)?;
            for (id, registration) in &self.registrations {
                budget.text(id)?;
                if !removed.contains(id) {
                    budget.text(id)?;
                    budget.text(id)?;
                    budget.registration_copy(registration)?;
                    candidate.insert(id.clone(), registration.clone());
                }
            }
            let current = compose(
                &self.base,
                &candidate,
                self.selection_limits,
                Some(&self.current),
                &mut budget,
            )?;
            Ok((current, removed))
        });
        match result {
            Ok((current, removed)) => {
                self.commit(candidate, current);
                Ok(removed)
            }
            Err(error) => {
                dispose(candidate);
                Err(error)
            }
        }
    }

    fn commit(&mut self, candidate: BTreeMap<String, Registration>, current: Composition) {
        let retired_registry = std::mem::replace(&mut self.registrations, candidate);
        let retired_current = std::mem::replace(&mut self.current, current);
        // Publication is complete. A catchable retired Drop panic cannot turn
        // a successful atomic update into a reported precommit failure.
        dispose(retired_current);
        dispose(retired_registry);
    }
}

/// Collect exact transitive dependents with one reverse-index traversal.
/// Only the ephemeral per-update inventory is examined; publication remains
/// atomic under guarded_compose.
fn dependent_closure(
    registrations: &BTreeMap<String, Registration>,
    root: &str,
    budget: &mut PlanningBudget<'_>,
) -> Result<BTreeSet<String>, ComposeError> {
    if registrations.is_empty() {
        return Ok(BTreeSet::new());
    }
    budget.text(root)?;
    if !registrations.contains_key(root) {
        return Ok(BTreeSet::new());
    }
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, registration) in registrations {
        budget.charge(1)?;
        for dependency in &registration.dependencies {
            budget.text(dependency)?;
            budget.charge(2)?;
            dependents
                .entry(dependency.as_str())
                .or_default()
                .push(id.as_str());
        }
    }
    let mut removed = BTreeSet::new();
    budget.charge(1)?;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        budget.charge(1)?;
        budget.text(id)?;
        budget.text(id)?;
        budget.charge(1)?;
        if !removed.insert(id.to_owned()) {
            continue;
        }
        budget.text(id)?;
        if let Some(children) = dependents.get(id) {
            budget.charge(children.len())?;
            stack.extend(children.iter().copied());
        }
    }
    Ok(removed)
}

fn guarded<T>(operation: impl FnOnce() -> Result<T, ComposeError>) -> Result<T, ComposeError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(payload) => {
            crate::discard_panic_payload(payload);
            Err(ComposeError::Panicked)
        }
    }
}

fn dispose(value: impl Sized) -> bool {
    guarded(|| {
        drop(value);
        Ok(())
    })
    .is_ok()
}

fn compose(
    base: &NetworkDef,
    registrations: &BTreeMap<String, Registration>,
    selection_limits: SelectionLimits,
    previous: Option<&Composition>,
    budget: &mut PlanningBudget<'_>,
) -> Result<Composition, ComposeError> {
    let mut plan = planning::plan(registrations, selection_limits, budget)?;
    // Reserve/filter the final manifest before replay or any reuse return.
    for id in plan.footprints.keys() {
        budget.text(id)?;
    }
    plan.footprints.retain(|id, _| plan.active.contains(id));
    // Composer edits are monotonic per operation: a registration can only be
    // added under a fresh ID or removed together with its dependents; an
    // existing registration and the frozen base cannot be replaced in place.
    // If all *writing* registrations remain selected, their edits and relative
    // dependency replay order are unchanged. Selected zero-edit registrations
    // can be added/removed without changing the network or its code leases.
    //
    // Run the complete planner first: even a losing or empty registration may
    // be unauthorized, cyclic, or make global selection ambiguous. Publish the
    // *new* active manifest and exact footprints with a fresh generation
    // identity, so old bound handles become stale even when the topology is
    // unchanged. Other selected write-set changes are replayed and compiled,
    // except the strictly checked source-pruning case below.
    if let Some(previous) = previous {
        for id in previous.footprints.keys() {
            budget.text(id)?;
            budget.text(id)?;
            budget.charge(1)?;
        }
        for id in &plan.active {
            budget.text(id)?;
            budget.text(id)?;
            budget.text(id)?;
            budget.charge(1)?;
        }
        let before: BTreeSet<&str> = previous
            .footprints
            .iter()
            .filter(|(_, writes)| !writes.is_empty())
            .map(|(id, _)| id.as_str())
            .collect();
        let after: BTreeSet<&str> = plan
            .active
            .iter()
            .filter(|id| !plan.footprints[*id].is_empty())
            .map(String::as_str)
            .collect();
        if before == after {
            return Ok(Composition {
                network: previous.network.clone(),
                compiled: previous.compiled.with_fresh_generation(),
                active: plan.active,
                footprints: plan.footprints,
            });
        }
    }
    let mut network = replay::replay_selected(base, registrations, &plan)?;
    network
        .connections
        .sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    network.sources.sort();
    normalize_requirements(&mut network.requirements);
    // A source-only removal is strictly monotone: all other validated
    // declarations and projections are identical, and the original successful
    // compilation already paid the compiler's work/instance/depth limits.
    // This still publishes a *fresh* generation with stale prior bindings.
    let compiled = match previous.and_then(|previous| {
        previous
            .compiled
            .reuse_after_source_pruning(&previous.network, &network)
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_root_primitive_replacements(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_root_connection_pruning(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_root_connection_capacity_changes(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_root_connection_additions(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_isolated_root_removal(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_batch_isolated_root_removal(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_isolated_root_addition(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_connected_root_addition(&previous.network, &network)
            })
            .or_else(|| {
                previous
                    .compiled
                    .reuse_after_connected_root_removal(&previous.network, &network)
            })
    }) {
        Some(compiled) => compiled,
        None => network.compile()?,
    };
    let planning::SelectedPlan {
        active, footprints, ..
    } = plan;
    Ok(Composition {
        network,
        compiled,
        active,
        footprints,
    })
}

#[cfg(test)]
mod dependent_closure_tests {
    use super::*;

    fn dependent_closure(
        registrations: &BTreeMap<String, Registration>,
        root: &str,
    ) -> BTreeSet<String> {
        super::dependent_closure(
            registrations,
            root,
            &mut PlanningBudget::new(
                PlanningLimits {
                    max_units: usize::MAX,
                },
                &mut 0,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn registration(id: &str, dependencies: Vec<String>) -> Registration {
        Registration::new(
            id,
            dependencies,
            0,
            GraphOrigin::default(),
            Vec::<Footprint>::new(),
            NetworkPatch::new(Vec::<Edit>::new()),
        )
    }

    #[test]
    fn revocation_includes_transitive_diamond_and_preserves_unrelated_nodes() {
        let registrations = BTreeMap::from([
            ("a".into(), registration("a", Vec::new())),
            ("b".into(), registration("b", vec!["a".into()])),
            ("c".into(), registration("c", vec!["a".into()])),
            ("d".into(), registration("d", vec!["b".into(), "c".into()])),
            ("e".into(), registration("e", Vec::new())),
            ("f".into(), registration("f", vec!["e".into()])),
        ]);
        assert_eq!(
            dependent_closure(&registrations, "a"),
            BTreeSet::from(["a".into(), "b".into(), "c".into(), "d".into()])
        );
        assert_eq!(
            dependent_closure(&registrations, "e"),
            BTreeSet::from(["e".into(), "f".into()])
        );
        assert!(dependent_closure(&registrations, "absent").is_empty());
    }

    #[test]
    fn reversed_lexical_chain_needs_no_global_rescan_or_recursion() {
        let registrations = (0..256)
            .map(|index| {
                let dependencies = if index == 255 {
                    Vec::new()
                } else {
                    vec![format!("{:04}", index + 1)]
                };
                let id = format!("{index:04}");
                (id.clone(), registration(&id, dependencies))
            })
            .collect();
        let removed = dependent_closure(&registrations, "0255");
        assert_eq!(removed.len(), 256);
    }
}

#[cfg(test)]
mod construction_cleanup_tests {
    use super::*;
    use crate::{ComponentDef, NetworkBuilder};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn rejected_base_cleanup_cannot_replace_a_conclusive_compile_error() {
        struct Bomb(Arc<AtomicUsize>);
        impl Drop for Bomb {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("base cleanup");
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let bomb = Bomb(drops.clone());
        let mut builder = NetworkBuilder::new();
        builder
            .add(
                "node",
                Arc::new(ComponentDef::map(move |n: i64| {
                    let _ = &bomb;
                    n
                })),
            )
            .unwrap();
        let mut base = builder.build().unwrap();
        base.sources.push("missing".into());
        assert!(matches!(Composer::new(base), Err(ComposeError::Network(_))));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod unchanged_selection_tests {
    use super::*;
    use crate::{component::ComponentDef, contract::Contract, network::NetworkBuilder};
    use std::sync::Arc;

    fn identity_contract() -> Contract {
        let interface = ComponentDef::map(|n: i64| n).interface().clone();
        Contract::new("compose/identity", interface).unwrap()
    }

    fn add(path: &str) -> NetworkPatch {
        NetworkPatch::new([Edit::Add {
            path: path.into(),
            definition: Arc::new(
                ComponentDef::map(|n: i64| n)
                    .claim(&identity_contract())
                    .unwrap(),
            ),
        }])
    }

    fn registration(id: &str, priority: i64, patch: NetworkPatch) -> Registration {
        Registration::new(
            id,
            [],
            priority,
            GraphOrigin::default(),
            patch.footprints(),
            patch,
        )
    }

    fn bound(composer: &Composer) -> crate::network::BoundOutput<i64> {
        composer
            .current()
            .compiled()
            .bind("worker", &identity_contract())
            .unwrap()
            .output::<i64>("output")
            .unwrap()
    }

    #[test]
    fn shadowed_admission_and_revocation_refresh_handle_identity() {
        let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
        composer
            .register([registration("winner", 10, add("worker"))])
            .unwrap();
        let previous = bound(&composer);

        composer
            .register([registration("loser", 0, add("worker"))])
            .unwrap();
        assert_eq!(
            previous.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().active(),
            &BTreeSet::from(["winner".into()])
        );
        assert_eq!(
            composer.current().footprints()["winner"],
            BTreeSet::from([Footprint::Component("worker".into())])
        );
        let previous = bound(&composer);
        assert_eq!(
            composer.revoke("loser").unwrap(),
            BTreeSet::from(["loser".into()])
        );
        assert_eq!(
            previous.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        let previous = bound(&composer);
        assert!(composer.revoke("missing").unwrap().is_empty());
        assert_eq!(
            previous.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        let previous = bound(&composer);
        composer.revoke("winner").unwrap();
        assert_eq!(
            previous.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert!(composer.current().active().is_empty());
    }

    #[test]
    fn unchanged_topology_does_not_bypass_authority_or_registry_rollback() {
        let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
        composer
            .register([registration("winner", 10, add("worker"))])
            .unwrap();
        let handle = composer
            .current()
            .compiled()
            .bind("worker", &identity_contract())
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        let unauthorized =
            Registration::new("bad", [], 0, GraphOrigin::default(), [], add("worker"));
        assert_eq!(
            composer.register([unauthorized]).unwrap_err(),
            ComposeError::Unauthorized {
                registration: "bad".into(),
                footprint: Footprint::Component("worker".into()),
            }
        );
        assert_eq!(
            handle.resolve(composer.current().compiled()),
            Ok((0, "output"))
        );
        assert!(composer.revoke("bad").unwrap().is_empty());
        assert_eq!(
            handle.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        // Duplicate registrations must still be rejected before planning.
        assert_eq!(
            composer
                .register([registration("winner", 10, add("worker"))])
                .unwrap_err(),
            ComposeError::DuplicateRegistration("winner".into())
        );
    }

    #[test]
    fn selected_empty_patch_updates_manifest_without_recompiling() {
        let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
        composer
            .register([registration("winner", 10, add("worker"))])
            .unwrap();
        let handle = composer
            .current()
            .compiled()
            .bind("worker", &identity_contract())
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer
            .register([registration("selected_no_op", 0, NetworkPatch::new([]))])
            .unwrap();
        assert_eq!(
            handle.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().active(),
            &BTreeSet::from(["winner".into(), "selected_no_op".into()])
        );
        assert!(composer.current().footprints()["selected_no_op"].is_empty());
        assert_eq!(
            composer.revoke("selected_no_op").unwrap(),
            BTreeSet::from(["selected_no_op".into()])
        );
        assert_eq!(
            handle.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().active(),
            &BTreeSet::from(["winner".into()])
        );
    }

    #[test]
    fn zero_edit_fast_path_still_obeys_selection_budget_and_atomicity() {
        let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
        composer
            .register([registration("writer", 0, add("worker"))])
            .unwrap();
        let old = bound(&composer);
        composer.set_selection_limits(SelectionLimits { max_work: 0 });
        assert_eq!(
            composer
                .register([registration("empty", 0, NetworkPatch::new([]))])
                .unwrap_err(),
            ComposeError::SelectionBudgetExceeded { limit: 0 }
        );
        assert_eq!(
            old.resolve(composer.current().compiled()),
            Ok((0, "output"))
        );
        assert_eq!(
            composer.current().active(),
            &BTreeSet::from(["writer".into()])
        );
        assert!(composer.revoke("empty").is_err());

        composer.set_selection_limits(SelectionLimits::default());
        composer
            .register([registration("empty", 0, NetworkPatch::new([]))])
            .unwrap();
        assert_eq!(
            old.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(
            composer.current().active(),
            &BTreeSet::from(["writer".into(), "empty".into()])
        );
    }

    #[test]
    fn changed_selected_write_set_creates_fresh_generation() {
        let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
        composer
            .register([registration("winner", 10, add("worker"))])
            .unwrap();
        let handle = composer
            .current()
            .compiled()
            .bind("worker", &identity_contract())
            .unwrap()
            .output::<i64>("output")
            .unwrap();
        composer
            .register([registration("new_writer", 0, add("second"))])
            .unwrap();
        assert_eq!(
            handle.resolve(composer.current().compiled()),
            Err(NetworkError::ForeignScope)
        );
        assert_eq!(composer.current().compiled().processes().len(), 2);
        assert_eq!(
            composer.revoke("new_writer").unwrap(),
            BTreeSet::from(["new_writer".into()])
        );
        assert_eq!(composer.current().compiled().processes().len(), 1);
    }
}
