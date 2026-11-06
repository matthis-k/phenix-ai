//! Trusted native adapter for the *single* graph scheduler.
//! GraphSession decides readiness and delivery. This adapter only dispatches
//! selected operations and owns native ticket polling/cancellation/draining.
use super::{BoundAction, CompiledGraph, ExecutionReport, GraphSession};
use crate::{
    pending::{Call, Poll, Ticket},
    runtime::{Outcome, Root},
};
use phenix_contract::{Code, Failure, Schema, Value};
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

impl NativeRunStatus {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Domain | Self::Cancelled | Self::Failed
        )
    }
}

struct PendingActivation {
    ticket: Ticket,
    node: String,
    logical_done: bool,
}

/// Convert a selected native operation's response into named graph outputs.
/// For one port, the entire response is that port's value (even if it is a Map).
/// For multiple ports, the operation returns a Map keyed by declared port names.
fn decode_native_outputs(
    ports: &BTreeMap<String, Schema>,
    value: Value,
) -> Result<BTreeMap<String, Value>, Failure> {
    match ports.len() {
        0 if value == Value::Null => Ok(BTreeMap::new()),
        0 => Err(Failure::new(Code::SchemaRejected,
            "outputless graph node returned a value")),
        1 => Ok([(ports.keys().next().expect("one port").clone(), value)]
            .into()),
        _ => match value {
            Value::Map(fields) => {
                if fields.keys().any(|port| !ports.contains_key(port)) {
                    return Err(Failure::new(Code::SchemaRejected,
                        "native node returned an undeclared output port"));
                }
                Ok(fields)
            }
            _ => Err(Failure::new(Code::SchemaRejected,
                "multiple graph outputs require a named value map")),
        },
    }
}

/// One admitted root, one compiled graph, one graph execution session.
///
/// A graph node path must be present in the host-verified action binding map.
/// Its untrusted `Node::action` string never independently grants permission.
pub struct NativeGraphRun<'graph, 'kernel> {
    graph: &'graph CompiledGraph,
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
            graph,
            root,
            session: graph.start(trigger, payload)?,
            actions,
            pending: BTreeMap::new(),
            cancelled: false,
            domain: None,
            failure: None,
        })
    }

    pub fn root(&self) -> &Root<'kernel> {
        &self.root
    }
    pub fn domain(&self) -> Option<&Value> {
        self.domain.as_ref()
    }
    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
    }
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }
    pub fn activations(&self) -> usize {
        self.session.activations()
    }

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
        }
        // Physical cancellation must be attempted for every outstanding native
        // ticket even if another ticket's cancellation fails. Already-delivered
        // tickets only need physical draining/release.
        let mut first_failure = None;
        for pending in self.pending.values() {
            if pending.logical_done {
                continue;
            }
            if let Err(failure) = self.root.cancel(&pending.ticket) {
                if failure.code == Code::ScopeRejected
                    && failure.message == "ticket is already cancelled or quiescent"
                {
                    continue;
                }
                first_failure.get_or_insert(failure);
            }
        }
        if let Some(failure) = first_failure {
            return Err(failure);
        }
        Ok(())
    }

    fn native_result(
        &mut self,
        activation: u64,
        node_path: &str,
        outcome: Outcome,
    ) -> Result<(), Failure> {
        match outcome {
            Outcome::Success(value) => {
                let decoded = self.graph.nodes().get(node_path)
                    .ok_or_else(|| Failure::new(
                        Code::InfrastructureFailure,
                        "native activation has no compiled node",
                    ))
                    .and_then(|node| decode_native_outputs(&node.outputs, value));
                let outputs = match decoded {
                    Ok(outputs) => outputs,
                    Err(failure) => return self.native_failure(activation, failure),
                };
                if let Err(failure) = self.session.complete(activation, outputs) {
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
    /// A quota limits this pump call only; it is not a wave execution budget.
    pub fn pump(&mut self, dispatch_quota: Option<usize>) -> Result<NativeRunStatus, Failure> {
        let ticket_ids: Vec<u64> = self.pending.keys().copied().collect();
        for id in ticket_ids {
            let mut pending = self
                .pending
                .remove(&id)
                .expect("ticket key was captured from this run");
            // Every failure path restores the pending ticket before returning,
            // so Root teardown can never silently abandon a native lease.
            let advanced = (|| -> Result<bool, Failure> {
                if !pending.logical_done {
                    match self.root.poll(&pending.ticket)? {
                        Poll::Pending => {}
                        Poll::Ready(Ok(outcome)) => {
                            pending.logical_done = true;
                            self.native_result(id, &pending.node, outcome)?;
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
                        Poll::Draining => {
                            pending.logical_done = true;
                            self.native_failure(
                                id,
                                Failure::new(
                                    Code::InfrastructureFailure,
                                    "graph ticket lost a previously delivered outcome",
                                ),
                            )?;
                        }
                    }
                }
                if pending.logical_done {
                    match self.root.release(&pending.ticket) {
                        Ok(()) => return Ok(true),
                        Err(failure)
                            if failure.code == Code::ScopeRejected
                                && failure.message == "physical lease is still live" =>
                        {
                            // The result has already been delivered. Polling
                            // again advances nested native children to quiescence.
                            let _ = self.root.poll(&pending.ticket)?;
                        }
                        Err(failure) => return Err(failure),
                    }
                }
                Ok(false)
            })();
            match advanced {
                Ok(true) => {}
                Ok(false) => {
                    self.pending.insert(id, pending);
                }
                Err(failure) => {
                    self.pending.insert(id, pending);
                    return Err(failure);
                }
            }
        }

        if !self.cancelled && self.failure.is_none() && self.domain.is_none() {
            let mut dispatched = 0usize;
            while dispatch_quota.is_none_or(|max| dispatched < max) {
                let Some(activation) = self.session.next_ready(None)? else {
                    break;
                };
                dispatched += 1;
                let binding = self.actions.get(&activation.node).ok_or_else(|| {
                    Failure::new(
                        Code::MissingBinding,
                        "graph node has no verified native binding",
                    )
                });
                let bound = match binding {
                    Ok(bound) if bound.selected.key() == activation.action => bound,
                    Ok(_) => {
                        self.native_failure(
                            activation.id,
                            Failure::new(
                                Code::Denied,
                                "graph action does not match verified selected operation",
                            ),
                        )?;
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
                match self
                    .root
                    .call_graph(&bound.declaring_owner, &bound.selected, &request, true)
                {
                    Ok(Call::Ready(outcome)) => {
                        self.native_result(activation.id, &activation.node, outcome)?
                    }
                    Ok(Call::Pending(ticket)) => {
                        self.pending.insert(
                            activation.id,
                            PendingActivation {
                                ticket,
                                node: activation.node,
                                logical_done: false,
                            },
                        );
                    }
                    Err(failure) => {
                        self.native_failure(activation.id, failure)?;
                        break;
                    }
                }
                if self.cancelled {
                    break;
                }
            }
        }
        Ok(self.status())
    }

    /// Returns only when logical graph work *and* native physical leases have
    /// settled. A cancelled/domain result is deliberately distinct from success.
    pub fn finish(self) -> Result<(NativeRunStatus, ExecutionReport, Option<Value>), Failure> {
        let status = self.status();
        if matches!(status, NativeRunStatus::Running | NativeRunStatus::Draining) {
            return Err(Failure::new(
                Code::ScopeRejected,
                "native graph execution still has live work",
            ));
        }
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        let report = self.session.finish()?;
        Ok((status, report, self.domain))
    }
}

#[cfg(test)]
mod output_projection_tests {
    use super::*;
    use phenix_contract::ValueSchema;
    fn schema() -> Schema {
        Schema::of(&ValueSchema::I64).unwrap()
    }
    #[test]
    fn one_named_port_preserves_a_structured_native_payload() {
        let ports = [("reply".to_owned(), schema())].into();
        let payload = Value::map([("nested", Value::I64(3))]);
        let result = decode_native_outputs(&ports, payload.clone()).unwrap();
        assert_eq!(result["reply"], payload);
    }
    #[test]
    fn multiple_named_ports_are_independently_emitted() {
        let ports = [("reply".to_owned(), schema()), ("usage".to_owned(), schema())].into();
        let result = decode_native_outputs(
            &ports,
            Value::map([("reply", Value::I64(8))]),
        ).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["reply"], Value::I64(8));
        assert!(decode_native_outputs(
            &ports, Value::map([("unknown", Value::I64(3))])
        ).is_err());
        assert!(decode_native_outputs(&ports, Value::I64(8)).is_err());
    }
    #[test]
    fn outputless_nodes_emit_nothing() {
        let ports = BTreeMap::new();
        assert!(decode_native_outputs(&ports, Value::Null).unwrap().is_empty());
        assert!(decode_native_outputs(&ports, Value::I64(1)).is_err());
    }
}
