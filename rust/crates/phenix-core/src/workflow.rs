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
fn deserialize_unique_workflow_map<'de, D, V>(
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
    Next { node: String },
    Finish,
}

// Internal lowering of the legacy service topology into execution steps.
// An Exit key is structurally disjoint from author-supplied Invoke names,
// preserving terminal outcomes without inventing or colliding string IDs.
// Fork and Join will be added to the same representation in subsequent work.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PlanStepId {
    Invoke(String),
    Exit { node: String, outcome: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlanStep {
    Invoke {
        import: InterfaceId,
        on_result: BTreeMap<String, PlanStepId>,
    },
    Exit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoweredPlan {
    entry: PlanStepId,
    steps: BTreeMap<PlanStepId, PlanStep>,
}

impl LoweredPlan {
    fn from_topology(topology: &WorkflowTopology) -> Self {
        let mut steps = BTreeMap::new();
        for (name, node) in &topology.nodes {
            let mut on_result = BTreeMap::new();
            for (outcome, edge) in &node.branches {
                let target = match edge {
                    WorkflowEdge::Next { node } => PlanStepId::Invoke(node.clone()),
                    WorkflowEdge::Finish => {
                        let exit = PlanStepId::Exit {
                            node: name.clone(),
                            outcome: outcome.clone(),
                        };
                        steps.insert(exit.clone(), PlanStep::Exit);
                        exit
                    }
                };
                on_result.insert(outcome.clone(), target);
            }
            steps.insert(
                PlanStepId::Invoke(name.clone()),
                PlanStep::Invoke {
                    import: node.import.clone(),
                    on_result,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledWorkflow {
    topology: WorkflowTopology,
    plan: LoweredPlan,
    bindings: BTreeMap<InterfaceId, ResolvedImportHandle>,
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
}

impl WorkflowTopology {
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
                if let WorkflowEdge::Next { node: target } = edge
                    && !self.nodes.contains_key(target)
                {
                    return Err(WorkflowCompileError::UnknownTarget {
                        from: name.clone(),
                        target: target.clone(),
                    });
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
                if let WorkflowEdge::Next { node } = edge {
                    pending.push(node.clone());
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
        // A workflow can have no Exit when it is a long-running service loop.
        // Every cycle in this interim IR invokes a service, so cancellation
        // and optional step limits are checked on every back edge.
        let plan = LoweredPlan::from_topology(&self);
        Ok(CompiledWorkflow {
            topology: self,
            plan,
            bindings: BTreeMap::new(),
        })
    }
}

impl CompiledWorkflow {
    pub fn topology(&self) -> &WorkflowTopology {
        &self.topology
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
            |_, import, state, _| invoke(import, state).map_err(WorkflowInvocationError::Failed),
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
            |name, import, state, cancelled| {
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

    fn execute_nodes<State, Error>(
        &self,
        state: &mut State,
        mut invoke: impl FnMut(
            &str,
            &InterfaceId,
            &mut State,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        mut cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        let mut current = &self.plan.entry;
        let mut count = 0u64;
        loop {
            let step = &self.plan.steps[current];
            match step {
                PlanStep::Invoke { import, on_result } => {
                    let PlanStepId::Invoke(name) = current else {
                        unreachable!("compiled invoke step has a typed invoke identity")
                    };
                    if cancelled() {
                        return Err(WorkflowRunError::Cancelled {
                            next_node: name.clone(),
                            executed_nodes: count,
                        });
                    }
                    if step_limit.is_some_and(|limit| count >= limit.get()) {
                        return Err(WorkflowRunError::StepLimitReached {
                            next_node: name.clone(),
                            executed_nodes: count,
                        });
                    }
                    let outcome = match invoke(name, import, state, &mut cancelled) {
                        Ok(outcome) => outcome,
                        Err(WorkflowInvocationError::Cancelled) => {
                            return Err(WorkflowRunError::Cancelled {
                                next_node: name.clone(),
                                executed_nodes: count,
                            });
                        }
                        Err(WorkflowInvocationError::Failed(error)) => {
                            return Err(WorkflowRunError::NodeFailed {
                                node: name.clone(),
                                error,
                            });
                        }
                    };
                    count = count
                        .checked_add(1)
                        .ok_or(WorkflowRunError::StepCounterOverflow)?;
                    current = on_result.get(&outcome).ok_or_else(|| {
                        WorkflowRunError::UndeclaredOutcome {
                            node: name.clone(),
                            outcome,
                        }
                    })?;
                }
                PlanStep::Exit => {
                    let PlanStepId::Exit { node, outcome } = current else {
                        unreachable!("compiled exit step has a typed exit identity")
                    };
                    // Cancellation can arrive while the final service is executing.
                    // A terminal edge does not invoke another provider, so the
                    // pre-Invoke check alone would incorrectly report success.
                    // The call has already completed: preserve the invocation
                    // count and never attempt to roll back its side effects.
                    if cancelled() {
                        return Err(WorkflowRunError::Cancelled {
                            next_node: node.clone(),
                            executed_nodes: count,
                        });
                    }
                    return Ok(WorkflowRunReport {
                        last_node: node.clone(),
                        final_outcome: outcome.clone(),
                        executed_nodes: count,
                    });
                }
            }
        }
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
            Some(PlanStep::Exit)
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
        unversioned.incorporate_semantic_metadata("phenix.workflow-ir", &vec![declaration.clone()]);
        assert_ne!(unversioned.generation(), selected.generation());
        assert_ne!(baseline.generation(), selected.generation());
        let bound = selected
            .generation_topology()
            .workflow(&consumer, "turn")
            .unwrap();
        assert!(bound.bound_import(&interface).is_some());
        assert_eq!(selected.workflows(), std::slice::from_ref(&declaration));

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
        let envelope = serde_json::to_vec(&serde_json::json!([{
            "owner": owner.as_str(),
            "id": "fixture.workflow-record@1",
            "kind": "fixture.record-kind@1",
            "role": "provide",
            "payload": { "type": "string", "value": "frozen" }
        }]))
        .unwrap();
        let workflows_first = selected
            .clone()
            .with_portable_contributions([(&owner, envelope.as_slice())])
            .unwrap();
        let contributions_first = baseline
            .clone()
            .with_portable_contributions([(&owner, envelope.as_slice())])
            .unwrap()
            .with_workflows([declaration.clone()])
            .unwrap();
        assert_eq!(
            workflows_first.generation(),
            contributions_first.generation()
        );
        assert_eq!(
            workflows_first.portable_contributions(),
            contributions_first.portable_contributions(),
        );
        for candidate in [&workflows_first, &contributions_first] {
            assert!(
                candidate
                    .generation_topology()
                    .workflow(&consumer, "turn")
                    .unwrap()
                    .bound_import(&interface)
                    .is_some()
            );
            let inspection = crate::ResolvedGenerationInspection::from_resolved(candidate);
            assert_eq!(inspection.generation(), workflows_first.generation());
            assert_eq!(inspection.workflows(), std::slice::from_ref(&declaration));
            assert_eq!(
                inspection.portable_contributions(),
                candidate.portable_contributions(),
            );
        }
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
            Err(GenerationResolutionError::InvalidWorkflow {
                error: WorkflowCompileError::ImportLookup { error, .. },
                ..
            }) if matches!(*error, ComponentGraphError::ImportNotDeclared { .. })
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
