//! Generic, provider-neutral workflow control flow.
//!
//! Component import/export resolution remains the kernel's single authority for
//! selecting a provider. A workflow binds its service nodes *after* those
//! imports have been validated. It does not implement its own provider solver.
//!
//! The executor only advances named nodes. Callers own state and dispatch each
//! service through the existing generation-pinned kernel invocation boundary.

use crate::{
    ComponentGraphError, ComponentId, InterfaceId, ResolvedComponentGraph, ResolvedImportHandle,
};
use serde::{Deserialize, Serialize, de::Error as _};
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::num::NonZeroU64;

/// Reject duplicate author-owned identities before canonicalizing portable
/// workflow JSON. Deserializing directly into BTreeMap would silently keep
/// the last node or normal-result edge, changing the authored control flow.
pub(crate) fn deserialize_unique_workflow_map<'de, D, V>(
    deserializer: D,
) -> Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct UniqueEntries<V>(PhantomData<V>);

    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for UniqueEntries<V> {
        type Value = BTreeMap<String, V>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a workflow map without duplicate identities")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut entries: M,
        ) -> Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((name, value)) = entries.next_entry::<String, V>()? {
                if result.insert(name.clone(), value).is_some() {
                    return Err(M::Error::custom(format!(
                        "duplicate workflow identity {name:?}"
                    )));
                }
            }
            Ok(result)
        }
    }

    deserializer.deserialize_map(UniqueEntries(PhantomData))
}

/// A typed handoff must also reject repeated slot keys, including identical
/// duplicate mappings. BTreeMap's default JSON decoding loses this evidence.
fn deserialize_unique_transfer_slots<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<crate::Key, crate::Key>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct UniqueSlots;
    impl<'de> serde::de::Visitor<'de> for UniqueSlots {
        type Value = BTreeMap<crate::Key, crate::Key>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a typed frame transfer without duplicate source fields")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut entries: M,
        ) -> Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((source, target)) = entries.next_entry::<crate::Key, crate::Key>()? {
                if result.insert(source.clone(), target).is_some() {
                    return Err(M::Error::custom(format!(
                        "duplicate transfer source {source}"
                    )));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(UniqueSlots)
}

/// A workflow belongs to the component that imports its node services.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDeclaration {
    pub owner: ComponentId,
    pub name: String,
    pub topology: WorkflowTopology,
}

/// An execution topology. The map key names a node within this workflow;
/// service identity remains independent of node identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTopology {
    pub entry: String,
    #[serde(deserialize_with = "deserialize_unique_workflow_map")]
    pub nodes: BTreeMap<String, WorkflowNode>,
}

/// A service node and the outcomes its caller is allowed to report.
///
/// Branch names belong to the contract adapter, not to the kernel. An adapter
/// projects a typed provider response into one of these declared outcomes.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    pub import: InterfaceId,
    #[serde(deserialize_with = "deserialize_unique_workflow_map")]
    pub branches: BTreeMap<String, WorkflowEdge>,
}

/// Branches are explicit. The kernel never guesses a default transition.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowEdge {
    Next {
        node: String,
    },
    /// Copy typed frame fields on an ordinary control-flow transition.
    /// Compilation retains this as data on an Invoke edge, not a fifth step.
    Transfer {
        node: String,
        #[serde(deserialize_with = "deserialize_unique_transfer_slots")]
        slots: BTreeMap<crate::Key, crate::Key>,
    },
    Finish,
    /// Copy typed output fields on an immediate terminal edge. This is
    /// data on an Invoke-to-Exit transition, not a new execution step.
    FinishTransfer {
        #[serde(deserialize_with = "deserialize_unique_transfer_slots")]
        slots: BTreeMap<crate::Key, crate::Key>,
    },
    /// Explicit normal failure outcome from an invoked child. Provider,
    /// transport, preparation and projection errors are never synthesized
    /// into this branch outcome.
    Fail,
    /// Admit named child scopes with independent frame snapshots. A generated
    /// Join step resolves the closed policy and explicitly selected outputs.
    Fork {
        #[serde(deserialize_with = "deserialize_unique_workflow_map")]
        branches: BTreeMap<String, String>,
        policy: crate::WorkflowJoinPolicy,
        #[serde(deserialize_with = "deserialize_unique_workflow_map")]
        outputs: BTreeMap<String, Vec<crate::Key>>,
        on_success: Box<WorkflowEdge>,
        on_failure: Box<WorkflowEdge>,
    },
    /// Map a bounded finite list into independent child frame snapshots.
    MapFork {
        collection: crate::Key,
        item_slot: crate::Key,
        child_output_slot: crate::Key,
        output_slot: crate::Key,
        max_children: usize,
        branch_entry: String,
        policy: crate::WorkflowJoinPolicy,
        on_success: Box<WorkflowEdge>,
        on_failure: Box<WorkflowEdge>,
    },
    /// Compile-time inclusion of a workflow authored by the same component.
    /// Each child finish outcome must map to one declared continuation.
    Include {
        workflow: String,
        site: String,
        #[serde(deserialize_with = "deserialize_unique_workflow_map")]
        on_exit: BTreeMap<String, WorkflowEdge>,
    },
    /// The same compile-time inclusion, with explicit data-only input/output
    /// aliasing. The selected parent frame must declare every field and type.
    IncludeMapped {
        workflow: String,
        site: String,
        #[serde(deserialize_with = "deserialize_unique_workflow_map")]
        on_exit: BTreeMap<String, WorkflowEdge>,
        #[serde(deserialize_with = "deserialize_unique_transfer_slots")]
        inputs: BTreeMap<crate::Key, crate::Key>,
        #[serde(deserialize_with = "deserialize_unique_transfer_slots")]
        outputs: BTreeMap<crate::Key, crate::Key>,
    },
}

// Internal lowering of the legacy service topology into execution steps.
// An Exit key is structurally disjoint from author-supplied Invoke names,
// preserving terminal outcomes without inventing or colliding string IDs.
// Fork and Join share the same pinned service-invocation dispatcher.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PlanStepId {
    Invoke(String),
    Fork { node: String, outcome: String },
    Join { node: String, outcome: String },
    Exit { node: String, outcome: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkflowMapFork {
    collection: crate::Key,
    item_slot: crate::Key,
    child_output_slot: crate::Key,
    output_slot: crate::Key,
    max_children: usize,
    branch_entry: PlanStepId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlanStep {
    Invoke {
        import: InterfaceId,
        on_result: BTreeMap<String, PlanStepId>,
        transfers: BTreeMap<String, BTreeMap<crate::Key, crate::Key>>,
    },
    Fork {
        branches: BTreeMap<String, PlanStepId>,
        map: Option<WorkflowMapFork>,
        join: PlanStepId,
    },
    Join {
        policy: crate::WorkflowJoinPolicy,
        outputs: BTreeMap<String, Vec<crate::Key>>,
        map_output: Option<(crate::Key, crate::Key)>,
        on_success: PlanStepId,
        on_failure: PlanStepId,
        on_success_transfer: Option<BTreeMap<crate::Key, crate::Key>>,
        on_failure_transfer: Option<BTreeMap<crate::Key, crate::Key>>,
    },
    Exit {
        failed: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoweredPlan {
    entry: PlanStepId,
    steps: BTreeMap<PlanStepId, PlanStep>,
}

impl LoweredPlan {
    /// A Join's return handoff remains data on its Join step. It reads the
    /// parent frame after explicitly selected child outputs have been merged.
    fn lower_join_return(
        edge: &WorkflowEdge,
        node: &str,
        outcome: &str,
        suffix: &str,
        steps: &mut BTreeMap<PlanStepId, PlanStep>,
    ) -> (PlanStepId, Option<BTreeMap<crate::Key, crate::Key>>) {
        match edge {
            WorkflowEdge::Next { node } => (PlanStepId::Invoke(node.clone()), None),
            WorkflowEdge::Transfer { node, slots } => {
                (PlanStepId::Invoke(node.clone()), Some(slots.clone()))
            }
            WorkflowEdge::Finish | WorkflowEdge::FinishTransfer { .. } => {
                let exit = PlanStepId::Exit {
                    node: node.to_owned(),
                    outcome: format!("{outcome}/{suffix}"),
                };
                steps.insert(exit.clone(), PlanStep::Exit { failed: false });
                let slots = match edge {
                    WorkflowEdge::FinishTransfer { slots } => Some(slots.clone()),
                    _ => None,
                };
                (exit, slots)
            }
            _ => unreachable!("Join continuation was validated before lowering"),
        }
    }

    fn from_topology(topology: &WorkflowTopology) -> Self {
        let mut steps = BTreeMap::new();
        for (name, node) in &topology.nodes {
            let mut on_result = BTreeMap::new();
            let mut transfers = BTreeMap::new();
            for (outcome, edge) in &node.branches {
                let target = match edge {
                    WorkflowEdge::Next { node } => PlanStepId::Invoke(node.clone()),
                    WorkflowEdge::Transfer { node, slots } => {
                        transfers.insert(outcome.clone(), slots.clone());
                        PlanStepId::Invoke(node.clone())
                    }
                    WorkflowEdge::Include { .. } | WorkflowEdge::IncludeMapped { .. } => {
                        unreachable!("selected subplans were inlined before lowering")
                    }
                    WorkflowEdge::Finish
                    | WorkflowEdge::FinishTransfer { .. }
                    | WorkflowEdge::Fail => {
                        if let WorkflowEdge::FinishTransfer { slots } = edge {
                            transfers.insert(outcome.clone(), slots.clone());
                        }
                        let exit = PlanStepId::Exit {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        };
                        steps.insert(
                            exit.clone(),
                            PlanStep::Exit {
                                failed: matches!(edge, WorkflowEdge::Fail),
                            },
                        );
                        exit
                    }
                    WorkflowEdge::Fork {
                        branches,
                        policy,
                        outputs,
                        on_success,
                        on_failure,
                    } => {
                        let fork = PlanStepId::Fork {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        };
                        let join = PlanStepId::Join {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        };
                        let (success, success_transfer) = Self::lower_join_return(
                            on_success, name, outcome, "success", &mut steps
                        );
                        let (failure, failure_transfer) = Self::lower_join_return(
                            on_failure, name, outcome, "failure", &mut steps
                        );
                        steps.insert(
                            fork.clone(),
                            PlanStep::Fork {
                                branches: branches
                                    .iter()
                                    .map(|(branch, entry)| {
                                        (branch.clone(), PlanStepId::Invoke(entry.clone()))
                                    })
                                    .collect(),
                                map: None,
                                join: join.clone(),
                            },
                        );
                        steps.insert(
                            join,
                            PlanStep::Join {
                                policy: *policy,
                                outputs: outputs.clone(),
                                map_output: None,
                                on_success: success,
                                on_failure: failure,
                                on_success_transfer: success_transfer,
                                on_failure_transfer: failure_transfer,
                            },
                        );
                        fork
                    }
                    WorkflowEdge::MapFork {
                        collection,
                        item_slot,
                        child_output_slot,
                        output_slot,
                        max_children,
                        branch_entry,
                        policy,
                        on_success,
                        on_failure,
                    } => {
                        let fork = PlanStepId::Fork {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        };
                        let join = PlanStepId::Join {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        };
                        let (success, success_transfer) = Self::lower_join_return(
                            on_success, name, outcome, "success", &mut steps
                        );
                        let (failure, failure_transfer) = Self::lower_join_return(
                            on_failure, name, outcome, "failure", &mut steps
                        );
                        steps.insert(
                            fork.clone(),
                            PlanStep::Fork {
                                branches: BTreeMap::new(),
                                map: Some(WorkflowMapFork {
                                    collection: collection.clone(),
                                    item_slot: item_slot.clone(),
                                    child_output_slot: child_output_slot.clone(),
                                    output_slot: output_slot.clone(),
                                    max_children: *max_children,
                                    branch_entry: PlanStepId::Invoke(branch_entry.clone()),
                                }),
                                join: join.clone(),
                            },
                        );
                        steps.insert(
                            join,
                            PlanStep::Join {
                                policy: *policy,
                                outputs: BTreeMap::new(),
                                map_output: Some((child_output_slot.clone(), output_slot.clone())),
                                on_success: success,
                                on_failure: failure,
                                on_success_transfer: success_transfer,
                                on_failure_transfer: failure_transfer,
                            },
                        );
                        fork
                    }
                };
                on_result.insert(outcome.clone(), target);
            }
            steps.insert(
                PlanStepId::Invoke(name.clone()),
                PlanStep::Invoke {
                    import: node.import.clone(),
                    on_result,
                    transfers,
                },
            );
        }
        Self {
            entry: PlanStepId::Invoke(topology.entry.clone()),
            steps,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowCompileError {
    MissingEntry(String),
    EmptyNodeName,
    EmptyBranch {
        node: String,
    },
    NoBranches {
        node: String,
    },
    UnknownTarget {
        from: String,
        target: String,
    },
    UnresolvedImport {
        node: String,
        import: InterfaceId,
    },
    ImportLookup {
        node: String,
        error: Box<ComponentGraphError>,
    },
    UnreachableNodes(Vec<String>),
    MissingSubplan(String),
    RecursiveSubplan(Vec<String>),
    SubplanDepthExceeded,
    SubplanExpansionTooLarge,
    InvalidInclusionSite(String),
    DuplicateInclusionSite(String),
    InclusionIdentityConflict(String),
    MissingSubplanExit {
        workflow: String,
        outcome: String,
    },
    UnknownSubplanExit {
        workflow: String,
        outcome: String,
    },
    UnsupportedReturnInclude,
    UnexpandedInclude {
        node: String,
        outcome: String,
    },
    InvalidFork {
        node: String,
        outcome: String,
        reason: String,
    },
    InvalidJoinContinuation {
        node: String,
        outcome: String,
    },
    InvalidFrameTransfer {
        node: String,
        outcome: String,
        reason: String,
    },
    NestedForkNotSupported {
        node: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledWorkflow {
    topology: WorkflowTopology,
    plan: LoweredPlan,
    bindings: BTreeMap<InterfaceId, ResolvedImportHandle>,
    frame_schema: Option<crate::WorkflowFrameSchema>,
    outcome_projections: BTreeMap<String, crate::WorkflowOutcomeProjection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowRunReport {
    pub last_node: String,
    /// The exact declared terminal edge reported by the last provider.
    /// Distinct finish outcomes from one node must remain distinguishable.
    pub final_outcome: String,
    pub executed_nodes: u64,
}

/// Error from executing against the providers selected at graph activation.
/// A request or response adapter failure, or a kernel dispatch error.
#[derive(Debug)]
pub enum WorkflowNodeDispatchError<E> {
    Prepare(E),
    Invoke(crate::KernelError),
    Project(E),
    Projection(crate::WorkflowProjectionError),
    ProjectionMismatch { selected: String, reported: String },
}

#[derive(Debug)]
pub enum WorkflowBoundCallError<E> {
    UnboundImport(InterfaceId),
    Invocation(E),
}

/// A node's request preparation can observe cancellation before dispatch.
/// Keep cancellation separate from provider or projection failures.
pub(crate) enum WorkflowInvocationError<E> {
    Cancelled,
    Failed(E),
}

#[derive(Debug)]
pub enum WorkflowRunError<E> {
    MissingWorkflow {
        owner: ComponentId,
        name: String,
    },
    MissingFrameSchema {
        owner: ComponentId,
        name: String,
    },
    FrameSchemaMismatch {
        owner: ComponentId,
        name: String,
    },
    Cancelled {
        next_node: String,
        executed_nodes: u64,
    },
    StepLimitReached {
        next_node: String,
        executed_nodes: u64,
    },
    NodeFailed {
        node: String,
        error: E,
    },
    UndeclaredOutcome {
        node: String,
        outcome: String,
    },
    StepCounterOverflow,
    StructuredFrameRequired {
        node: String,
    },
    InvalidJoin {
        node: String,
        error: crate::WorkflowJoinError,
    },
    InvalidJoinFrame {
        node: String,
        error: crate::WorkflowFrameError,
    },
    InvalidMapInput {
        node: String,
        reason: String,
    },
    InvalidTransitionFrame {
        node: String,
        error: crate::WorkflowFrameError,
    },
    ExplicitFailure {
        node: String,
        outcome: String,
        executed_nodes: u64,
    },
    ScopeDepthExceeded {
        node: String,
        maximum: usize,
    },
}

impl WorkflowTopology {
    /// Expand same-owner subplans before binding imports or compiling execution steps.
    ///
    /// The site-qualified IDs live only in the immutable compiled plan. No
    /// runtime CallPlan, provider lookup, or extra root is introduced. All
    /// returns must be mapped explicitly to a parent edge. Recursive references
    /// and oversized expansions fail before generation activation.
    pub(crate) fn inline_selected(
        owner: &ComponentId,
        name: &str,
        selected: &BTreeMap<(ComponentId, String), WorkflowTopology>,
    ) -> Result<Self, WorkflowCompileError> {
        fn expand(
            owner: &ComponentId,
            name: &str,
            selected: &BTreeMap<(ComponentId, String), WorkflowTopology>,
            stack: &mut Vec<String>,
            cache: &mut BTreeMap<String, WorkflowTopology>,
        ) -> Result<WorkflowTopology, WorkflowCompileError> {
            const MAX_INCLUSION_DEPTH: usize = 64;
            const MAX_EXPANDED_NODES: usize = 65_536;
            if let Some(position) = stack.iter().position(|item| item == name) {
                let mut cycle = stack[position..].to_vec();
                cycle.push(name.to_owned());
                return Err(WorkflowCompileError::RecursiveSubplan(cycle));
            }
            if stack.len() >= MAX_INCLUSION_DEPTH {
                return Err(WorkflowCompileError::SubplanDepthExceeded);
            }
            if let Some(cached) = cache.get(name) {
                return Ok(cached.clone());
            }
            let source = selected
                .get(&(owner.clone(), name.to_owned()))
                .ok_or_else(|| WorkflowCompileError::MissingSubplan(name.to_owned()))?;
            stack.push(name.to_owned());
            if source.nodes.len() > MAX_EXPANDED_NODES {
                return Err(WorkflowCompileError::SubplanExpansionTooLarge);
            }
            let mut expanded = source.clone();
            let mut sites = BTreeSet::new();
            for (parent_name, parent_node) in &source.nodes {
                for (outcome, edge) in &parent_node.branches {
                    let (workflow, site, on_exit, inputs, outputs) = match edge {
                        WorkflowEdge::Include {
                            workflow,
                            site,
                            on_exit,
                        } => (workflow, site, on_exit, None, None),
                        WorkflowEdge::IncludeMapped {
                            workflow,
                            site,
                            on_exit,
                            inputs,
                            outputs,
                        } => (workflow, site, on_exit, Some(inputs), Some(outputs)),
                        _ => continue,
                    };
                    if site.trim().is_empty() || site.trim() != site {
                        return Err(WorkflowCompileError::InvalidInclusionSite(site.clone()));
                    }
                    if !sites.insert(site.clone()) {
                        return Err(WorkflowCompileError::DuplicateInclusionSite(site.clone()));
                    }
                    let child = expand(owner, workflow, selected, stack, cache)?;
                    // A branch Exit settles its owning Fork, not the
                    // subplan itself. Only root-scope exits are rewritten
                    // as the included plan's declared return handoff.
                    let mut scoped = BTreeSet::new();
                    let mut pending_children = Vec::new();
                    for source_node in child.nodes.values() {
                        for source_edge in source_node.branches.values() {
                            match source_edge {
                                WorkflowEdge::Fork { branches, .. } => {
                                    pending_children.extend(branches.values().cloned());
                                }
                                WorkflowEdge::MapFork { branch_entry, .. } => {
                                    pending_children.push(branch_entry.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    while let Some(current) = pending_children.pop() {
                        if !scoped.insert(current.clone()) {
                            continue;
                        }
                        let source_node = child.nodes.get(&current).ok_or_else(|| {
                            WorkflowCompileError::UnknownTarget {
                                from: workflow.clone(),
                                target: current.clone(),
                            }
                        })?;
                        for edge in source_node.branches.values() {
                            match edge {
                                WorkflowEdge::Next { node }
                                | WorkflowEdge::Transfer { node, .. } => {
                                    pending_children.push(node.clone());
                                }
                                WorkflowEdge::Fork {
                                    on_success,
                                    on_failure,
                                    ..
                                }
                                | WorkflowEdge::MapFork {
                                    on_success,
                                    on_failure,
                                    ..
                                } => {
                                    for continuation in [on_success, on_failure] {
                                        if let WorkflowEdge::Next { node }
                                            | WorkflowEdge::Transfer { node, .. } =
                                                continuation.as_ref()
                                        {
                                            pending_children.push(node.clone());
                                        }
                                    }
                                }
                                WorkflowEdge::Finish
                                | WorkflowEdge::FinishTransfer { .. }
                                | WorkflowEdge::Fail => {}
                                WorkflowEdge::Include { .. }
                                | WorkflowEdge::IncludeMapped { .. } => {
                                    unreachable!("child workflow inclusions are already expanded");
                                }
                            }
                        }
                    }
                    let prefix = format!("__include__/{site}/");
                    let entry = format!("{prefix}{}", child.entry);
                    let mut declared_exits = BTreeSet::new();
                    // A root-scope Join can terminate the included workflow.
                    // Its public outcome is the same one used by normal
                    // lowering: <fork outcome>/<success|failure>. A nested
                    // child-scope Join instead settles its owning fork.
                    let qualify_join_return =
                        |edge: &WorkflowEdge,
                         child_name: &str,
                         child_outcome: &str,
                         suffix: &str,
                         exits: &mut BTreeSet<String>|
                         -> Result<WorkflowEdge, WorkflowCompileError> {
                            match edge {
                                WorkflowEdge::Next { node } => Ok(WorkflowEdge::Next {
                                    node: format!("{prefix}{node}"),
                                }),
                                WorkflowEdge::Finish if scoped.contains(child_name) => {
                                    Ok(WorkflowEdge::Finish)
                                }
                                WorkflowEdge::Finish => {
                                    let terminal = format!("{child_outcome}/{suffix}");
                                    exits.insert(terminal.clone());
                                    if outputs.is_some_and(|slots| !slots.is_empty()) {
                                        return Err(WorkflowCompileError::InvalidFork {
                                        node: child_name.to_owned(),
                                        outcome: terminal,
                                        reason: "mapped outputs from a terminal Join need an ordered Join transfer".into(),
                                    });
                                    }
                                    match on_exit.get(&terminal) {
                                        Some(WorkflowEdge::Next { node }) => {
                                            Ok(WorkflowEdge::Next { node: node.clone() })
                                        }
                                        Some(WorkflowEdge::Finish) => Ok(WorkflowEdge::Finish),
                                        Some(_) => {
                                            Err(WorkflowCompileError::InvalidJoinContinuation {
                                                node: child_name.to_owned(),
                                                outcome: terminal,
                                            })
                                        }
                                        None => Err(WorkflowCompileError::MissingSubplanExit {
                                            workflow: workflow.clone(),
                                            outcome: terminal,
                                        }),
                                    }
                                }
                                _ => Err(WorkflowCompileError::InvalidJoinContinuation {
                                    node: child_name.to_owned(),
                                    outcome: child_outcome.to_owned(),
                                }),
                            }
                        };
                    let mut inserted = Vec::with_capacity(child.nodes.len());
                    for (child_name, child_node) in &child.nodes {
                        let qualified = format!("{prefix}{child_name}");
                        let mut node = child_node.clone();
                        for (child_outcome, child_edge) in &mut node.branches {
                            *child_edge = match child_edge {
                                WorkflowEdge::Next { node } => WorkflowEdge::Next {
                                    node: format!("{prefix}{node}"),
                                },
                                WorkflowEdge::Transfer { node, slots } => WorkflowEdge::Transfer {
                                    node: format!("{prefix}{node}"),
                                    slots: slots.clone(),
                                },
                                WorkflowEdge::Finish
                                | WorkflowEdge::FinishTransfer { .. }
                                | WorkflowEdge::Fail
                                    if scoped.contains(child_name) =>
                                {
                                    child_edge.clone()
                                }
                                WorkflowEdge::Finish
                                | WorkflowEdge::FinishTransfer { .. }
                                | WorkflowEdge::Fail => {
                                    if matches!(child_edge, WorkflowEdge::FinishTransfer { .. }) {
                                        // Sequential handoffs cannot be represented as one
                                        // source-to-target map without losing aliasing.
                                        return Err(WorkflowCompileError::UnsupportedReturnInclude);
                                    }
                                    if matches!(child_edge, WorkflowEdge::Fail) {
                                        return Err(WorkflowCompileError::InvalidFork {
                                            node: child_name.clone(),
                                            outcome: child_outcome.clone(),
                                            reason: "failing child exits require a scoped fork, not inline subplan return".into(),
                                        });
                                    }
                                    declared_exits.insert(child_outcome.clone());
                                    match on_exit.get(child_outcome) {
                                        Some(WorkflowEdge::Next { node }) => match outputs {
                                            Some(slots) if !slots.is_empty() => {
                                                WorkflowEdge::Transfer {
                                                    node: node.clone(),
                                                    slots: (*slots).clone(),
                                                }
                                            }
                                            _ => WorkflowEdge::Next { node: node.clone() },
                                        },
                                        Some(WorkflowEdge::Transfer { node, slots }) => {
                                            // These transfers share one immutable source
                                            // snapshot. Combining them is only sound when
                                            // the parent transfer neither reads nor rewrites
                                            // an output written by the included return.
                                            let mut combined = outputs.cloned().unwrap_or_default();
                                            let child_targets: BTreeSet<_> =
                                                combined.values().cloned().collect();
                                            for (source, target) in slots {
                                                if child_targets.contains(source)
                                                    || child_targets.contains(target)
                                                    || combined.contains_key(source)
                                                {
                                                    return Err(
                                                        WorkflowCompileError::InvalidFrameTransfer {
                                                            node: child_name.clone(),
                                                            outcome: child_outcome.clone(),
                                                            reason: "ordered return transfers require distinct independent sources and destinations".into(),
                                                        },
                                                    );
                                                }
                                                combined.insert(source.clone(), target.clone());
                                            }
                                            WorkflowEdge::Transfer {
                                                node: node.clone(),
                                                slots: combined,
                                            }
                                        }
                                        Some(WorkflowEdge::Finish)
                                            if outputs.is_some_and(|slots| !slots.is_empty()) =>
                                        {
                                            WorkflowEdge::FinishTransfer {
                                                slots: (*outputs.expect("nonempty mapping"))
                                                    .clone(),
                                            }
                                        }
                                        Some(WorkflowEdge::Finish) => WorkflowEdge::Finish,
                                        Some(WorkflowEdge::FinishTransfer { .. }) => {
                                            return Err(
                                                WorkflowCompileError::UnsupportedReturnInclude,
                                            );
                                        }
                                        Some(WorkflowEdge::Fail) => {
                                            return Err(WorkflowCompileError::InvalidFork {
                                                node: child_name.clone(),
                                                outcome: child_outcome.clone(),
                                                reason: "subplan return cannot produce a scoped child failure".into(),
                                            });
                                        }
                                        Some(
                                            WorkflowEdge::Include { .. }
                                            | WorkflowEdge::IncludeMapped { .. },
                                        ) => {
                                            return Err(
                                                WorkflowCompileError::UnsupportedReturnInclude,
                                            );
                                        }
                                        Some(WorkflowEdge::MapFork { .. }) => {
                                            return Err(WorkflowCompileError::InvalidFork {
                                                node: child_name.clone(),
                                                outcome: child_outcome.clone(),
                                                reason: "subplan return cannot directly admit a map fork".into(),
                                            });
                                        }
                                        Some(WorkflowEdge::Fork { .. }) => {
                                            return Err(WorkflowCompileError::InvalidFork {
                                                node: child_name.clone(),
                                                outcome: child_outcome.clone(),
                                                reason:
                                                    "subplan return cannot directly admit a fork"
                                                        .into(),
                                            });
                                        }
                                        None => {
                                            return Err(WorkflowCompileError::MissingSubplanExit {
                                                workflow: workflow.clone(),
                                                outcome: child_outcome.clone(),
                                            });
                                        }
                                    }
                                }
                                WorkflowEdge::Include { .. }
                                | WorkflowEdge::IncludeMapped { .. } => {
                                    return Err(WorkflowCompileError::UnexpandedInclude {
                                        node: child_name.clone(),
                                        outcome: child_outcome.clone(),
                                    });
                                }
                                WorkflowEdge::MapFork {
                                    collection,
                                    item_slot,
                                    child_output_slot,
                                    output_slot,
                                    max_children,
                                    branch_entry,
                                    policy,
                                    on_success,
                                    on_failure,
                                } => {
                                    let mut qualify = |edge: &WorkflowEdge, suffix| {
                                        qualify_join_return(
                                            edge,
                                            child_name,
                                            child_outcome,
                                            suffix,
                                            &mut declared_exits,
                                        )
                                    };
                                    WorkflowEdge::MapFork {
                                        collection: collection.clone(),
                                        item_slot: item_slot.clone(),
                                        child_output_slot: child_output_slot.clone(),
                                        output_slot: output_slot.clone(),
                                        max_children: *max_children,
                                        branch_entry: format!("{prefix}{branch_entry}"),
                                        policy: *policy,
                                        on_success: Box::new(qualify(on_success, "success")?),
                                        on_failure: Box::new(qualify(on_failure, "failure")?),
                                    }
                                }
                                WorkflowEdge::Fork {
                                    branches,
                                    policy,
                                    outputs,
                                    on_success,
                                    on_failure,
                                } => {
                                    let mut qualify = |edge: &WorkflowEdge, suffix| {
                                        qualify_join_return(
                                            edge,
                                            child_name,
                                            child_outcome,
                                            suffix,
                                            &mut declared_exits,
                                        )
                                    };
                                    WorkflowEdge::Fork {
                                        branches: branches
                                            .iter()
                                            .map(|(branch, entry)| {
                                                (branch.clone(), format!("{prefix}{entry}"))
                                            })
                                            .collect(),
                                        policy: *policy,
                                        outputs: outputs.clone(),
                                        on_success: Box::new(qualify(on_success, "success")?),
                                        on_failure: Box::new(qualify(on_failure, "failure")?),
                                    }
                                }
                            };
                        }
                        inserted.push((qualified, node));
                    }
                    for exit in on_exit.keys() {
                        if !declared_exits.contains(exit) {
                            return Err(WorkflowCompileError::UnknownSubplanExit {
                                workflow: workflow.clone(),
                                outcome: exit.clone(),
                            });
                        }
                    }
                    if expanded.nodes.len().saturating_add(inserted.len()) > MAX_EXPANDED_NODES {
                        return Err(WorkflowCompileError::SubplanExpansionTooLarge);
                    }
                    // Detect collisions with authored nodes and earlier sites.
                    // All mutations are local to the candidate until validation completes.
                    for (qualified, node) in inserted {
                        if expanded.nodes.insert(qualified.clone(), node).is_some() {
                            return Err(WorkflowCompileError::InclusionIdentityConflict(qualified));
                        }
                    }
                    expanded
                        .nodes
                        .get_mut(parent_name)
                        .expect("source parent is present in its cloned topology")
                        .branches
                        .insert(
                            outcome.clone(),
                            match inputs {
                                Some(slots) if !slots.is_empty() => WorkflowEdge::Transfer {
                                    node: entry,
                                    slots: (*slots).clone(),
                                },
                                _ => WorkflowEdge::Next { node: entry },
                            },
                        );
                }
            }
            stack.pop();
            cache.insert(name.to_owned(), expanded.clone());
            Ok(expanded)
        }
        expand(owner, name, selected, &mut Vec::new(), &mut BTreeMap::new())
    }

    /// Compile against the caller's canonical resolved component imports.
    ///
    /// This rejects undeclared, disabled, incompatible or unauthorized node
    /// providers. The resulting execution must still use generation-pinned
    /// kernel dispatch; compilation grants no new authority.
    pub(crate) fn compile_for_component(
        self,
        graph: &ResolvedComponentGraph,
        component: &ComponentId,
    ) -> Result<CompiledWorkflow, WorkflowCompileError> {
        // Validate topology before reading the selected binding graph. An
        // invalid edge must not be disguised as a missing import.
        let mut compiled = self.compile(|_| true)?;
        let mut bindings = BTreeMap::new();
        for (name, node) in &compiled.topology.nodes {
            if bindings.contains_key(&node.import) {
                continue;
            }
            match graph.import_handle(component, &node.import) {
                Ok(Some(import)) => {
                    bindings.insert(node.import.clone(), import.clone());
                }
                Ok(None) => {
                    return Err(WorkflowCompileError::UnresolvedImport {
                        node: name.clone(),
                        import: node.import.clone(),
                    });
                }
                Err(error) => {
                    return Err(WorkflowCompileError::ImportLookup {
                        node: name.clone(),
                        error: Box::new(error),
                    });
                }
            }
        }
        compiled.bindings = bindings;
        Ok(compiled)
    }

    /// Validate the complete topology and verify that every referenced service
    /// has a selected, compatible and authorized binding in the resolved graph.
    ///
    /// Internal validation helper; only `compile_for_component` supplies a
    /// production binding check against the canonical resolved graph.
    fn compile(
        self,
        mut bound_import: impl FnMut(&InterfaceId) -> bool,
    ) -> Result<CompiledWorkflow, WorkflowCompileError> {
        if !self.nodes.contains_key(&self.entry) {
            return Err(WorkflowCompileError::MissingEntry(self.entry));
        }
        if self.nodes.keys().any(|name| name.trim().is_empty()) {
            return Err(WorkflowCompileError::EmptyNodeName);
        }

        for (name, node) in &self.nodes {
            if node.branches.is_empty() {
                return Err(WorkflowCompileError::NoBranches { node: name.clone() });
            }
            if !bound_import(&node.import) {
                return Err(WorkflowCompileError::UnresolvedImport {
                    node: name.clone(),
                    import: node.import.clone(),
                });
            }
            for (outcome, edge) in &node.branches {
                if outcome.trim().is_empty() {
                    return Err(WorkflowCompileError::EmptyBranch { node: name.clone() });
                }
                match edge {
                    WorkflowEdge::Next { node: target }
                    | WorkflowEdge::Transfer { node: target, .. }
                        if !self.nodes.contains_key(target) =>
                    {
                        return Err(WorkflowCompileError::UnknownTarget {
                            from: name.clone(),
                            target: target.clone(),
                        });
                    }
                    WorkflowEdge::Include { .. } | WorkflowEdge::IncludeMapped { .. } => {
                        return Err(WorkflowCompileError::UnexpandedInclude {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        });
                    }
                    WorkflowEdge::MapFork {
                        collection: _,
                        item_slot: _,
                        child_output_slot: _,
                        output_slot: _,
                        max_children,
                        branch_entry,
                        policy,
                        on_success,
                        on_failure,
                    } => {
                        if *max_children == 0 {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: "map fan-out must declare a positive bound".into(),
                            });
                        }
                        if !self.nodes.contains_key(branch_entry) {
                            return Err(WorkflowCompileError::UnknownTarget {
                                from: name.clone(),
                                target: branch_entry.clone(),
                            });
                        }
                        if let crate::WorkflowJoinPolicy::Quorum(k) = policy
                            && k.get() > *max_children
                        {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: "quorum exceeds maximum admitted map children".into(),
                            });
                        }
                        for continuation in [on_success, on_failure] {
                            match continuation.as_ref() {
                                WorkflowEdge::Next { node: target }
                                | WorkflowEdge::Transfer { node: target, .. }
                                    if !self.nodes.contains_key(target) =>
                                {
                                    return Err(WorkflowCompileError::UnknownTarget {
                                        from: name.clone(),
                                        target: target.clone(),
                                    });
                                }
                                WorkflowEdge::Next { .. }
                                | WorkflowEdge::Transfer { .. }
                                | WorkflowEdge::Finish
                                | WorkflowEdge::FinishTransfer { .. } => {}
                                _ => {
                                    return Err(WorkflowCompileError::InvalidJoinContinuation {
                                        node: name.clone(),
                                        outcome: outcome.clone(),
                                    });
                                }
                            }
                        }
                    }
                    WorkflowEdge::Fork {
                        branches,
                        outputs,
                        on_success,
                        on_failure,
                        policy,
                    } => {
                        if branches.is_empty() || branches.keys().any(|key| key.trim().is_empty()) {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: "fork needs at least one nonempty named child".into(),
                            });
                        }
                        if let crate::WorkflowJoinPolicy::Quorum(k) = policy
                            && k.get() > branches.len()
                        {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: "quorum exceeds admitted children".into(),
                            });
                        }
                        let mut emitted = BTreeSet::new();
                        for (branch, slots) in outputs {
                            if !branches.contains_key(branch) {
                                return Err(WorkflowCompileError::InvalidFork {
                                    node: name.clone(),
                                    outcome: outcome.clone(),
                                    reason: format!("outputs refer to unknown branch {branch}"),
                                });
                            }
                            for slot in slots {
                                if !emitted.insert(slot) {
                                    return Err(WorkflowCompileError::InvalidFork {
                                        node: name.clone(),
                                        outcome: outcome.clone(),
                                        reason: format!(
                                            "slot {slot} is produced by multiple branches"
                                        ),
                                    });
                                }
                            }
                        }
                        for continuation in [on_success, on_failure] {
                            match continuation.as_ref() {
                                WorkflowEdge::Next { node: target }
                                | WorkflowEdge::Transfer { node: target, .. }
                                    if !self.nodes.contains_key(target) =>
                                {
                                    return Err(WorkflowCompileError::UnknownTarget {
                                        from: name.clone(),
                                        target: target.clone(),
                                    });
                                }
                                WorkflowEdge::Next { .. }
                                | WorkflowEdge::Transfer { .. }
                                | WorkflowEdge::Finish
                                | WorkflowEdge::FinishTransfer { .. } => {}
                                _ => {
                                    return Err(WorkflowCompileError::InvalidJoinContinuation {
                                        node: name.clone(),
                                        outcome: outcome.clone(),
                                    });
                                }
                            }
                        }
                        for entry in branches.values() {
                            if !self.nodes.contains_key(entry) {
                                return Err(WorkflowCompileError::UnknownTarget {
                                    from: name.clone(),
                                    target: entry.clone(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Validate reachability without rejecting legitimate execution cycles.
        let mut seen = BTreeSet::new();
        let mut pending = vec![self.entry.clone()];
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            for edge in self.nodes[&name].branches.values() {
                match edge {
                    WorkflowEdge::Next { node } | WorkflowEdge::Transfer { node, .. } => {
                        pending.push(node.clone());
                    }
                    WorkflowEdge::Fork {
                        branches,
                        on_success,
                        on_failure,
                        ..
                    } => {
                        pending.extend(branches.values().cloned());
                        for continuation in [on_success, on_failure] {
                            if let WorkflowEdge::Next { node }
                                            | WorkflowEdge::Transfer { node, .. } =
                                                continuation.as_ref()
                                        {
                                pending.push(node.clone());
                            }
                        }
                    }
                    WorkflowEdge::MapFork {
                        branch_entry,
                        on_success,
                        on_failure,
                        ..
                    } => {
                        pending.push(branch_entry.clone());
                        for continuation in [on_success, on_failure] {
                            if let WorkflowEdge::Next { node }
                                            | WorkflowEdge::Transfer { node, .. } =
                                                continuation.as_ref()
                                        {
                                pending.push(node.clone());
                            }
                        }
                    }
                    WorkflowEdge::Finish
                    | WorkflowEdge::FinishTransfer { .. }
                    | WorkflowEdge::Fail
                    | WorkflowEdge::Include { .. }
                    | WorkflowEdge::IncludeMapped { .. } => {}
                }
            }
        }
        if seen.len() != self.nodes.len() {
            let unreachable = self
                .nodes
                .keys()
                .filter(|name| !seen.contains(*name))
                .cloned()
                .collect();
            return Err(WorkflowCompileError::UnreachableNodes(unreachable));
        }
        // A fork owns disjoint child regions. Child scopes never enter their
        // parent/sibling via an ordinary edge. Nested scopes have a separate
        // owner; their declared Join continuations remain parent-owned.
        let mut scope_members = BTreeMap::<(String, String), BTreeSet<String>>::new();
        for (fork_owner, node) in &self.nodes {
            for (fork_outcome, edge) in &node.branches {
                let branch_entries = match edge {
                    WorkflowEdge::Fork { branches, .. } => branches.clone(),
                    WorkflowEdge::MapFork { branch_entry, .. } => {
                        BTreeMap::from([("map".to_owned(), branch_entry.clone())])
                    }
                    _ => continue,
                };
                let mut owner_of = BTreeMap::<String, String>::new();
                for (branch, entry) in &branch_entries {
                    let mut visit = vec![entry.clone()];
                    let mut visited = BTreeSet::new();
                    while let Some(current) = visit.pop() {
                        if !visited.insert(current.clone()) {
                            continue;
                        }
                        if current == *fork_owner || current == self.entry {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: fork_owner.clone(),
                                outcome: fork_outcome.clone(),
                                reason: "child escapes into its fork or the root entry".into(),
                            });
                        }
                        if let Some(previous) = owner_of.insert(current.clone(), branch.clone())
                            && previous != *branch
                        {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: fork_owner.clone(),
                                outcome: fork_outcome.clone(),
                                reason: format!(
                                    "child {branch} crosses into child {previous} at {current}"
                                ),
                            });
                        }
                        for next in self.nodes[&current].branches.values() {
                            match next {
                                WorkflowEdge::Next { node }
                                | WorkflowEdge::Transfer { node, .. } => visit.push(node.clone()),
                                WorkflowEdge::Finish
                                | WorkflowEdge::FinishTransfer { .. }
                                | WorkflowEdge::Fail => {}
                                WorkflowEdge::MapFork {
                                    on_success,
                                    on_failure,
                                    ..
                                }
                                | WorkflowEdge::Fork {
                                    on_success,
                                    on_failure,
                                    ..
                                } => {
                                    // Nested children are visited in their own
                                    // declared scope. Their Join continuations
                                    // remain in this outer child scope.
                                    for continuation in [on_success, on_failure] {
                                        if let WorkflowEdge::Next { node }
                                            | WorkflowEdge::Transfer { node, .. } =
                                                continuation.as_ref()
                                        {
                                            visit.push(node.clone());
                                        }
                                    }
                                }
                                WorkflowEdge::Include { .. }
                                | WorkflowEdge::IncludeMapped { .. } => {
                                    unreachable!("includes are rejected before reachability")
                                }
                            }
                        }
                    }
                }
                // Join continuations are parent-owned. Letting one point
                // at a child would restart it outside the child scope with
                // the parent's frame and bypass structured settlement.
                let (on_success, on_failure) = match edge {
                    WorkflowEdge::Fork {
                        on_success,
                        on_failure,
                        ..
                    }
                    | WorkflowEdge::MapFork {
                        on_success,
                        on_failure,
                        ..
                    } => (on_success, on_failure),
                    _ => unreachable!("only forks have admitted child scopes"),
                };
                for continuation in [on_success, on_failure] {
                    if let WorkflowEdge::Next { node: target }
                    | WorkflowEdge::Transfer { node: target, .. } = continuation.as_ref()
                        && owner_of.contains_key(target)
                    {
                        return Err(WorkflowCompileError::InvalidFork {
                            node: fork_owner.clone(),
                            outcome: fork_outcome.clone(),
                            reason: format!(
                                "join continuation re-enters child {target} without a new fork"
                            ),
                        });
                    }
                }
                // The parent cannot jump into children except through its fork.
                for (other_name, other_node) in &self.nodes {
                    if owner_of.contains_key(other_name) {
                        continue;
                    }
                    for other_edge in other_node.branches.values() {
                        if let WorkflowEdge::Next { node: target }
                        | WorkflowEdge::Transfer { node: target, .. } = other_edge
                            && owner_of.contains_key(target)
                        {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: fork_owner.clone(),
                                outcome: fork_outcome.clone(),
                                reason: format!(
                                    "parent node {other_name} enters child {target} outside its fork"
                                ),
                            });
                        }
                    }
                }
                scope_members.insert(
                    (fork_owner.clone(), fork_outcome.clone()),
                    owner_of.into_keys().collect(),
                );
            }
        }
        // Active nesting forms a finite DAG. A nested fork may be revisited
        // after its Join in an ordinary service-bound loop, but a child
        // cannot recursively admit an active ancestor scope. Check before
        // activation so a recursive declaration never invokes providers.
        fn verify_nesting(
            id: &(String, String),
            nested: &BTreeMap<(String, String), BTreeSet<(String, String)>>,
            visiting: &mut BTreeSet<(String, String)>,
            resolved: &mut BTreeMap<(String, String), usize>,
            traversal_depth: usize,
        ) -> Result<usize, WorkflowCompileError> {
            const MAX_SCOPE_DEPTH: usize = 64;
            if traversal_depth > MAX_SCOPE_DEPTH {
                return Err(WorkflowCompileError::InvalidFork {
                    node: id.0.clone(),
                    outcome: id.1.clone(),
                    reason: "nested scope depth exceeds 64".into(),
                });
            }
            if let Some(depth) = resolved.get(id) {
                return Ok(*depth);
            }
            if !visiting.insert(id.clone()) {
                return Err(WorkflowCompileError::InvalidFork {
                    node: id.0.clone(),
                    outcome: id.1.clone(),
                    reason: "recursive structured scope admission".into(),
                });
            }
            let mut depth = 1usize;
            if let Some(children) = nested.get(id) {
                for child in children {
                    depth = depth.max(
                        1 + verify_nesting(child, nested, visiting, resolved, traversal_depth + 1)?,
                    );
                    if depth > MAX_SCOPE_DEPTH {
                        return Err(WorkflowCompileError::InvalidFork {
                            node: id.0.clone(),
                            outcome: id.1.clone(),
                            reason: "nested scope depth exceeds 64".into(),
                        });
                    }
                }
            }
            visiting.remove(id);
            resolved.insert(id.clone(), depth);
            Ok(depth)
        }
        let mut nested = BTreeMap::new();
        for (id, members) in &scope_members {
            let mut subscopes = BTreeSet::new();
            for member in members {
                for (outcome, edge) in &self.nodes[member].branches {
                    if matches!(
                        edge,
                        WorkflowEdge::Fork { .. } | WorkflowEdge::MapFork { .. }
                    ) {
                        subscopes.insert((member.clone(), outcome.clone()));
                    }
                }
            }
            nested.insert(id.clone(), subscopes);
        }
        let mut resolved_depths = BTreeMap::new();
        for scope in scope_members.keys() {
            verify_nesting(
                scope,
                &nested,
                &mut BTreeSet::new(),
                &mut resolved_depths,
                1,
            )?;
        }

        // A workflow can have no Exit when it is a long-running service loop.
        // Every cycle in this interim IR invokes a service, so cancellation
        // and optional step limits are checked on every back edge.
        let plan = LoweredPlan::from_topology(&self);
        Ok(CompiledWorkflow {
            topology: self,
            plan,
            bindings: BTreeMap::new(),
            frame_schema: None,
            outcome_projections: BTreeMap::new(),
        })
    }
}

impl CompiledWorkflow {
    pub fn topology(&self) -> &WorkflowTopology {
        &self.topology
    }

    /// Selected typed frame contract. Missing means this legacy workflow has
    /// no frame-aware entry, not that an arbitrary schema can be supplied.
    pub fn frame_schema(&self) -> Option<&crate::WorkflowFrameSchema> {
        self.frame_schema.as_ref()
    }

    /// Read only the projection selected with this compiled generation.
    pub fn outcome_projection(&self, node: &str) -> Option<&crate::WorkflowOutcomeProjection> {
        self.outcome_projections.get(node)
    }

    pub(crate) fn bind_outcome_projection(
        &mut self,
        node: String,
        projection: crate::WorkflowOutcomeProjection,
    ) {
        self.outcome_projections.insert(node, projection);
    }

    pub(crate) fn validate_outcome_projection(
        &self,
        node: &str,
        projection: &crate::WorkflowOutcomeProjection,
    ) -> Result<(), crate::WorkflowProjectionError> {
        let node_def = self
            .topology
            .nodes
            .get(node)
            .ok_or(crate::WorkflowProjectionError::UnknownNode(node.to_owned()))?;
        let binding = self
            .bindings
            .get(&node_def.import)
            .ok_or(crate::WorkflowProjectionError::UnboundNode(node.to_owned()))?;
        projection.validate(
            binding.response_schema(),
            &node_def.branches.keys().cloned().collect(),
        )
    }

    pub(crate) fn bind_frame_schema(&mut self, schema: crate::WorkflowFrameSchema) {
        self.frame_schema = Some(schema);
    }

    pub(crate) fn requires_frame(&self) -> bool {
        self.plan.steps.values().any(|step| match step {
            PlanStep::Fork { .. } => true,
            PlanStep::Invoke { transfers, .. } => !transfers.is_empty(),
            _ => false,
        })
    }

    pub(crate) fn validate_frame_schema(
        &self,
        schema: &crate::WorkflowFrameSchema,
    ) -> Result<(), WorkflowCompileError> {
        for (name, node) in &self.topology.nodes {
            for (outcome, edge) in &node.branches {
                // Both an Invoke outcome and a Join continuation may
                // transfer data. Type-check every mapping before activation.
                let mut transitions = vec![edge];
                if let WorkflowEdge::Fork {
                    on_success,
                    on_failure,
                    ..
                }
                | WorkflowEdge::MapFork {
                    on_success,
                    on_failure,
                    ..
                } = edge
                {
                    transitions.extend([on_success.as_ref(), on_failure.as_ref()]);
                }
                for transition in transitions {
                if let WorkflowEdge::Transfer { slots, .. }
                | WorkflowEdge::FinishTransfer { slots } = transition
                {
                    let mut destinations = BTreeSet::new();
                    for (source, target) in slots {
                        if !destinations.insert(target) {
                            return Err(WorkflowCompileError::InvalidFrameTransfer {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: format!("target field {target} is written twice"),
                            });
                        }
                        let from = schema.slots.get(source).ok_or_else(|| {
                            WorkflowCompileError::InvalidFrameTransfer {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: format!("source field {source} is undeclared"),
                            }
                        })?;
                        let to = schema.slots.get(target).ok_or_else(|| {
                            WorkflowCompileError::InvalidFrameTransfer {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: format!("target field {target} is undeclared"),
                            }
                        })?;
                        if !matches!(
                            to.accepts(from),
                            crate::SchemaCompatibility::Exact
                                | crate::SchemaCompatibility::Compatible
                        ) {
                            return Err(WorkflowCompileError::InvalidFrameTransfer {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: format!("incompatible handoff {source} to {target}"),
                            });
                        }
                    }
                }
                }
                if let WorkflowEdge::MapFork {
                    collection,
                    item_slot,
                    child_output_slot,
                    output_slot,
                    ..
                } = edge
                {
                    let required = [collection, item_slot, child_output_slot, output_slot];
                    for slot in required {
                        if !schema.slots.contains_key(slot) {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: format!("map frame slot {slot} is absent"),
                            });
                        }
                    }
                    if !matches!(schema.slots.get(collection), Some(crate::Type::List(_)))
                        || !matches!(schema.slots.get(output_slot), Some(crate::Type::List(_)))
                    {
                        return Err(WorkflowCompileError::InvalidFork {
                            node: name.clone(),
                            outcome: outcome.clone(),
                            reason: "map collection and output must have list schemas".into(),
                        });
                    }
                    if let Some(crate::Type::List(item)) = schema.slots.get(collection) {
                        let slot_type = &schema.slots[item_slot];
                        if !matches!(
                            slot_type.accepts(item),
                            crate::SchemaCompatibility::Exact
                                | crate::SchemaCompatibility::Compatible
                        ) {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: "map item slot cannot accept collection element schema"
                                    .into(),
                            });
                        }
                    }
                    if let Some(crate::Type::List(item)) = schema.slots.get(output_slot) {
                        let produced = &schema.slots[child_output_slot];
                        if !matches!(
                            item.accepts(produced),
                            crate::SchemaCompatibility::Exact
                                | crate::SchemaCompatibility::Compatible
                        ) {
                            return Err(WorkflowCompileError::InvalidFork {
                                node: name.clone(),
                                outcome: outcome.clone(),
                                reason: "map output cannot accept child result slot".into(),
                            });
                        }
                    }
                }
                if let WorkflowEdge::Fork { outputs, .. } = edge {
                    for slots in outputs.values() {
                        for slot in slots {
                            if !schema.slots.contains_key(slot) {
                                return Err(WorkflowCompileError::InvalidFork {
                                    node: name.clone(),
                                    outcome: outcome.clone(),
                                    reason: format!(
                                        "join output slot {slot} is absent from its selected frame"
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Selected import handle, including provider, authority and generation
    /// resolution data. Available after compile_for_component.
    pub fn bound_import(&self, import: &InterfaceId) -> Option<&ResolvedImportHandle> {
        self.bindings.get(import)
    }

    /// Interpret declared control-flow edges with a test-only dispatcher.
    ///
    /// Production code enters through `RootExecutionHandle::execute_workflow`
    /// so the caller cannot substitute an unbound execution callback.
    ///
    /// Cancellation is checked before every service call. Step limits are
    /// optional. A failed call is never silently retried or replayed.
    #[cfg(test)]
    fn execute<State, Error>(
        &self,
        state: &mut State,
        mut invoke: impl FnMut(&InterfaceId, &mut State) -> Result<String, Error>,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        self.execute_nodes(
            state,
            None,
            |_, import, state, _, _| invoke(import, state).map_err(WorkflowInvocationError::Failed),
            cancelled,
            step_limit,
        )
    }

    /// Execute against provider imports selected at generation preparation.
    ///
    /// This is Core-private. Only the root execution host may supply the
    /// dispatcher, retaining pinned-generation authority and Service Layers.
    pub(crate) fn execute_bound<State, Error>(
        &self,
        state: &mut State,
        mut invoke: impl FnMut(
            &str,
            &InterfaceId,
            &ResolvedImportHandle,
            &mut State,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<WorkflowBoundCallError<Error>>> {
        self.execute_nodes(
            state,
            None,
            |name, import, state, _, cancelled| {
                let binding = self.bindings.get(import).ok_or_else(|| {
                    WorkflowInvocationError::Failed(WorkflowBoundCallError::UnboundImport(
                        import.clone(),
                    ))
                })?;
                invoke(name, import, binding, state, cancelled).map_err(|error| match error {
                    WorkflowInvocationError::Cancelled => WorkflowInvocationError::Cancelled,
                    WorkflowInvocationError::Failed(error) => {
                        WorkflowInvocationError::Failed(WorkflowBoundCallError::Invocation(error))
                    }
                })
            },
            cancelled,
            step_limit,
        )
    }

    // All entries share this dispatcher, even child scopes. No second provider
    // resolver or agent-specific engine is instantiated for a fork.
    fn invoke_step<State, Error>(
        &self,
        cursor: &PlanStepId,
        state: &mut State,
        mut data: Option<&mut crate::WorkflowFrame>,
        invoke: &mut impl FnMut(
            &str,
            &InterfaceId,
            &mut State,
            Option<&mut crate::WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        cancelled: &mut impl FnMut() -> bool,
        budget: (&mut u64, Option<NonZeroU64>),
    ) -> Result<PlanStepId, WorkflowRunError<Error>> {
        let (count, limit) = budget;
        let PlanStepId::Invoke(name) = cursor else {
            unreachable!("invoke dispatch requires an Invoke identity")
        };
        let PlanStep::Invoke {
            import,
            on_result,
            transfers,
        } = &self.plan.steps[cursor]
        else {
            unreachable!("compiled Invoke has an Invoke step")
        };
        if cancelled() {
            return Err(WorkflowRunError::Cancelled {
                next_node: name.clone(),
                executed_nodes: *count,
            });
        }
        if limit.is_some_and(|limit| *count >= limit.get()) {
            return Err(WorkflowRunError::StepLimitReached {
                next_node: name.clone(),
                executed_nodes: *count,
            });
        }
        let outcome = match invoke(name, import, state, data.as_deref_mut(), cancelled) {
            Ok(outcome) => outcome,
            Err(WorkflowInvocationError::Cancelled) => {
                return Err(WorkflowRunError::Cancelled {
                    next_node: name.clone(),
                    executed_nodes: *count,
                });
            }
            Err(WorkflowInvocationError::Failed(error)) => {
                return Err(WorkflowRunError::NodeFailed {
                    node: name.clone(),
                    error,
                });
            }
        };
        *count = count
            .checked_add(1)
            .ok_or(WorkflowRunError::StepCounterOverflow)?;
        let target = on_result.get(&outcome).cloned().ok_or_else(|| {
            WorkflowRunError::UndeclaredOutcome {
                node: name.clone(),
                outcome: outcome.clone(),
            }
        })?;
        if let Some(mappings) = transfers.get(&outcome) {
            let frame = data
                .ok_or_else(|| WorkflowRunError::StructuredFrameRequired { node: name.clone() })?;
            frame.transfer_slots(mappings).map_err(|error| {
                WorkflowRunError::InvalidTransitionFrame {
                    node: name.clone(),
                    error,
                }
            })?;
        }
        Ok(target)
    }

    pub(crate) fn execute_bound_framed<State, Error>(
        &self,
        state: &mut State,
        frame: &mut crate::WorkflowFrame,
        mut invoke: impl FnMut(
            &str,
            &InterfaceId,
            &ResolvedImportHandle,
            &mut State,
            &mut crate::WorkflowFrame,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<WorkflowBoundCallError<Error>>> {
        self.execute_nodes(
            state,
            Some(frame),
            |name, import, state, frame, cancelled| {
                let binding = self.bindings.get(import).ok_or_else(|| {
                    WorkflowInvocationError::Failed(WorkflowBoundCallError::UnboundImport(
                        import.clone(),
                    ))
                })?;
                let frame = frame.expect("framed plan always carries a frame");
                invoke(name, import, binding, state, frame, cancelled).map_err(
                    |error| match error {
                        WorkflowInvocationError::Cancelled => WorkflowInvocationError::Cancelled,
                        WorkflowInvocationError::Failed(error) => WorkflowInvocationError::Failed(
                            WorkflowBoundCallError::Invocation(error),
                        ),
                    },
                )
            },
            cancelled,
            step_limit,
        )
    }

    fn execute_nodes<State, Error>(
        &self,
        state: &mut State,
        frame: Option<&mut crate::WorkflowFrame>,
        invoke: impl FnMut(
            &str,
            &InterfaceId,
            &mut State,
            Option<&mut crate::WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        self.execute_cooperative(state, frame, invoke, cancelled, step_limit)
    }
}

#[path = "workflow_scheduler.rs"]
mod scheduler;

#[cfg(test)]
mod inclusion_tests {
    use super::*;

    fn owner() -> ComponentId {
        ComponentId::parse("fixture.subplan-owner").unwrap()
    }

    fn service(name: &str, outcomes: &[(&str, WorkflowEdge)]) -> WorkflowNode {
        WorkflowNode {
            import: InterfaceId::parse(name).unwrap(),
            branches: outcomes
                .iter()
                .map(|(name, edge)| ((*name).to_owned(), edge.clone()))
                .collect(),
        }
    }

    fn include(workflow: &str, site: &str, on_exit: &[(&str, WorkflowEdge)]) -> WorkflowEdge {
        WorkflowEdge::Include {
            workflow: workflow.to_owned(),
            site: site.to_owned(),
            on_exit: on_exit
                .iter()
                .map(|(name, edge)| ((*name).to_owned(), edge.clone()))
                .collect(),
        }
    }

    fn selected() -> BTreeMap<(ComponentId, String), WorkflowTopology> {
        let child = WorkflowTopology {
            entry: "work".into(),
            nodes: BTreeMap::from([(
                "work".into(),
                service("fixture.child@1", &[("returned", WorkflowEdge::Finish)]),
            )]),
        };
        let main = WorkflowTopology {
            entry: "start".into(),
            nodes: BTreeMap::from([
                (
                    "start".into(),
                    service(
                        "fixture.start@1",
                        &[(
                            "delegate",
                            include(
                                "child",
                                "one",
                                &[(
                                    "returned",
                                    WorkflowEdge::Next {
                                        node: "after".into(),
                                    },
                                )],
                            ),
                        )],
                    ),
                ),
                (
                    "after".into(),
                    service("fixture.after@1", &[("done", WorkflowEdge::Finish)]),
                ),
            ]),
        };
        BTreeMap::from([
            ((owner(), "main".into()), main),
            ((owner(), "child".into()), child),
        ])
    }

    #[test]
    fn mapped_subplan_can_return_directly_to_parent_finish() {
        let mut all = selected();
        let parent = all.get_mut(&(owner(), "main".into())).unwrap();
        parent.nodes.remove("after");
        parent.nodes.get_mut("start").unwrap().branches.insert(
            "delegate".into(),
            WorkflowEdge::IncludeMapped {
                workflow: "child".into(),
                site: "terminal".into(),
                inputs: BTreeMap::new(),
                outputs: BTreeMap::from([(
                    crate::Key::parse("child_output").unwrap(),
                    crate::Key::parse("parent_output").unwrap(),
                )]),
                on_exit: BTreeMap::from([("returned".into(), WorkflowEdge::Finish)]),
            },
        );
        let topology = WorkflowTopology::inline_selected(&owner(), "main", &all).unwrap();
        assert!(matches!(
            topology.nodes["__include__/terminal/work"].branches["returned"],
            WorkflowEdge::FinishTransfer { .. }
        ));
        let compiled = topology.compile(|_| true).unwrap();
        let child_output = crate::Key::parse("child_output").unwrap();
        let parent_output = crate::Key::parse("parent_output").unwrap();
        let schema = crate::WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([
                (child_output.clone(), crate::Type::U64),
                (parent_output.clone(), crate::Type::U64),
            ]),
        };
        compiled.validate_frame_schema(&schema).unwrap();
        let mut frame = crate::WorkflowFrame::new(
            schema,
            BTreeMap::from([
                (child_output.clone(), crate::PhenixValue::U64(0)),
                (parent_output.clone(), crate::PhenixValue::U64(0)),
            ]),
        )
        .unwrap();
        let report = compiled
            .execute_nodes(
                &mut (),
                Some(&mut frame),
                |node, _, _, frame, _| {
                    if node == "__include__/terminal/work" {
                        frame
                            .unwrap()
                            .set(&child_output, crate::PhenixValue::U64(42))
                            .unwrap();
                        Ok::<_, WorkflowInvocationError<String>>("returned".to_owned())
                    } else {
                        assert_eq!(node, "start");
                        Ok::<_, WorkflowInvocationError<String>>("delegate".to_owned())
                    }
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(report.final_outcome, "returned");
        assert_eq!(report.executed_nodes, 2);
        assert_eq!(
            frame.get(&parent_output),
            Some(&crate::PhenixValue::U64(42))
        );
    }

    #[test]
    fn independent_parent_return_transfer_composes_with_child_outputs() {
        let mut all = selected();
        let parent = all.get_mut(&(owner(), "main".into())).unwrap();
        parent.nodes.get_mut("start").unwrap().branches.insert(
            "delegate".into(),
            WorkflowEdge::IncludeMapped {
                workflow: "child".into(),
                site: "composed".into(),
                inputs: BTreeMap::new(),
                outputs: BTreeMap::from([(
                    crate::Key::parse("child_output").unwrap(),
                    crate::Key::parse("parent_output").unwrap(),
                )]),
                on_exit: BTreeMap::from([(
                    "returned".into(),
                    WorkflowEdge::Transfer {
                        node: "after".into(),
                        slots: BTreeMap::from([(
                            crate::Key::parse("extra_input").unwrap(),
                            crate::Key::parse("extra_output").unwrap(),
                        )]),
                    },
                )]),
            },
        );
        let flattened = WorkflowTopology::inline_selected(&owner(), "main", &all).unwrap();
        let WorkflowEdge::Transfer { node, slots } =
            &flattened.nodes["__include__/composed/work"].branches["returned"]
        else {
            panic!("expected a single compiled return transfer");
        };
        assert_eq!(node, "after");
        assert_eq!(slots.len(), 2);
        let compiled = flattened.compile(|_| true).unwrap();
        let schema = crate::WorkflowFrameSchema {
            revision: 1,
            slots: [
                "child_output",
                "parent_output",
                "extra_input",
                "extra_output",
            ]
            .into_iter()
            .map(|name| (crate::Key::parse(name).unwrap(), crate::Type::U64))
            .collect(),
        };
        compiled.validate_frame_schema(&schema).unwrap();
        let mut frame = crate::WorkflowFrame::new(
            schema,
            [
                ("child_output", 0_u64),
                ("parent_output", 0),
                ("extra_input", 17),
                ("extra_output", 0),
            ]
            .into_iter()
            .map(|(name, value)| {
                (
                    crate::Key::parse(name).unwrap(),
                    crate::PhenixValue::U64(value),
                )
            })
            .collect(),
        )
        .unwrap();
        compiled
            .execute_nodes(
                &mut (),
                Some(&mut frame),
                |node, _, _, frame, _| {
                    if node == "__include__/composed/work" {
                        frame
                            .unwrap()
                            .set(
                                &crate::Key::parse("child_output").unwrap(),
                                crate::PhenixValue::U64(9),
                            )
                            .unwrap();
                        Ok::<_, WorkflowInvocationError<String>>("returned".into())
                    } else if node == "start" {
                        Ok("delegate".into())
                    } else {
                        Ok("done".into())
                    }
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(
            frame.get(&crate::Key::parse("parent_output").unwrap()),
            Some(&crate::PhenixValue::U64(9))
        );
        assert_eq!(
            frame.get(&crate::Key::parse("extra_output").unwrap()),
            Some(&crate::PhenixValue::U64(17))
        );
        // A downstream mapping that reads a freshly written return target
        // requires a second phase. Reject it rather than reading stale data.
        let parent = all.get_mut(&(owner(), "main".into())).unwrap();
        let WorkflowEdge::IncludeMapped { on_exit, .. } = parent
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .get_mut("delegate")
            .unwrap()
        else {
            unreachable!()
        };
        on_exit.insert(
            "returned".into(),
            WorkflowEdge::Transfer {
                node: "after".into(),
                slots: BTreeMap::from([(
                    crate::Key::parse("parent_output").unwrap(),
                    crate::Key::parse("extra_output").unwrap(),
                )]),
            },
        );
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::InvalidFrameTransfer { .. })
        ));
    }

    #[test]
    fn included_fork_join_finish_returns_to_parent_via_declared_outcome() {
        let mut all = selected();
        all.insert(
            (owner(), "fork-child".into()),
            WorkflowTopology {
                entry: "fork".into(),
                nodes: BTreeMap::from([
                    (
                        "fork".into(),
                        service(
                            "fixture.fork@1",
                            &[(
                                "spawn",
                                WorkflowEdge::Fork {
                                    branches: BTreeMap::from([("worker".into(), "work".into())]),
                                    policy: crate::WorkflowJoinPolicy::All(
                                        crate::WorkflowJoinAllPolicy::CollectAll,
                                    ),
                                    outputs: BTreeMap::new(),
                                    on_success: Box::new(WorkflowEdge::Finish),
                                    on_failure: Box::new(WorkflowEdge::Finish),
                                },
                            )],
                        ),
                    ),
                    (
                        "work".into(),
                        service("fixture.child@1", &[("done", WorkflowEdge::Finish)]),
                    ),
                ]),
            },
        );
        let parent = all.get_mut(&(owner(), "main".into())).unwrap();
        parent.nodes.get_mut("start").unwrap().branches.insert(
            "delegate".into(),
            include(
                "fork-child",
                "subfork",
                &[
                    (
                        "spawn/success",
                        WorkflowEdge::Next {
                            node: "after".into(),
                        },
                    ),
                    ("spawn/failure", WorkflowEdge::Finish),
                ],
            ),
        );
        let compiled = WorkflowTopology::inline_selected(&owner(), "main", &all)
            .unwrap()
            .compile(|_| true)
            .unwrap();
        let mut frame = crate::WorkflowFrame::new(
            crate::WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::new(),
            },
            BTreeMap::new(),
        )
        .unwrap();
        let mut invoked = Vec::new();
        let report = compiled
            .execute_nodes(
                &mut invoked,
                Some(&mut frame),
                |node, _, visited, _, _| {
                    visited.push(node.to_owned());
                    Ok::<_, WorkflowInvocationError<String>>(match node {
                        "start" => "delegate".into(),
                        "__include__/subfork/fork" => "spawn".into(),
                        "__include__/subfork/work" => "done".into(),
                        "after" => "done".into(),
                        _ => panic!("unexpected node {node}"),
                    })
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(report.final_outcome, "done");
        assert_eq!(report.executed_nodes, 4);
        assert_eq!(
            invoked,
            [
                "start",
                "__include__/subfork/fork",
                "__include__/subfork/work",
                "after"
            ]
        );
        let parent = all.get_mut(&(owner(), "main".into())).unwrap();
        let WorkflowEdge::Include { on_exit, .. } = parent
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .get_mut("delegate")
            .unwrap()
        else {
            unreachable!()
        };
        on_exit.remove("spawn/failure");
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::MissingSubplanExit { outcome, .. })
                if outcome == "spawn/failure"
        ));
    }

    #[test]
    fn included_map_join_finishes_into_parent_without_losing_ordered_results() {
        let mut all = selected();
        let key = |name: &str| crate::Key::parse(name).unwrap();
        all.insert(
            (owner(), "mapped-child".into()),
            WorkflowTopology {
                entry: "batch".into(),
                nodes: BTreeMap::from([
                    (
                        "batch".into(),
                        service(
                            "fixture.batch@1",
                            &[(
                                "launch",
                                WorkflowEdge::MapFork {
                                    collection: key("items"),
                                    item_slot: key("item"),
                                    child_output_slot: key("result"),
                                    output_slot: key("results"),
                                    max_children: 8,
                                    branch_entry: "item-task".into(),
                                    policy: crate::WorkflowJoinPolicy::All(
                                        crate::WorkflowJoinAllPolicy::CollectAll,
                                    ),
                                    on_success: Box::new(WorkflowEdge::Finish),
                                    on_failure: Box::new(WorkflowEdge::Finish),
                                },
                            )],
                        ),
                    ),
                    (
                        "item-task".into(),
                        service("fixture.item@1", &[("done", WorkflowEdge::Finish)]),
                    ),
                ]),
            },
        );
        all.get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .insert(
                "delegate".into(),
                include(
                    "mapped-child",
                    "batch-return",
                    &[
                        (
                            "launch/success",
                            WorkflowEdge::Next {
                                node: "after".into(),
                            },
                        ),
                        ("launch/failure", WorkflowEdge::Finish),
                    ],
                ),
            );
        let compiled = WorkflowTopology::inline_selected(&owner(), "main", &all)
            .unwrap()
            .compile(|_| true)
            .unwrap();
        let schema = crate::WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([
                (key("items"), crate::Type::List(Box::new(crate::Type::U64))),
                (key("item"), crate::Type::U64),
                (key("result"), crate::Type::U64),
                (
                    key("results"),
                    crate::Type::List(Box::new(crate::Type::U64)),
                ),
            ]),
        };
        compiled.validate_frame_schema(&schema).unwrap();
        let mut frame = crate::WorkflowFrame::new(
            schema,
            BTreeMap::from([
                (
                    key("items"),
                    crate::PhenixValue::List(vec![
                        crate::PhenixValue::U64(2),
                        crate::PhenixValue::U64(5),
                    ]),
                ),
                (key("item"), crate::PhenixValue::U64(0)),
                (key("result"), crate::PhenixValue::U64(0)),
                (key("results"), crate::PhenixValue::List(Vec::new())),
            ]),
        )
        .unwrap();
        let mut order = Vec::new();
        let report = compiled
            .execute_nodes(
                &mut order,
                Some(&mut frame),
                |node, _, visited, local, _| {
                    visited.push(node.to_owned());
                    if node == "__include__/batch-return/item-task" {
                        let frame = local.unwrap();
                        let crate::PhenixValue::U64(value) = frame.get(&key("item")).unwrap()
                        else {
                            panic!("worker was not given its item");
                        };
                        frame
                            .set(&key("result"), crate::PhenixValue::U64(value * 3))
                            .unwrap();
                        Ok::<_, WorkflowInvocationError<String>>("done".into())
                    } else if node == "start" {
                        Ok("delegate".into())
                    } else if node == "__include__/batch-return/batch" {
                        Ok("launch".into())
                    } else {
                        assert_eq!(node, "after");
                        Ok("done".into())
                    }
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(report.final_outcome, "done");
        assert_eq!(report.executed_nodes, 5);
        assert_eq!(
            order,
            [
                "start",
                "__include__/batch-return/batch",
                "__include__/batch-return/item-task",
                "__include__/batch-return/item-task",
                "after",
            ]
        );
        assert_eq!(
            frame.get(&key("results")),
            Some(&crate::PhenixValue::List(vec![
                crate::PhenixValue::U64(6),
                crate::PhenixValue::U64(15),
            ]))
        );
    }

    #[test]
    fn included_plan_runs_on_the_same_invoke_exit_executor() {
        let all = selected();
        let flattened = WorkflowTopology::inline_selected(&owner(), "main", &all).unwrap();
        assert_eq!(flattened.nodes.len(), 3);
        assert!(flattened.nodes.contains_key("__include__/one/work"));
        assert!(flattened.nodes.values().all(|node| {
            node.branches
                .values()
                .all(|edge| !matches!(edge, WorkflowEdge::Include { .. }))
        }));
        let compiled = flattened.compile(|_| true).unwrap();
        let mut order = Vec::new();
        let report = compiled
            .execute(
                &mut order,
                |import, visited| {
                    visited.push(import.as_str().to_owned());
                    Ok::<_, ()>(match import.as_str() {
                        "fixture.start@1" => "delegate".into(),
                        "fixture.child@1" => "returned".into(),
                        "fixture.after@1" => "done".into(),
                        unexpected => panic!("unexpected import {unexpected}"),
                    })
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(
            order,
            vec!["fixture.start@1", "fixture.child@1", "fixture.after@1"]
        );
        assert_eq!(report.last_node, "after");
        assert_eq!(report.final_outcome, "done");
        assert_eq!(report.executed_nodes, 3);
        assert!(matches!(
            all[&(owner(), "main".to_owned())].nodes["start"].branches["delegate"],
            WorkflowEdge::Include { .. }
        ));
    }

    #[test]
    fn nested_inclusion_and_child_terminal_handoff_are_compiled() {
        let mut all = selected();
        all.insert(
            (owner(), "leaf".into()),
            WorkflowTopology {
                entry: "finish".into(),
                nodes: BTreeMap::from([(
                    "finish".into(),
                    service("fixture.leaf@1", &[("leaf_done", WorkflowEdge::Finish)]),
                )]),
            },
        );
        all.get_mut(&(owner(), "child".into()))
            .unwrap()
            .nodes
            .get_mut("work")
            .unwrap()
            .branches
            .insert(
                "returned".into(),
                include("leaf", "inner", &[("leaf_done", WorkflowEdge::Finish)]),
            );
        all.get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .insert(
                "delegate".into(),
                include(
                    "child",
                    "outer",
                    &[(
                        "leaf_done",
                        WorkflowEdge::Next {
                            node: "after".into(),
                        },
                    )],
                ),
            );
        let flattened = WorkflowTopology::inline_selected(&owner(), "main", &all).unwrap();
        assert!(
            flattened
                .nodes
                .contains_key("__include__/outer/__include__/inner/finish")
        );
        let compiled = flattened.compile(|_| true).unwrap();
        let report = compiled
            .execute(
                &mut (),
                |import, _| {
                    Ok::<_, ()>(match import.as_str() {
                        "fixture.start@1" => "delegate".into(),
                        "fixture.child@1" => "returned".into(),
                        "fixture.leaf@1" => "leaf_done".into(),
                        "fixture.after@1" => "done".into(),
                        unexpected => panic!("unexpected import {unexpected}"),
                    })
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(report.executed_nodes, 4);
        assert_eq!(report.final_outcome, "done");
    }

    #[test]
    fn recursive_and_unresolved_subplans_fail_closed() {
        let mut all = selected();
        all.get_mut(&(owner(), "child".into()))
            .unwrap()
            .nodes
            .get_mut("work")
            .unwrap()
            .branches
            .insert(
                "returned".into(),
                include("main", "back", &[("done", WorkflowEdge::Finish)]),
            );
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::RecursiveSubplan(_))
        ));
        let mut missing = selected();
        missing
            .get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .insert(
                "delegate".into(),
                include("not-selected", "one", &[("returned", WorkflowEdge::Finish)]),
            );
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &missing),
            Err(WorkflowCompileError::MissingSubplan(plan)) if plan == "not-selected"
        ));
    }

    #[test]
    fn incomplete_return_mapping_and_identity_conflicts_reject() {
        let mut all = selected();
        all.get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .insert("delegate".into(), include("child", "one", &[]));
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::MissingSubplanExit { outcome, .. }) if outcome == "returned"
        ));
        let mut all = selected();
        all.get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .insert(
                "__include__/one/work".into(),
                service("fixture.conflict@1", &[("done", WorkflowEdge::Finish)]),
            );
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::InclusionIdentityConflict(_))
        ));
    }

    #[test]
    fn duplicate_sites_and_unknown_child_exits_reject() {
        let mut all = selected();
        all.get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .get_mut("after")
            .unwrap()
            .branches
            .insert(
                "second".into(),
                include("child", "one", &[("returned", WorkflowEdge::Finish)]),
            );
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::DuplicateInclusionSite(site)) if site == "one"
        ));
        let mut all = selected();
        all.get_mut(&(owner(), "main".into()))
            .unwrap()
            .nodes
            .get_mut("start")
            .unwrap()
            .branches
            .insert(
                "delegate".into(),
                include(
                    "child",
                    "one",
                    &[
                        ("returned", WorkflowEdge::Finish),
                        ("unknown", WorkflowEdge::Finish),
                    ],
                ),
            );
        assert!(matches!(
            WorkflowTopology::inline_selected(&owner(), "main", &all),
            Err(WorkflowCompileError::UnknownSubplanExit { outcome, .. }) if outcome == "unknown"
        ));
    }

    #[test]
    fn unmapped_include_cannot_enter_unbound_compiler() {
        let topology = selected().remove(&(owner(), "main".into())).unwrap();
        assert!(matches!(
            topology.compile(|_| true),
            Err(WorkflowCompileError::UnexpandedInclude { .. })
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(name: &str) -> InterfaceId {
        InterfaceId::parse(name).expect("fixture service id")
    }

    fn node(name: &str, branches: &[(&str, WorkflowEdge)]) -> (String, WorkflowNode) {
        (
            name.into(),
            WorkflowNode {
                import: interface(&format!("fixture.{name}@1")),
                branches: branches
                    .iter()
                    .map(|(key, value)| ((*key).into(), value.clone()))
                    .collect(),
            },
        )
    }

    fn sample() -> WorkflowTopology {
        WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([
                node(
                    "model",
                    &[
                        (
                            "tools",
                            WorkflowEdge::Next {
                                node: "tool".into(),
                            },
                        ),
                        ("final", WorkflowEdge::Finish),
                    ],
                ),
                node(
                    "tool",
                    &[(
                        "done",
                        WorkflowEdge::Next {
                            node: "model".into(),
                        },
                    )],
                ),
            ]),
        }
    }

    #[test]
    fn excessive_nested_scope_depth_rejects_at_compilation() {
        let mut nodes = BTreeMap::new();
        for index in 0..=65usize {
            let name = format!("nested-{index:02}");
            let edge = if index == 65 {
                WorkflowEdge::Finish
            } else {
                WorkflowEdge::Fork {
                    branches: BTreeMap::from([("only".into(), format!("nested-{:02}", index + 1))]),
                    policy: crate::WorkflowJoinPolicy::All(
                        crate::WorkflowJoinAllPolicy::CollectAll,
                    ),
                    outputs: BTreeMap::new(),
                    on_success: Box::new(WorkflowEdge::Finish),
                    on_failure: Box::new(WorkflowEdge::Finish),
                }
            };
            nodes.insert(
                name.clone(),
                WorkflowNode {
                    import: interface("fixture.nested@1"),
                    branches: BTreeMap::from([("done".into(), edge)]),
                },
            );
        }
        let plan = WorkflowTopology {
            entry: "nested-00".into(),
            nodes,
        };
        assert!(matches!(
            plan.compile(|_| true),
            Err(WorkflowCompileError::InvalidFork { reason, .. })
                if reason.contains("depth exceeds 64")
        ));
    }

    #[test]
    fn a_join_cannot_resume_inside_its_child_without_a_new_fork() {
        let topology = WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([
                node(
                    "model",
                    &[(
                        "spawn",
                        WorkflowEdge::Fork {
                            branches: BTreeMap::from([("one".into(), "child".into())]),
                            policy: crate::WorkflowJoinPolicy::All(
                                crate::WorkflowJoinAllPolicy::CollectAll,
                            ),
                            outputs: BTreeMap::new(),
                            on_success: Box::new(WorkflowEdge::Next {
                                node: "child".into(),
                            }),
                            on_failure: Box::new(WorkflowEdge::Finish),
                        },
                    )],
                ),
                node("child", &[("done", WorkflowEdge::Finish)]),
            ]),
        };
        assert!(matches!(
            topology.compile(|_| true),
            Err(WorkflowCompileError::InvalidFork { reason, .. })
                if reason.contains("re-enters child")
        ));
    }

    #[test]
    fn explicit_failure_is_not_synthesized_from_an_invocation_error() {
        let topology = WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([node("model", &[("failed", WorkflowEdge::Fail)])]),
        };
        let workflow = topology.compile(|_| true).unwrap();
        let report = workflow.execute(
            &mut (),
            |_, _| Ok::<String, String>("failed".into()),
            || false,
            None,
        );
        assert!(matches!(
            report,
            Err(WorkflowRunError::ExplicitFailure {
                node,
                outcome,
                executed_nodes: 1
            }) if node == "model" && outcome == "failed"
        ));
    }

    #[test]
    fn compiles_and_executes_loop_and_conditional_exit() {
        let workflow = sample().compile(|_| true).unwrap();
        let mut calls = Vec::new();
        let report = workflow
            .execute(
                &mut calls,
                |service, calls| {
                    calls.push(service.as_str().to_owned());
                    Ok::<_, String>(
                        match calls.len() {
                            1 => "tools",
                            2 => "done",
                            _ => "final",
                        }
                        .into(),
                    )
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(
            calls,
            ["fixture.model@1", "fixture.tool@1", "fixture.model@1"]
        );
        assert_eq!(report.last_node, "model");
        assert_eq!(report.final_outcome, "final");
        assert_eq!(report.executed_nodes, 3);
    }

    #[test]
    fn compiled_execution_plan_has_disjoint_invoke_and_exit_steps() {
        let compiled = sample().compile(|_| true).unwrap();
        assert!(matches!(
            compiled.plan.steps.get(&PlanStepId::Invoke("model".into())),
            Some(PlanStep::Invoke { .. })
        ));
        let exit = PlanStepId::Exit {
            node: "model".into(),
            outcome: "final".into(),
        };
        assert!(matches!(
            compiled.plan.steps.get(&exit),
            Some(PlanStep::Exit { failed: false })
        ));
        assert_ne!(exit, PlanStepId::Invoke("model".into()));
    }

    #[test]
    fn frozen_compiled_steps_are_authoritative_over_authoring_snapshot() {
        let mut compiled = sample().compile(|_| true).unwrap();
        // Deliberately break the retained authoring snapshot. Execution
        // must use only the validated, frozen plan and selected imports.
        compiled.topology.nodes.clear();

        let mut calls = Vec::new();
        let report = compiled
            .execute(
                &mut calls,
                |import, calls| {
                    calls.push(import.as_str().to_owned());
                    Ok::<_, ()>("final".into())
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(calls, ["fixture.model@1"]);
        assert_eq!(report.final_outcome, "final");
        assert_eq!(report.executed_nodes, 1);
    }

    #[test]
    fn cancellation_after_final_invoke_prevents_successful_exit() {
        let compiled = sample().compile(|_| true).unwrap();
        let mut calls = 0;
        let mut cancellation_checks = 0;
        let result = compiled.execute(
            &mut calls,
            |_, calls| {
                *calls += 1;
                Ok::<_, ()>("final".into())
            },
            || {
                cancellation_checks += 1;
                cancellation_checks == 2
            },
            None,
        );
        assert!(matches!(
            result,
            Err(WorkflowRunError::Cancelled {
                next_node,
                executed_nodes: 1,
            }) if next_node == "model"
        ));
        assert_eq!(calls, 1, "completed provider calls must never be replayed");
        assert_eq!(cancellation_checks, 2);
    }

    #[test]
    fn terminal_projection_preserves_distinct_outcomes_from_the_same_node() {
        let topology = WorkflowTopology {
            entry: "terminal".into(),
            nodes: BTreeMap::from([node(
                "terminal",
                &[
                    ("completed", WorkflowEdge::Finish),
                    ("aborted", WorkflowEdge::Finish),
                ],
            )]),
        };
        let compiled = topology.compile(|_| true).unwrap();
        for outcome in ["completed", "aborted"] {
            let report = compiled
                .execute(
                    &mut (),
                    |_, _| Ok::<_, ()>(outcome.to_owned()),
                    || false,
                    None,
                )
                .unwrap();
            assert_eq!(report.last_node, "terminal");
            assert_eq!(report.final_outcome, outcome);
            assert_eq!(report.executed_nodes, 1);
        }
    }

    #[test]
    fn checks_binding_and_missing_transitions_before_activation() {
        assert!(matches!(
            sample().compile(|service| service.as_str() != "fixture.tool@1"),
            Err(WorkflowCompileError::UnresolvedImport { node, .. }) if node == "tool"
        ));
        let mut missing = sample();
        missing.nodes.get_mut("model").unwrap().branches.insert(
            "tools".into(),
            WorkflowEdge::Next {
                node: "absent".into(),
            },
        );
        assert!(matches!(
            missing.compile(|_| true),
            Err(WorkflowCompileError::UnknownTarget { target, .. }) if target == "absent"
        ));
        let mut unreachable = sample();
        unreachable.entry = "tool".into();
        // The tool node loops to model, so both nodes remain reachable.
        assert!(unreachable.compile(|_| true).is_ok());
        let mut unreachable = sample();
        unreachable
            .nodes
            .get_mut("model")
            .unwrap()
            .branches
            .remove("tools");
        assert!(matches!(
            unreachable.compile(|_| true),
            Err(WorkflowCompileError::UnreachableNodes(nodes)) if nodes == ["tool"]
        ));
    }

    #[test]
    fn rejects_missing_entry_and_empty_branch() {
        let mut graph = sample();
        graph.entry = "missing".into();
        assert!(matches!(
            graph.compile(|_| true),
            Err(WorkflowCompileError::MissingEntry(_))
        ));
        let mut graph = sample();
        graph
            .nodes
            .get_mut("model")
            .unwrap()
            .branches
            .insert("".into(), WorkflowEdge::Finish);
        assert!(matches!(
            graph.compile(|_| true),
            Err(WorkflowCompileError::EmptyBranch { .. })
        ));
    }

    #[test]
    fn allows_service_only_cycle_without_an_exit_and_obeys_limits() {
        let topology = WorkflowTopology {
            entry: "poll".into(),
            nodes: BTreeMap::from([(
                "poll".into(),
                WorkflowNode {
                    import: interface("fixture.poll@1"),
                    branches: BTreeMap::from([(
                        "again".into(),
                        WorkflowEdge::Next {
                            node: "poll".into(),
                        },
                    )]),
                },
            )]),
        };
        let compiled = topology.compile(|_| true).unwrap();
        assert!(
            compiled
                .plan
                .steps
                .values()
                .all(|step| matches!(step, PlanStep::Invoke { .. }))
        );

        let mut calls = 0_u64;
        let limit = compiled.execute(
            &mut calls,
            |_, calls| {
                *calls += 1;
                Ok::<_, ()>("again".into())
            },
            || false,
            NonZeroU64::new(3),
        );
        assert!(matches!(
            limit,
            Err(WorkflowRunError::StepLimitReached {
                executed_nodes: 3,
                next_node,
            }) if next_node == "poll"
        ));
        assert_eq!(calls, 3);

        let mut checks = 0;
        let cancelled = compiled.execute(
            &mut calls,
            |_, calls| {
                *calls += 1;
                Ok::<_, ()>("again".into())
            },
            || {
                checks += 1;
                checks == 2
            },
            None,
        );
        assert!(matches!(
            cancelled,
            Err(WorkflowRunError::Cancelled {
                executed_nodes: 1,
                next_node,
            }) if next_node == "poll"
        ));
        assert_eq!(calls, 4);
    }

    #[test]
    fn permits_service_bound_long_lived_branch_with_separate_finish() {
        let mut graph = sample();
        graph.nodes.insert(
            "trap".into(),
            WorkflowNode {
                import: interface("fixture.trap@1"),
                branches: BTreeMap::from([(
                    "repeat".into(),
                    WorkflowEdge::Next {
                        node: "trap".into(),
                    },
                )]),
            },
        );
        graph.nodes.get_mut("model").unwrap().branches.insert(
            "trapped".into(),
            WorkflowEdge::Next {
                node: "trap".into(),
            },
        );
        assert!(graph.compile(|_| true).is_ok());
    }

    #[test]
    fn preserves_cancellation_limits_errors_and_unexpected_outcomes() {
        let graph = sample().compile(|_| true).unwrap();
        let mut calls = 0u64;
        let result = graph.execute(
            &mut calls,
            |_, calls| {
                *calls += 1;
                Ok::<_, ()>(if *calls % 2 == 1 { "tools" } else { "done" }.into())
            },
            || false,
            NonZeroU64::new(3),
        );
        assert!(matches!(
            result,
            Err(WorkflowRunError::StepLimitReached {
                executed_nodes: 3,
                ..
            })
        ));
        assert_eq!(calls, 3);
        let result = graph.execute(
            &mut (),
            |_, _| -> Result<String, ()> { panic!("cancelled call executed") },
            || true,
            None,
        );
        assert!(matches!(
            result,
            Err(WorkflowRunError::Cancelled {
                executed_nodes: 0,
                ..
            })
        ));
        let result = graph.execute(&mut (), |_, _| Err::<String, _>("failure"), || false, None);
        assert!(matches!(
            result,
            Err(WorkflowRunError::NodeFailed { node, error: "failure" }) if node == "model"
        ));
        let result = graph.execute(
            &mut (),
            |_, _| Ok::<_, ()>("unknown".into()),
            || false,
            None,
        );
        assert!(matches!(
            result,
            Err(WorkflowRunError::UndeclaredOutcome { .. })
        ));
    }

    #[test]
    fn topology_roundtrip_rejects_unknown_fields() {
        let original = sample();
        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: WorkflowTopology = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
        assert!(
            serde_json::from_str::<WorkflowTopology>(
                r#"{"entry":"foo","nodes":{},"runtime_specific_behavior":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn portable_topologies_reject_duplicate_node_and_outcome_identities() {
        // Both definitions are individually valid. Identical repeated JSON
        // keys must reject rather than silently selecting the last value.
        let duplicate_nodes = r#"{"entry":"model","nodes":{"model":{"import":"fixture.model@1","branches":{"final":{"kind":"finish"}}},"model":{"import":"fixture.model@1","branches":{"final":{"kind":"finish"}}}}}"#;
        assert!(serde_json::from_str::<WorkflowTopology>(duplicate_nodes).is_err());

        let duplicate_outcomes = r#"{"entry":"model","nodes":{"model":{"import":"fixture.model@1","branches":{"final":{"kind":"finish"},"final":{"kind":"finish"}}}}}"#;
        assert!(serde_json::from_str::<WorkflowTopology>(duplicate_outcomes).is_err());

        let valid = serde_json::to_vec(&sample()).unwrap();
        assert_eq!(
            serde_json::from_slice::<WorkflowTopology>(&valid).unwrap(),
            sample()
        );
    }

    #[test]
    fn binds_workflow_to_existing_component_imports() {
        use crate::{
            Authority, ComponentExport, ComponentImport, ComponentManifest, InterfaceSchema,
            PluginExecution, PluginId, PluginManifest,
        };

        let owner = PluginId::parse("fixture.workflow-owner").unwrap();
        let consumer = ComponentId::parse("fixture.workflow").unwrap();
        let provider = ComponentId::parse("fixture.model").unwrap();
        let interface = InterfaceId::parse("fixture.model@1").unwrap();
        let optional = InterfaceId::parse("fixture.optional@1").unwrap();
        let plugin = PluginManifest {
            id: owner.clone(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let graph = ResolvedComponentGraph::compile(
            [plugin],
            [
                ComponentManifest {
                    listeners: Vec::new(),
                    id: consumer.clone(),
                    owner: owner.clone(),
                    imports: vec![
                        ComponentImport {
                            interface: interface.clone(),
                            schema: InterfaceSchema::default(),
                            required: true,
                            authority: Authority::default(),
                        },
                        ComponentImport {
                            interface: optional.clone(),
                            schema: InterfaceSchema::default(),
                            required: false,
                            authority: Authority::default(),
                        },
                    ],
                    exports: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                ComponentManifest {
                    listeners: Vec::new(),
                    id: provider,
                    owner,
                    imports: Vec::new(),
                    exports: vec![ComponentExport {
                        interface: interface.clone(),
                        schema: InterfaceSchema::default(),
                        priority: 1,
                        required_authority: Authority::default(),
                    }],
                    maximum_authority: Authority::default(),
                },
            ],
            &Authority::default(),
        )
        .unwrap();

        let topology = WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([node("model", &[("done", WorkflowEdge::Finish)])]),
        };
        let compiled = topology
            .clone()
            .compile_for_component(&graph, &consumer)
            .unwrap();
        assert!(compiled.bound_import(&interface).is_some());
        let mut unbound = topology.clone();
        unbound.nodes.get_mut("model").unwrap().import = optional.clone();
        assert!(matches!(
            unbound.compile_for_component(&graph, &consumer),
            Err(WorkflowCompileError::UnresolvedImport { import, .. }) if import == optional
        ));
        let unrelated = ComponentId::parse("fixture.model").unwrap();
        assert!(matches!(
            topology.clone().compile_for_component(&graph, &unrelated),
            Err(WorkflowCompileError::ImportLookup { error, .. })
                if matches!(*error, ComponentGraphError::ImportNotDeclared { .. })
        ));
        let absent = ComponentId::parse("fixture.absent-component").unwrap();
        assert!(matches!(
            topology.compile_for_component(&graph, &absent),
            Err(WorkflowCompileError::ImportLookup { error, .. })
                if matches!(error.as_ref(), ComponentGraphError::UnknownComponent(component) if component == &absent)
        ));
    }

    #[test]
    fn generation_activates_only_valid_declared_workflows() {
        use crate::{
            Authority, ComponentExport, ComponentImport, ComponentManifest,
            GenerationResolutionError, InterfaceSchema, PluginExecution, PluginId, PluginManifest,
            ResolvedGeneration,
        };

        let owner = PluginId::parse("fixture.workflow-generation").unwrap();
        let provider_owner = PluginId::parse("fixture.workflow-provider-owner").unwrap();
        let consumer = ComponentId::parse("fixture.workflow-consumer").unwrap();
        let provider = ComponentId::parse("fixture.workflow-provider").unwrap();
        let interface = InterfaceId::parse("fixture.model@1").unwrap();
        let plugin = PluginManifest {
            id: owner.clone(),
            version: 1,
            execution: PluginExecution::ResourceOnly,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let provider_plugin = PluginManifest {
            id: provider_owner.clone(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let consumer_component = ComponentManifest {
            listeners: Vec::new(),
            id: consumer.clone(),
            owner: owner.clone(),
            imports: vec![ComponentImport {
                interface: interface.clone(),
                schema: InterfaceSchema::default(),
                required: true,
                authority: Authority::default(),
            }],
            exports: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let provider_component = ComponentManifest {
            listeners: Vec::new(),
            id: provider,
            owner: provider_owner,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface.clone(),
                schema: InterfaceSchema::default(),
                priority: 1,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        };
        // A topology-only owner may import contracts but may not export
        // executable providers under the resource-only execution mode.
        let mut forbidden_export = provider_component.clone();
        forbidden_export.owner = owner.clone();
        assert!(matches!(
            ResolvedGeneration::resolve(
                [plugin.clone(), provider_plugin.clone()],
                [consumer_component.clone(), forbidden_export],
                [],
                &Authority::default(),
            ),
            Err(GenerationResolutionError::ComponentGraph(
                crate::ComponentGraphError::ResourceOnlyComponentOwner { .. }
            ))
        ));
        let baseline = ResolvedGeneration::resolve(
            [plugin.clone(), provider_plugin.clone()],
            [consumer_component.clone(), provider_component.clone()],
            [],
            &Authority::default(),
        )
        .unwrap();
        let declaration = WorkflowDeclaration {
            owner: consumer.clone(),
            name: "turn".into(),
            topology: WorkflowTopology {
                entry: "model".into(),
                nodes: BTreeMap::from([node("model", &[("final", WorkflowEdge::Finish)])]),
            },
        };
        let selected = baseline
            .clone()
            .with_workflows([declaration.clone()])
            .unwrap();
        // Compiler/IR semantic revisions must participate in generation
        // identity: identical declarations under a different Core lowering
        // contract are not interchangeable pinned executions.
        let mut unversioned = baseline.clone();
        unversioned.incorporate_semantic_metadata(&vec![declaration.clone()]);
        assert_ne!(unversioned.generation(), selected.generation());
        assert_ne!(baseline.generation(), selected.generation());
        let bound = selected
            .generation_topology()
            .workflow(&consumer, "turn")
            .unwrap();
        assert!(bound.bound_import(&interface).is_some());
        assert_eq!(selected.workflows(), std::slice::from_ref(&declaration));

        // The candidate resolver, not just a unit-only compiler, lowers a
        // reusable subplan and binds its imports through the same graph.
        let mut child = declaration.clone();
        child.name = "child".into();
        let parent = WorkflowDeclaration {
            owner: consumer.clone(),
            name: "parent".into(),
            topology: WorkflowTopology {
                entry: "before".into(),
                nodes: BTreeMap::from([(
                    "before".into(),
                    WorkflowNode {
                        import: interface.clone(),
                        branches: BTreeMap::from([(
                            "delegate".into(),
                            WorkflowEdge::Include {
                                workflow: "child".into(),
                                site: "selected".into(),
                                on_exit: BTreeMap::from([("final".into(), WorkflowEdge::Finish)]),
                            },
                        )]),
                    },
                )]),
            },
        };
        let with_subplan = baseline
            .clone()
            .with_workflows([parent.clone(), child.clone()])
            .unwrap();
        assert!(
            with_subplan
                .generation_topology()
                .workflow(&consumer, "parent")
                .unwrap()
                .topology()
                .nodes
                .contains_key("__include__/selected/model")
        );
        assert_eq!(
            with_subplan.generation(),
            baseline
                .clone()
                .with_workflows([child.clone(), parent.clone()])
                .unwrap()
                .generation()
        );
        assert!(matches!(
            baseline.clone().with_workflows([parent]),
            Err(GenerationResolutionError::InvalidWorkflow { error, .. })
                if matches!(*error, WorkflowCompileError::MissingSubplan(_))
        ));

        // Declaration enumeration does not participate in generation identity.
        let mut additional = declaration.clone();
        additional.name = "extra".into();
        let forward = baseline
            .clone()
            .with_workflows([declaration.clone(), additional.clone()])
            .unwrap();
        let reverse = baseline
            .clone()
            .with_workflows([additional, declaration.clone()])
            .unwrap();
        assert_eq!(forward.generation(), reverse.generation());
        assert_eq!(forward.workflows(), reverse.workflows());
        assert_ne!(forward.generation(), selected.generation());

        // A reconfiguration cannot retain a live topology after its required
        // provider has disappeared. Do not search for an implicit replacement.
        assert!(
            selected
                .clone()
                .with_plugin_set(
                    [plugin.clone()].into(),
                    [consumer_component.clone()].into(),
                    Vec::new(),
                    Vec::new(),
                    &Authority::default(),
                )
                .is_err()
        );

        let same = selected
            .clone()
            .with_workflows([declaration.clone()])
            .unwrap();
        assert_eq!(same.generation(), selected.generation());
        assert!(matches!(
            selected.clone().with_workflows(std::iter::empty()),
            Err(GenerationResolutionError::WorkflowAlreadyBound)
        ));

        let mut unbound = declaration.clone();
        unbound.topology.nodes.get_mut("model").unwrap().import =
            InterfaceId::parse("fixture.missing@1").unwrap();
        assert!(matches!(
            baseline.clone().with_workflows([unbound]),
            Err(GenerationResolutionError::InvalidWorkflow { error, .. })
                if matches!(
                    error.as_ref(),
                    WorkflowCompileError::ImportLookup { error, .. }
                        if matches!(error.as_ref(), ComponentGraphError::ImportNotDeclared { .. })
                )
        ));
        assert!(matches!(
            baseline
                .clone()
                .with_workflows([declaration.clone(), declaration.clone()]),
            Err(GenerationResolutionError::DuplicateWorkflow { .. })
        ));
        // Duplicate ownership is rejected before validating either competing
        // body's imports. Reversing enumeration cannot change the error into
        // a provider-resolution failure.
        let mut conflicting = declaration.clone();
        conflicting.topology.nodes.get_mut("model").unwrap().import =
            InterfaceId::parse("fixture.unselected@1").unwrap();
        for declarations in [
            [conflicting.clone(), declaration.clone()],
            [declaration.clone(), conflicting.clone()],
        ] {
            assert!(matches!(
                baseline.clone().with_workflows(declarations),
                Err(GenerationResolutionError::DuplicateWorkflow { owner, name })
                    if owner == consumer && name == "turn"
            ));
        }

        // A stable topology owner can survive a new candidate selection.
        let retained = selected
            .clone()
            .with_plugin_set(
                [plugin.clone(), provider_plugin.clone()].into(),
                [consumer_component.clone(), provider_component.clone()].into(),
                Vec::new(),
                Vec::new(),
                &Authority::default(),
            )
            .unwrap();
        assert_eq!(retained.workflows(), selected.workflows());
        assert!(
            retained
                .generation_topology()
                .workflow(&consumer, "turn")
                .is_some()
        );

        // Replacing an independent provider does not retire an unchanged
        // topology owner; the canonical resolver selects its new binding.
        let mut upgraded_provider = provider_plugin.clone();
        upgraded_provider.version += 1;
        let rebound = selected
            .clone()
            .with_plugin_set(
                [plugin.clone(), upgraded_provider].into(),
                [consumer_component.clone(), provider_component.clone()].into(),
                Vec::new(),
                Vec::new(),
                &Authority::default(),
            )
            .unwrap();
        assert_eq!(rebound.workflows(), selected.workflows());
        assert!(
            rebound
                .generation_topology()
                .workflow(&consumer, "turn")
                .unwrap()
                .bound_import(&interface)
                .is_some()
        );
        assert_ne!(rebound.generation(), selected.generation());

        // The new plugin revision must re-publish the topology. Reusing the
        // same component ID and imports is not proof of identical authorship.
        let mut upgraded_owner = plugin.clone();
        upgraded_owner.version += 1;
        let upgraded = selected
            .clone()
            .with_plugin_set(
                [upgraded_owner, provider_plugin.clone()].into(),
                [consumer_component.clone(), provider_component.clone()].into(),
                Vec::new(),
                Vec::new(),
                &Authority::default(),
            )
            .unwrap();
        assert!(upgraded.workflows().is_empty());
        assert!(
            upgraded
                .generation_topology()
                .workflow(&consumer, "turn")
                .is_none()
        );

        // A second plugin cannot inherit a first plugin's workflow merely by
        // selecting the same component ID in a different generation.
        let mut new_owner = plugin.clone();
        new_owner.id = PluginId::parse("fixture.new-workflow-owner").unwrap();
        let mut reassigned = consumer_component.clone();
        reassigned.owner = new_owner.id.clone();
        let replaced = selected
            .clone()
            .with_plugin_set(
                [new_owner, provider_plugin.clone()].into(),
                [reassigned, provider_component.clone()].into(),
                Vec::new(),
                Vec::new(),
                &Authority::default(),
            )
            .unwrap();
        assert!(replaced.workflows().is_empty());
        assert!(
            replaced
                .generation_topology()
                .workflow(&consumer, "turn")
                .is_none()
        );

        // Removing the owner retires its topology with the next generation.
        let retired = selected
            .with_plugin_set(
                [provider_plugin].into(),
                [provider_component].into(),
                Vec::new(),
                Vec::new(),
                &Authority::default(),
            )
            .unwrap();
        assert!(
            retired
                .generation_topology()
                .workflow(&consumer, "turn")
                .is_none()
        );
        assert!(retired.workflows().is_empty());
    }

    #[test]
    fn bound_dispatch_requires_real_imports() {
        let raw = sample().compile(|_| true).unwrap();
        let result = raw.execute_bound(
            &mut (),
            |_, _, _, _, _| Ok::<_, WorkflowInvocationError<()>>("final".into()),
            || false,
            None,
        );
        assert!(matches!(
            result,
            Err(WorkflowRunError::NodeFailed {
                error: WorkflowBoundCallError::UnboundImport(_),
                ..
            })
        ));
    }

    #[test]
    fn provider_replacement_preserves_control_flow_topology() {
        use crate::{
            Authority, ComponentExport, ComponentImport, ComponentManifest, InterfaceSchema,
            PluginExecution, PluginId, PluginManifest, ProviderCompositionPolicy,
        };

        let owner = PluginId::parse("fixture.workflow-owner").unwrap();
        let consumer = ComponentId::parse("fixture.agent-topology").unwrap();
        let model = InterfaceId::parse("fixture.model@1").unwrap();
        let provider = |name: &str, priority: i32| {
            let id = ComponentId::parse(name).unwrap();
            ComponentManifest {
                id,
                owner: owner.clone(),
                imports: Vec::new(),
                exports: vec![ComponentExport {
                    interface: model.clone(),
                    schema: InterfaceSchema::default(),
                    priority,
                    required_authority: Authority::default(),
                }],
                listeners: Vec::new(),
                maximum_authority: Authority::default(),
            }
        };
        let plugin = PluginManifest {
            id: owner.clone(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let components = [
            ComponentManifest {
                id: consumer.clone(),
                owner: owner.clone(),
                imports: vec![ComponentImport {
                    interface: model.clone(),
                    schema: InterfaceSchema::default(),
                    required: true,
                    authority: Authority::default(),
                }],
                exports: Vec::new(),
                listeners: Vec::new(),
                maximum_authority: Authority::default(),
            },
            provider("fixture.basic-model", 100),
            provider("fixture.advanced-model", 10),
        ];
        let topology = WorkflowTopology {
            entry: "model".into(),
            nodes: BTreeMap::from([node("model", &[("final", WorkflowEdge::Finish)])]),
        };
        let mut observed = Vec::new();
        for id in ["fixture.basic-model", "fixture.advanced-model"] {
            let policy = ProviderCompositionPolicy::new()
                .with_explicit_binding(model.clone(), ComponentId::parse(id).unwrap());
            let graph = ResolvedComponentGraph::compile_with_provider_policy(
                [plugin.clone()],
                components.clone(),
                &Authority::default(),
                &policy,
            )
            .unwrap();
            let compiled = topology
                .clone()
                .compile_for_component(&graph, &consumer)
                .unwrap();
            assert_eq!(compiled.topology(), &topology);
            compiled
                .execute_bound(
                    &mut observed,
                    |node_name, _, binding, results, _| {
                        assert_eq!(node_name, "model");
                        results.push(binding.exporter().as_str().to_owned());
                        Ok::<_, WorkflowInvocationError<()>>("final".into())
                    },
                    || false,
                    None,
                )
                .unwrap();
        }
        assert_eq!(observed, ["fixture.basic-model", "fixture.advanced-model"]);
    }

    #[test]
    fn decorators_do_not_change_topology() {
        let compiled = sample().compile(|_| true).unwrap();
        let original = compiled.topology().clone();
        let mut trace = Vec::new();
        compiled
            .execute(
                &mut (),
                |service, _| {
                    trace.push(format!("before:{}", service.as_str()));
                    let outcome = if service.as_str() == "fixture.model@1" {
                        "final"
                    } else {
                        "done"
                    };
                    trace.push(format!("after:{}", service.as_str()));
                    Ok::<_, ()>(outcome.to_owned())
                },
                || false,
                None,
            )
            .unwrap();
        assert_eq!(trace, ["before:fixture.model@1", "after:fixture.model@1"]);
        assert_eq!(compiled.topology(), &original);
    }
}
