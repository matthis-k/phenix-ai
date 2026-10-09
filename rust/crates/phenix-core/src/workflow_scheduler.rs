//! Structured cooperative plan execution.
//!
//! Every cursor is one root-owned scope with an independently staged frame.
//! Forks schedule *one child invocation per turn*, including nested forks:
//! a nested child is never allowed to monopolize its outer sibling's turns.
//! No worker is detached and no provider resolver or generation is recreated.
//! The kernel root owns all pinned invocation and generation leases.
use super::{
    CompiledWorkflow, InterfaceId, NonZeroU64, PlanStep, PlanStepId,
    WorkflowChildSettlement, WorkflowInvocationError, WorkflowJoinDecision,
    WorkflowJoinObservation, WorkflowJoinPolicy, WorkflowRunError, WorkflowRunReport,
};
use crate::{PhenixValue, WorkflowFrame};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ACTIVE_SCOPE_DEPTH: usize = 64;

struct Cursor {
    step: PlanStepId,
    frame: Option<WorkflowFrame>,
    active: Option<Box<ActiveFork>>,
    join_decision: Option<WorkflowJoinDecision>,
}

impl Cursor {
    fn at(step: PlanStepId, frame: Option<WorkflowFrame>) -> Self {
        Self {
            step,
            frame,
            active: None,
            join_decision: None,
        }
    }
}

struct ActiveFork {
    join: PlanStepId,
    children: BTreeMap<String, Cursor>,
    settled: BTreeSet<String>,
    observations: Vec<WorkflowJoinObservation>,
    next_index: usize,
    settlement_order: u64,
}

impl ActiveFork {
    fn next_ready(&mut self) -> Option<String> {
        if self.children.is_empty() {
            return None;
        }
        for _ in 0..self.children.len() {
            let index = self.next_index;
            self.next_index = (self.next_index + 1) % self.children.len();
            let name = self
                .children
                .keys()
                .nth(index)
                .expect("index is bounded by admitted children");
            if !self.settled.contains(name) {
                return Some(name.clone());
            }
        }
        None
    }
}

impl CompiledWorkflow {
    fn admit_fork<E>(
        &self,
        node: &str,
        cursor: &Cursor,
    ) -> Result<ActiveFork, WorkflowRunError<E>> {
        let PlanStep::Fork { branches, map, join } = &self.plan.steps[&cursor.step] else {
            unreachable!("admission starts only at a compiled Fork");
        };
        let frame = cursor
            .frame
            .as_ref()
            .ok_or_else(|| WorkflowRunError::StructuredFrameRequired {
                node: node.to_owned(),
            })?;
        let mut children = branches
            .iter()
            .map(|(key, step)| (key.clone(), Cursor::at(step.clone(), Some(frame.clone()))))
            .collect::<BTreeMap<_, _>>();
        if let Some(map) = map {
            let items = match frame.get(&map.collection) {
                Some(PhenixValue::List(items)) => items,
                _ => {
                    return Err(WorkflowRunError::InvalidMapInput {
                        node: node.to_owned(),
                        reason: format!("map source {} must be a list", map.collection),
                    });
                }
            };
            if items.len() > map.max_children {
                return Err(WorkflowRunError::InvalidMapInput {
                    node: node.to_owned(),
                    reason: format!(
                        "map admits at most {} children; got {}",
                        map.max_children,
                        items.len()
                    ),
                });
            }
            let PlanStep::Join { policy, .. } = &self.plan.steps[join] else {
                unreachable!("Fork refers to a Join");
            };
            if items.is_empty() && !matches!(policy, WorkflowJoinPolicy::All(_)) {
                return Err(WorkflowRunError::InvalidMapInput {
                    node: node.to_owned(),
                    reason: "empty map requires an All join policy".into(),
                });
            }
            if let WorkflowJoinPolicy::Quorum(required) = policy
                && required.get() > items.len()
            {
                return Err(WorkflowRunError::InvalidMapInput {
                    node: node.to_owned(),
                    reason: format!(
                        "join requires {} children but map admitted {}",
                        required,
                        items.len()
                    ),
                });
            }
            for (index, item) in items.iter().enumerate() {
                let mut snapshot = frame.clone();
                snapshot
                    .set(&map.item_slot, item.clone())
                    .map_err(|error| WorkflowRunError::InvalidJoinFrame {
                        node: node.to_owned(),
                        error,
                    })?;
                children.insert(
                    format!("{index:06}"),
                    Cursor::at(map.branch_entry.clone(), Some(snapshot)),
                );
            }
        }
        Ok(ActiveFork {
            join: join.clone(),
            children,
            settled: BTreeSet::new(),
            observations: Vec::new(),
            next_index: 0,
            settlement_order: 0,
        })
    }

    fn join_frame<E>(
        &self,
        node: &str,
        parent: &mut Cursor,
        active: &ActiveFork,
        selected: &[String],
    ) -> Result<(), WorkflowRunError<E>> {
        let PlanStep::Join {
            outputs,
            map_output,
            ..
        } = &self.plan.steps[&active.join]
        else {
            unreachable!("Fork's target is a Join");
        };
        let base = parent
            .frame
            .as_ref()
            .expect("a Fork requires a previously validated frame");
        let mut candidate = base.clone();
        for child in selected {
            if let Some(slots) = outputs.get(child) {
                let source = active.children[child]
                    .frame
                    .as_ref()
                    .expect("a child inherits its parent's typed frame");
                candidate
                    .collect_from(source, slots)
                    .map_err(|error| WorkflowRunError::InvalidJoinFrame {
                        node: node.to_owned(),
                        error,
                    })?;
            }
        }
        if let Some((child_output, output_slot)) = map_output {
            // Map results have the input's stable ordinal order rather
            // than nondeterministic completion order.
            let mut ordered = selected.to_vec();
            ordered.sort();
            let values = ordered
                .iter()
                .map(|branch| {
                    active.children[branch]
                        .frame
                        .as_ref()
                        .and_then(|frame| frame.get(child_output))
                        .cloned()
                        .ok_or_else(|| WorkflowRunError::InvalidMapInput {
                            node: node.to_owned(),
                            reason: format!("missing map child output {child_output} in {branch}"),
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            candidate
                .set(output_slot, PhenixValue::List(values))
                .map_err(|error| WorkflowRunError::InvalidJoinFrame {
                    node: node.to_owned(),
                    error,
                })?;
        }
        parent.frame = Some(candidate);
        Ok(())
    }

    /// Advance a scope at most one provider invocation, including when a
    /// nested Fork is ready. Returning a terminal settlement never converts
    /// a provider or adapter error into a declared normal failure.
    fn tick<State, Error>(
        &self,
        cursor: &mut Cursor,
        state: &mut State,
        invoke: &mut impl FnMut(
            &str,
            &InterfaceId,
            &mut State,
            Option<&mut WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        cancelled: &mut impl FnMut() -> bool,
        budget: (&mut u64, Option<NonZeroU64>),
        depth: usize,
    ) -> Result<Option<WorkflowChildSettlement>, WorkflowRunError<Error>> {
        let (count, step_limit) = budget;
        if cancelled() {
            return Err(WorkflowRunError::Cancelled {
                next_node: format!("{:?}", cursor.step),
                executed_nodes: *count,
            });
        }
        match &self.plan.steps[&cursor.step] {
            PlanStep::Invoke { .. } => {
                cursor.step = self.invoke_step(
                    &cursor.step,
                    state,
                    cursor.frame.as_mut(),
                    invoke,
                    cancelled,
                    (count, step_limit),
                )?;
            }
            PlanStep::Fork { .. } => {
                let PlanStepId::Fork { node, .. } = &cursor.step else {
                    unreachable!("Fork identity points to Fork step");
                };
                let node = node.clone();
                if cursor.active.is_none() {
                    if depth >= MAX_ACTIVE_SCOPE_DEPTH {
                        return Err(WorkflowRunError::ScopeDepthExceeded {
                            node,
                            maximum: MAX_ACTIVE_SCOPE_DEPTH,
                        });
                    }
                    cursor.active = Some(Box::new(self.admit_fork(&node, cursor)?));
                }
                let mut active = cursor.active.take().expect("admitted fork is stored");
                if active.children.is_empty() {
                    // Vacuous All: the map bound and Join policy were checked
                    // before this step. Its output is an empty typed list.
                    self.join_frame(&node, cursor, &active, &[])?;
                    cursor.join_decision = Some(WorkflowJoinDecision::Succeeded {
                        selected: Vec::new(),
                        cancel_remaining: false,
                    });
                    cursor.step = active.join;
                    return Ok(None);
                }
                let branch = active
                    .next_ready()
                    .expect("an undecided join always has a runnable child");
                let settlement = self.tick(
                    active.children.get_mut(&branch).expect("selected child exists"),
                    state,
                    invoke,
                    cancelled,
                    (count, step_limit),
                    depth + 1,
                )?;
                if let Some(settlement) = settlement {
                    active.settled.insert(branch.clone());
                    active.observations.push(WorkflowJoinObservation {
                        branch,
                        order: active.settlement_order,
                        settlement,
                    });
                    active.settlement_order = active
                        .settlement_order
                        .checked_add(1)
                        .ok_or(WorkflowRunError::StepCounterOverflow)?;
                }
                let PlanStep::Join { policy, .. } = &self.plan.steps[&active.join] else {
                    unreachable!("Fork refers to a Join");
                };
                let admitted = active.children.keys().cloned().collect();
                let decision = policy.decide(&admitted, &active.observations)
                    .map_err(|error| WorkflowRunError::InvalidJoin {
                        node: node.clone(),
                        error,
                    })?;
                if decision == WorkflowJoinDecision::Pending {
                    cursor.active = Some(active);
                } else {
                    // Cooperative dispatch guarantees no child call is in
                    // flight here; dropping the remaining continuations
                    // settles sibling scopes before the parent resumes.
                    if let WorkflowJoinDecision::Succeeded { selected, .. } = &decision {
                        self.join_frame(&node, cursor, &active, selected)?;
                    }
                    cursor.step = active.join;
                    cursor.join_decision = Some(decision);
                }
                return Ok(None);
            }
            PlanStep::Join {
                on_success,
                on_failure,
                ..
            } => {
                cursor.step = match cursor
                    .join_decision
                    .take()
                    .expect("Join must follow the settlement of its Fork")
                {
                    WorkflowJoinDecision::Succeeded { .. } => on_success.clone(),
                    WorkflowJoinDecision::Failed { .. } => on_failure.clone(),
                    WorkflowJoinDecision::Pending => unreachable!("Join cannot resume pending scope"),
                };
            }
            PlanStep::Exit { .. } => {}
        }
        if let PlanStep::Exit { failed } = &self.plan.steps[&cursor.step] {
            return Ok(Some(if *failed {
                WorkflowChildSettlement::Failed
            } else {
                WorkflowChildSettlement::Completed
            }));
        }
        Ok(None)
    }

    pub(super) fn execute_cooperative<State, Error>(
        &self,
        state: &mut State,
        frame: Option<&mut WorkflowFrame>,
        mut invoke: impl FnMut(
            &str,
            &InterfaceId,
            &mut State,
            Option<&mut WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> Result<String, WorkflowInvocationError<Error>>,
        mut cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        let mut root = Cursor::at(self.plan.entry.clone(), frame.as_deref().cloned());
        let mut count = 0u64;
        let result = (|| {
            loop {
                if self.tick(
                    &mut root,
                    state,
                    &mut invoke,
                    &mut cancelled,
                    (&mut count, step_limit),
                    0,
                )?
                .is_some()
                {
                    let PlanStepId::Exit { node, outcome } = &root.step else {
                        unreachable!("a settled root has an Exit identity");
                    };
                    match &self.plan.steps[&root.step] {
                        PlanStep::Exit { failed: true } => {
                            return Err(WorkflowRunError::ExplicitFailure {
                                node: node.clone(),
                                outcome: outcome.clone(),
                                executed_nodes: count,
                            });
                        }
                        PlanStep::Exit { failed: false } => {
                            return Ok(WorkflowRunReport {
                                last_node: node.clone(),
                                final_outcome: outcome.clone(),
                                executed_nodes: count,
                            });
                        }
                        _ => unreachable!("only Exit settles the root"),
                    }
                }
            }
        })();
        if let (Some(destination), Some(last_frame)) = (frame, root.frame) {
            *destination = last_frame;
        }
        result
    }
}
