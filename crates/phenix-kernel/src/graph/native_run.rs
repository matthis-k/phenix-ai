//! Trusted native adapter for the *single* graph scheduler.
//! GraphSession decides readiness and delivery. This adapter only dispatches
//! selected operations and owns native ticket polling/cancellation/draining.
use super::{BoundAction, CompiledGraph, ExecutionReport, GraphSession};
use crate::{
    pending::{Call, Poll, Ticket},
    runtime::{Outcome, Root},
};
use phenix_contract::{Code, Failure, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRunStatus {
    Running,
    Draining,
    Succeeded,
    Domain,
    Cancelled,
    Failed,
}

struct PendingActivation {
    ticket: Ticket,
    logical_done: bool,
}

/// One admitted root, one compiled graph, one graph execution session.
///
/// A graph node path must be present in the host-verified action binding map.
/// Its untrusted `Node::action` string never independently grants permission.
pub struct NativeGraphRun<'graph, 'kernel> {
    root: Root<'kernel>,
    session: GraphSession<'graph>,
    actions: BTreeMap<String, BoundAction>,
    pending: BTreeMap<u64, PendingActivation>,
    cancelled: bool,
    domain: Option<Value>,
    failure: Option<Failure>,
}

impl<'graph, 'kernel> NativeGraphRun<'graph, 'kernel> {
    pub fn start(
        graph: &'graph CompiledGraph,
        root: Root<'kernel>,
        actions: BTreeMap<String, BoundAction>,
        trigger: &str,
        payload: Value,
    ) -> Result<Self, Failure> {
        Ok(Self {
            root,
            session: graph.start(trigger, payload)?,
            actions,
            pending: BTreeMap::new(),
            cancelled: false,
            domain: None,
            failure: None,
        })
    }

    pub fn root(&self) -> &Root<'kernel> { &self.root }
    pub fn domain(&self) -> Option<&Value> { self.domain.as_ref() }
    pub fn failure(&self) -> Option<&Failure> { self.failure.as_ref() }
    pub fn in_flight(&self) -> usize { self.pending.len() }

    pub fn status(&self) -> NativeRunStatus {
        if !self.pending.is_empty() || !self.session.settled() {
            if self.cancelled || self.failure.is_some() {
                NativeRunStatus::Draining
            } else {
                NativeRunStatus::Running
            }
        } else if self.failure.is_some() {
            NativeRunStatus::Failed
        } else if self.domain.is_some() {
            NativeRunStatus::Domain
        } else if self.cancelled {
            NativeRunStatus::Cancelled
        } else {
            NativeRunStatus::Succeeded
        }
    }

    /// Stop scheduling and revoke every outstanding native ticket.
    /// Roots and physical leases remain owned until the tickets drain.
    pub fn cancel(&mut self) -> Result<(), Failure> {
        if !self.cancelled {
            self.cancelled = true;
            self.session.cancel();
            for pending in self.pending.values() {
                self.root.cancel(&pending.ticket)?;
            }
        }
        Ok(())
    }

    fn native_result(&mut self, activation: u64, outcome: Outcome) -> Result<(), Failure> {
        match outcome {
            Outcome::Success(value) => {
                if let Err(failure) = self.session.complete(
                    activation, [("value".into(), value)].into(),
                ) {
                    self.failure.get_or_insert(failure);
                    self.cancel()?;
                }
            }
            Outcome::Domain(value) => {
                // Domain outcomes are not promoted to successes or sent to
                // arbitrary graph output ports. Root supervision owns them.
                if self.domain.is_none() {
                    self.domain = Some(value);
                }
                self.cancel()?;
                self.session.complete(activation, BTreeMap::new())?;
            }
        }
        Ok(())
    }

    fn native_failure(&mut self, activation: u64, failure: Failure) -> Result<(), Failure> {
        self.session.fail(activation, failure.clone())?;
        self.failure.get_or_insert(failure);
        self.cancel()
    }

    /// Advance existing tickets before admitting additional ready graph nodes.
    /// A physically live lease is never discarded merely because the graph
    /// has cancelled or logically finished its activation.
    pub fn pump(&mut self, activation_limit: Option<usize>) -> Result<NativeRunStatus, Failure> {
        let ticket_ids: Vec<u64> = self.pending.keys().copied().collect();
        for id in ticket_ids {
            let mut pending = self.pending.remove(&id)
                .expect("ticket key was captured from this run");
            if !pending.logical_done {
                match self.root.poll(&pending.ticket)? {
                    Poll::Pending => {}
                    Poll::Ready(Ok(outcome)) => {
                        pending.logical_done = true;
                        self.native_result(id, outcome)?;
                    }
                    Poll::Ready(Err(failure)) => {
                        pending.logical_done = true;
                        self.native_failure(id, failure)?;
                    }
                    Poll::Cancelled { .. } => {
                        pending.logical_done = true;
                        self.cancel()?;
                        self.session.complete(id, BTreeMap::new())?;
                    }
                    Poll::Draining => return Err(Failure::new(
                        Code::InfrastructureFailure,
                        "graph ticket entered draining without a delivered outcome",
                    )),
                }
            }
            if pending.logical_done {
                match self.root.release(&pending.ticket) {
                    Ok(()) => continue,
                    Err(failure) if failure.code == Code::ScopeRejected
                        && failure.message == "physical lease is still live" =>
                    {
                        // A delivered/cancelled ticket can still have a native
                        // child to poll. Advancing it is required for release.
                        let _ = self.root.poll(&pending.ticket)?;
                    }
                    Err(failure) => return Err(failure),
                }
            }
            self.pending.insert(id, pending);
        }

        if !self.cancelled && self.failure.is_none() && self.domain.is_none() {
            while let Some(activation) = self.session.next_ready(activation_limit)? {
                let binding = self.actions.get(&activation.node).ok_or_else(|| {
                    Failure::new(Code::MissingBinding, "graph node has no verified native binding")
                });
                let bound = match binding {
                    Ok(bound) if bound.selected.key() == activation.action => bound,
                    Ok(_) => {
                        self.native_failure(activation.id, Failure::new(
                            Code::Denied, "graph action does not match verified selected operation",
                        ))?;
                        break;
                    }
                    Err(failure) => {
                        self.native_failure(activation.id, failure)?;
                        break;
                    }
                };
                let request = if activation.args.len() == 1 {
                    activation.args.into_values().next().expect("one argument")
                } else {
                    Value::Map(activation.args)
                };
                match self.root.call_graph(
                    &bound.declaring_owner, &bound.selected, &request, true,
                ) {
                    Ok(Call::Ready(outcome)) => self.native_result(activation.id, outcome)?,
                    Ok(Call::Pending(ticket)) => {
                        self.pending.insert(activation.id, PendingActivation {
                            ticket, logical_done: false,
                        });
                    }
                    Err(failure) => {
                        self.native_failure(activation.id, failure)?;
                        break;
                    }
                }
                if self.cancelled { break; }
            }
        }
        Ok(self.status())
    }

    /// Returns only when logical graph work *and* native physical leases have
    /// settled. A cancelled/domain result is deliberately distinct from success.
    pub fn finish(self) -> Result<(NativeRunStatus, ExecutionReport, Option<Value>), Failure> {
        let status = self.status();
        if matches!(status, NativeRunStatus::Running | NativeRunStatus::Draining) {
            return Err(Failure::new(Code::ScopeRejected,
                "native graph execution still has live work"));
        }
        if let Some(failure) = self.failure { return Err(failure); }
        let report = self.session.finish()?;
        Ok((status, report, self.domain))
    }
}
