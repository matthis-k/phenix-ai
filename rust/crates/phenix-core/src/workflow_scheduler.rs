//! Structured cooperative plan execution.
//!
//! Every cursor is one root-owned scope with an independently staged frame.
//! Forks schedule *one child invocation per turn*, including nested forks:
//! a nested child is never allowed to monopolize its outer sibling's turns.
//! No worker is detached and no provider resolver or generation is recreated.
//! The kernel root owns all pinned invocation and generation leases.
use super::{
    CompiledWorkflow, InterfaceId, InvokePosition, NonZeroU64, PlanStep, PlanStepId,
    WorkflowInvocationError, WorkflowInvokeAdvance, WorkflowInvokePoll, WorkflowRunError,
    WorkflowRunReport,
};
use crate::{
    PhenixValue, WorkflowChildSettlement, WorkflowFrame, WorkflowJoinDecision,
    WorkflowJoinObservation, WorkflowJoinPolicy,
};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ACTIVE_SCOPE_DEPTH: usize = 64;

/// Typed child-scope identities must remain disjoint even when plugin-authored
/// branch or node names contain '/', ':', or escape characters.
fn child_scope(parent: &str, kind: &str, node: &str, branch: &str) -> String {
    fn segment(value: &str) -> String {
        use std::fmt::Write as _;
        let mut encoded = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
                encoded.push(char::from(byte));
            } else {
                write!(&mut encoded, "%{byte:02X}").expect("writing to a string cannot fail");
            }
        }
        encoded
    }
    format!("{parent}/{kind}/{}/{}", segment(node), segment(branch))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TickStatus {
    Progress,
    Blocked,
    Settled(WorkflowChildSettlement),
}

struct Cursor {
    step: PlanStepId,
    scope: String,
    submitted: bool,
    frame: Option<WorkflowFrame>,
    /// Suspended parent frames, one per currently entered included subplan.
    subplan_parents: Vec<(String, WorkflowFrame)>,
    active: Option<Box<ActiveFork>>,
    join_decision: Option<WorkflowJoinDecision>,
}

impl Cursor {
    fn at(step: PlanStepId, frame: Option<WorkflowFrame>, scope: String) -> Self {
        Self {
            step,
            scope,
            submitted: false,
            frame,
            subplan_parents: Vec::new(),
            active: None,
            join_decision: None,
        }
    }
}

struct ActiveFork {
    join: PlanStepId,
    children: BTreeMap<String, Cursor>,
    ready_order: Vec<String>,
    settled: BTreeSet<String>,
    observations: Vec<WorkflowJoinObservation>,
    next_index: usize,
    settlement_order: u64,
}

impl ActiveFork {
    fn next_ready(&mut self) -> Option<String> {
        if self.ready_order.is_empty() {
            return None;
        }
        for _ in 0..self.ready_order.len() {
            let index = self.next_index;
            self.next_index = (self.next_index + 1) % self.ready_order.len();
            let name = &self.ready_order[index];
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
        let PlanStep::Fork {
            branches,
            map,
            join,
        } = &self.plan.steps[&cursor.step]
        else {
            unreachable!("admission starts only at a compiled Fork");
        };
        let frame =
            cursor
                .frame
                .as_ref()
                .ok_or_else(|| WorkflowRunError::StructuredFrameRequired {
                    node: node.to_owned(),
                })?;
        let mut children = branches
            .iter()
            .map(|(key, step)| {
                (key.clone(), {
                    let mut child = Cursor::at(
                        step.clone(),
                        Some(frame.clone()),
                        child_scope(&cursor.scope, "fork", node, key),
                    );
                    child.subplan_parents = cursor.subplan_parents.clone();
                    child
                })
            })
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
                children.insert(format!("{index:020}"), {
                    let mut child = Cursor::at(
                        map.branch_entry.clone(),
                        Some(snapshot),
                        child_scope(&cursor.scope, "map", node, &index.to_string()),
                    );
                    child.subplan_parents = cursor.subplan_parents.clone();
                    child
                });
            }
        }
        Ok(ActiveFork {
            join: join.clone(),
            ready_order: children.keys().cloned().collect(),
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
                candidate.collect_from(source, slots).map_err(|error| {
                    WorkflowRunError::InvalidJoinFrame {
                        node: node.to_owned(),
                        error,
                    }
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

    /// Apply one transition against exactly the selected execution frame.
    /// The four-step IR is unchanged: includes are compiler-owned boundary
    /// metadata, not provider calls or independent executors.
    fn transition_frame<E>(
        &self,
        cursor: &mut Cursor,
        target: &PlanStepId,
        slots: Option<&BTreeMap<crate::Key, crate::Key>>,
    ) -> Result<(), WorkflowRunError<E>> {
        let target_node = match target {
            PlanStepId::Invoke(node)
            | PlanStepId::Fork { node, .. }
            | PlanStepId::Join { node, .. }
            | PlanStepId::Exit { node, .. } => node,
        };
        let is_root_exit = cursor.scope == "root" && matches!(target, PlanStepId::Exit { .. });
        let target_scope = if is_root_exit {
            None
        } else {
            self.scope_for_node(target_node)
                .map(|scope| scope.prefix.clone())
        };
        // Only the destination lineage remains live. A child finishing
        // through a parent continuation must publish its declared outputs
        // before that parent is allowed to execute the next provider.
        let mut remaining = slots.cloned().unwrap_or_default();
        while let Some((prefix, _)) = cursor.subplan_parents.last() {
            if target_scope
                .as_ref()
                .is_some_and(|destination| destination.starts_with(prefix))
            {
                break;
            }
            let (retiring, mut parent) = cursor
                .subplan_parents
                .pop()
                .expect("entered scope retains one parent snapshot");
            let subplan = &self.scoped_subplans[&retiring];
            let child = cursor.frame.take().expect("selected child has a frame");
            parent
                .publish_subplan(&child, &subplan.outputs)
                .map_err(|error| WorkflowRunError::InvalidTransitionFrame {
                    node: target_node.clone(),
                    error,
                })?;
            // The compiler's flat IncludeMapped edge also carries exactly
            // these child-to-parent output aliases. They are now published
            // through the isolated frames rather than re-applied as a flat
            // transfer; unrelated parent-owned transfers remain ordered.
            for (source, target) in &subplan.outputs {
                if remaining.get(source) == Some(target) {
                    remaining.remove(source);
                }
            }
            cursor.frame = Some(parent);
        }

        let mut entering = self
            .scoped_subplans
            .values()
            .filter(|scope| {
                target_scope
                    .as_ref()
                    .is_some_and(|target| target.starts_with(&scope.prefix))
                    && !cursor
                        .subplan_parents
                        .iter()
                        .any(|(prefix, _)| prefix == &scope.prefix)
            })
            .collect::<Vec<_>>();
        entering.sort_by_key(|scope| scope.prefix.len());
        for scope in entering {
            if target_node != &scope.entry {
                return Err(WorkflowRunError::InvalidTransitionFrame {
                    node: target_node.clone(),
                    error: crate::WorkflowFrameError::IncompatibleSchema,
                });
            }
            let parent =
                cursor
                    .frame
                    .take()
                    .ok_or_else(|| WorkflowRunError::StructuredFrameRequired {
                        node: target_node.clone(),
                    })?;
            let child = parent
                .isolate_subplan(scope.schema.clone(), &scope.inputs, scope.initial.clone())
                .map_err(|error| WorkflowRunError::InvalidTransitionFrame {
                    node: target_node.clone(),
                    error,
                })?;
            cursor.subplan_parents.push((scope.prefix.clone(), parent));
            cursor.frame = Some(child);
            for (source, target) in &scope.inputs {
                if remaining.get(source) == Some(target) {
                    remaining.remove(source);
                }
            }
        }
        if !remaining.is_empty() {
            let frame =
                cursor
                    .frame
                    .as_mut()
                    .ok_or_else(|| WorkflowRunError::StructuredFrameRequired {
                        node: target_node.clone(),
                    })?;
            frame.transfer_slots(&remaining).map_err(|error| {
                WorkflowRunError::InvalidTransitionFrame {
                    node: target_node.clone(),
                    error,
                }
            })?;
        }
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
            &str,
            &mut State,
            Option<&mut WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> WorkflowInvokePoll<Error>,
        cancelled: &mut impl FnMut() -> bool,
        cancel_scope: &mut impl FnMut(&str),
        budget: (&mut u64, &mut u64, Option<NonZeroU64>, usize),
    ) -> Result<TickStatus, WorkflowRunError<Error>> {
        let (count, admissions, step_limit, depth) = budget;
        // Invoke performs its own single pre-dispatch cancellation check.
        // A redundant check here would change cancellation ordering and
        // turn terminal-after-invoke cancellation into a pre-call failure.
        if !matches!(&self.plan.steps[&cursor.step], PlanStep::Invoke { .. }) && cancelled() {
            return Err(WorkflowRunError::Cancelled {
                next_node: match &cursor.step {
                    PlanStepId::Invoke(node)
                    | PlanStepId::Fork { node, .. }
                    | PlanStepId::Join { node, .. }
                    | PlanStepId::Exit { node, .. } => node.clone(),
                },
                executed_nodes: *count,
            });
        }
        match &self.plan.steps[&cursor.step] {
            PlanStep::Invoke { .. } => {
                match self.invoke_step(
                    InvokePosition {
                        step: &cursor.step,
                        scope: &cursor.scope,
                        already_submitted: cursor.submitted,
                    },
                    state,
                    cursor.frame.as_mut(),
                    invoke,
                    cancelled,
                    (count, admissions, step_limit),
                )? {
                    WorkflowInvokeAdvance::Next { step, outcome } => {
                        let slots = match &self.plan.steps[&cursor.step] {
                            PlanStep::Invoke { transfers, .. } => transfers.get(&outcome),
                            _ => unreachable!("Invoke advance starts at an Invoke"),
                        };
                        self.transition_frame(cursor, &step, slots)?;
                        cursor.step = step;
                        cursor.submitted = false;
                    }
                    WorkflowInvokeAdvance::Started => {
                        cursor.submitted = true;
                        return Ok(TickStatus::Progress);
                    }
                    WorkflowInvokeAdvance::Waiting => return Ok(TickStatus::Blocked),
                }
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
                    return Ok(TickStatus::Progress);
                }
                // Poll at most one turn of each live child. This lets an
                // already-running sibling complete while another native
                // callback remains suspended; no busy-spin is permitted.
                let mut progress = false;
                for _ in 0..active.ready_order.len() {
                    let branch = active
                        .next_ready()
                        .expect("an undecided join always has a runnable child");
                    let status = self.tick(
                        active
                            .children
                            .get_mut(&branch)
                            .expect("selected child exists"),
                        state,
                        invoke,
                        cancelled,
                        cancel_scope,
                        (count, admissions, step_limit, depth + 1),
                    )?;
                    match status {
                        TickStatus::Blocked => continue,
                        TickStatus::Progress => {
                            progress = true;
                            break;
                        }
                        TickStatus::Settled(settlement) => {
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
                            progress = true;
                            break;
                        }
                    }
                }
                let PlanStep::Join { policy, .. } = &self.plan.steps[&active.join] else {
                    unreachable!("Fork refers to a Join");
                };
                let admitted = active.children.keys().cloned().collect();
                let decision = policy
                    .decide(&admitted, &active.observations)
                    .map_err(|error| WorkflowRunError::InvalidJoin {
                        node: node.clone(),
                        error,
                    })?;
                if decision == WorkflowJoinDecision::Pending {
                    cursor.active = Some(active);
                    return Ok(if progress {
                        TickStatus::Progress
                    } else {
                        TickStatus::Blocked
                    });
                }
                // An early winner does not permit abandoning in-flight
                // native tasks. Signal unselected siblings under their
                // structured child scopes; actual worker leases remain
                // pinned until their callbacks settle.
                let cancel_remaining = match &decision {
                    WorkflowJoinDecision::Pending => false,
                    WorkflowJoinDecision::Succeeded {
                        cancel_remaining, ..
                    }
                    | WorkflowJoinDecision::Failed {
                        cancel_remaining, ..
                    } => *cancel_remaining,
                };
                if cancel_remaining {
                    for (branch, child) in &active.children {
                        if !active.settled.contains(branch) {
                            cancel_scope(&child.scope);
                        }
                    }
                }
                if let WorkflowJoinDecision::Succeeded { selected, .. } = &decision {
                    self.join_frame(&node, cursor, &active, selected)?;
                }
                cursor.step = active.join;
                cursor.join_decision = Some(decision);
                return Ok(TickStatus::Progress);
            }
            PlanStep::Join {
                on_success,
                on_failure,
                on_success_transfer,
                on_failure_transfer,
                ..
            } => {
                let (next, transfer) = match cursor
                    .join_decision
                    .take()
                    .expect("Join must follow the settlement of its Fork")
                {
                    WorkflowJoinDecision::Succeeded { .. } => (on_success, on_success_transfer),
                    WorkflowJoinDecision::Failed { .. } => (on_failure, on_failure_transfer),
                    WorkflowJoinDecision::Pending => {
                        unreachable!("Join cannot resume pending scope")
                    }
                };
                // Join outputs are staged first. A mapped subplan return
                // publishes only declared child fields into its parent frame.
                self.transition_frame(cursor, next, transfer.as_ref())?;
                cursor.step = next.clone();
            }
            PlanStep::Exit { .. } => {}
        }
        if let PlanStep::Exit { failed } = &self.plan.steps[&cursor.step] {
            // Cancellation can arrive while the final provider is running.
            // An immediate terminal edge cannot report success in that case.
            if cancelled() {
                let PlanStepId::Exit { node, .. } = &cursor.step else {
                    unreachable!("terminal settlement has an Exit identity")
                };
                return Err(WorkflowRunError::Cancelled {
                    next_node: node.clone(),
                    executed_nodes: *count,
                });
            }
            return Ok(TickStatus::Settled(if *failed {
                WorkflowChildSettlement::Failed
            } else {
                WorkflowChildSettlement::Completed
            }));
        }
        Ok(TickStatus::Progress)
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
        cancelled: impl FnMut() -> bool,
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        self.execute_driven(
            state,
            frame,
            |node, import, _, state, frame, cancellation| {
                WorkflowInvokePoll::Ready(invoke(node, import, state, frame, cancellation))
            },
            cancelled,
            (
                |_| {},
                || unreachable!("a synchronous Invoke cannot suspend"),
            ),
            step_limit,
        )
    }

    /// Drive the *same* lowered Invoke/Fork/Join/Exit state machine with a
    /// pending-capable node boundary. Nothing changes the selected providers,
    /// frame ownership, join policy, or normal outcome vocabulary.
    pub(crate) fn execute_suspending<State, Error>(
        &self,
        state: &mut State,
        frame: Option<&mut WorkflowFrame>,
        invoke: impl FnMut(
            &str,
            &InterfaceId,
            &str,
            &mut State,
            Option<&mut WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> WorkflowInvokePoll<Error>,
        cancelled: impl FnMut() -> bool,
        callbacks: (impl FnMut(&str), impl FnMut()),
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        self.execute_driven(state, frame, invoke, cancelled, callbacks, step_limit)
    }

    fn execute_driven<State, Error>(
        &self,
        state: &mut State,
        frame: Option<&mut WorkflowFrame>,
        mut invoke: impl FnMut(
            &str,
            &InterfaceId,
            &str,
            &mut State,
            Option<&mut WorkflowFrame>,
            &mut dyn FnMut() -> bool,
        ) -> WorkflowInvokePoll<Error>,
        mut cancelled: impl FnMut() -> bool,
        callbacks: (impl FnMut(&str), impl FnMut()),
        step_limit: Option<NonZeroU64>,
    ) -> Result<WorkflowRunReport, WorkflowRunError<Error>> {
        let (mut cancel_scope, mut wait_for_settlement) = callbacks;
        let mut root = Cursor::at(
            self.plan.entry.clone(),
            frame.as_deref().cloned(),
            "root".into(),
        );
        let mut count = 0u64;
        let mut admissions = 0u64;
        let result = (|| {
            loop {
                match self.tick(
                    &mut root,
                    state,
                    &mut invoke,
                    &mut cancelled,
                    &mut cancel_scope,
                    (&mut count, &mut admissions, step_limit, 0),
                )? {
                    TickStatus::Progress => {}
                    TickStatus::Blocked => wait_for_settlement(),
                    TickStatus::Settled(_) => {
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
            }
        })();
        if result.is_err() {
            cancel_scope("root");
        }
        if let Some(destination) = frame {
            if result.is_err() {
                // A failed included plan cannot replace its caller's frame
                // with private child state or partially published outputs.
                if let Some((_, original)) = root.subplan_parents.first() {
                    *destination = original.clone();
                } else if let Some(last_frame) = root.frame {
                    *destination = last_frame;
                }
            } else if let Some(last_frame) = root.frame {
                *destination = last_frame;
            }
        }
        result
    }
}
