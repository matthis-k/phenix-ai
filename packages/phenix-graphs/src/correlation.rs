//! Opt-in fixed-lane correlation using ordinary compiled processes and queues.
use crate::runtime::{Readiness, ReadinessPolicy, ReadinessSnapshot};
use crate::{
    ComponentDef, Interface, NetworkBuilder, NetworkError, PortDef, Process, ProcessContext,
    ProcessEmission, ProcessFactory, Step,
};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Explicit capacity envelope for fixed-lane request correlation.
/// `max_live` must cover the application's maximum simultaneous correlations.
/// `distinct_scope_skew` is the maximum number of different request identities the
/// host may queue at one ingress before dispatch; it must fit both the live lanes
/// and ingress capacity. This is not a bound on repeated packets from one request.
///
/// Progress is only established for finite joins whose incomplete scopes contribute
/// at most one packet per input before becoming runnable. Repeated packets can fill
/// a lane and preserve strict FIFO backpressure in front of later requests. The
/// adapter neither reorders packets nor claims arbitrary-stream fairness.
#[derive(Clone, Copy, Debug)]
pub struct CorrelationBounds {
    pub max_live: NonZeroUsize,
    pub distinct_scope_skew: NonZeroUsize,
    pub ingress_capacity: NonZeroUsize,
    pub lane_capacity: NonZeroUsize,
}

/// Preserve the primitive interface at a reusable composite boundary. No factory
/// is spawned while authoring; the inner factory is spawned once per session.
pub fn bounded(
    interface: Arc<Interface>,
    inner: Arc<dyn ProcessFactory>,
    readiness: Readiness,
    bounds: CorrelationBounds,
) -> Result<ComponentDef, NetworkError> {
    if bounds.distinct_scope_skew > bounds.max_live
        || bounds.distinct_scope_skew > bounds.ingress_capacity
        || interface.inputs().is_empty()
    {
        return Err(NetworkError::InvalidProjection(
            "insufficient correlation bound: distinct-scope skew exceeds live lanes or ingress capacity, or no inputs".into(),
        ));
    }
    crate::runtime::validate_policy(&readiness, interface.inputs().keys().map(String::as_str))
        .map_err(NetworkError::InvalidProjection)?;
    let names: Vec<_> = interface.inputs().keys().cloned().collect();
    let ports = (0..bounds.max_live.get()).flat_map(|lane| {
        interface.inputs().values().map(move |port| PortDef {
            name: lane_name(lane, &port.name),
            value: port.value.clone(),
        })
    });
    let expanded = Arc::new(Interface::new(
        ports,
        interface.outputs().values().cloned(),
    )?);
    let mut network = NetworkBuilder::new();
    let join = network.node(
        "join",
        expanded,
        Arc::new(JoinFactory {
            inner,
            policy: LanePolicy {
                names,
                policy: readiness,
                lane: None,
            },
            lanes: bounds.max_live,
        }),
    )?;
    for (index, port) in interface.inputs().values().enumerate() {
        let outputs = (0..bounds.max_live.get()).map(|lane| PortDef {
            name: lane_name(lane, &port.name),
            value: port.value.clone(),
        });
        let router_interface = Arc::new(Interface::new([port.clone()], outputs)?);
        let name = port.name.clone();
        let router = network.node(
            format!("route{index}"),
            router_interface,
            Arc::new(move || Router {
                name: name.clone(),
                emitted: false,
            }),
        )?;
        network.expose_input_dynamic(
            &port.name,
            &router.input_endpoint(&port.name)?,
            bounds.ingress_capacity,
        )?;
        for lane in 0..bounds.max_live.get() {
            let name = lane_name(lane, &port.name);
            network.connect_dynamic(
                &router.output_endpoint(&name)?,
                &join.input_endpoint(&name)?,
                bounds.lane_capacity,
            )?;
        }
    }
    for name in interface.outputs().keys() {
        network.expose_output_dynamic(name, &join.output_endpoint(name)?)?;
    }
    Ok(ComponentDef::composite(network.build()?))
}

fn lane_name(lane: usize, name: &str) -> String {
    format!("lane{lane}:{name}")
}

struct Router {
    name: String,
    emitted: bool,
}
impl Process for Router {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = context.take(&self.name) else {
            return Ok(if context.is_eos(&self.name) {
                Step::Finished
            } else {
                Step::Complete
            });
        };
        let lane = context
            .correlation_lane()
            .ok_or("correlation adapter requires admitted request credit")?;
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            lane_name(lane, &self.name),
            value,
        )))
    }
}

#[derive(Clone)]
struct LanePolicy {
    names: Vec<String>,
    policy: Readiness,
    lane: Option<usize>,
}
impl ReadinessPolicy for LanePolicy {
    fn closure_ports(&self, lane: usize) -> Vec<String> {
        match &self.policy {
            Readiness::Optional { required, optional } if required.is_empty() => {
                optional.iter().map(|name| lane_name(lane, name)).collect()
            }
            Readiness::Custom(policy) => policy
                .closure_ports(lane)
                .iter()
                .map(|name| lane_name(lane, name))
                .collect(),
            _ => vec![],
        }
    }
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        let Some(lane) = self.lane.or(snapshot.correlation_lane) else {
            return vec![];
        };
        let heads: Vec<_> = self
            .names
            .iter()
            .filter_map(|name| {
                snapshot
                    .heads
                    .iter()
                    .find(|h| h.port == lane_name(lane, name))
                    .map(|h| {
                        let mut h = h.clone();
                        h.port = name.clone();
                        h
                    })
            })
            .collect();
        let available: Vec<_> = heads
            .iter()
            .filter(|h| h.available)
            .map(|h| h.port.clone())
            .collect();
        let selected = match &self.policy {
            Readiness::Any => available.into_iter().take(1).collect(),
            Readiness::All | Readiness::CorrelatedAll if available.len() == self.names.len() => {
                available
            }
            Readiness::AtLeast(n) if available.len() >= n.get() => {
                available.into_iter().take(n.get()).collect()
            }
            Readiness::Optional { required, optional }
                if required
                    .iter()
                    .all(|n| heads.iter().any(|h| &h.port == n && h.available))
                    && optional.iter().all(|n| {
                        heads
                            .iter()
                            .any(|h| &h.port == n && (h.available || h.closed))
                    }) =>
            {
                required.iter().chain(optional).cloned().collect()
            }
            Readiness::Custom(policy) => policy.select(&ReadinessSnapshot {
                request: snapshot.request,
                correlation_lane: Some(lane),
                heads,
            }),
            _ => vec![],
        };
        selected.iter().map(|name| lane_name(lane, name)).collect()
    }
}
struct JoinFactory {
    inner: Arc<dyn ProcessFactory>,
    policy: LanePolicy,
    lanes: NonZeroUsize,
}
impl ProcessFactory for JoinFactory {
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(Join {
            inner: self.inner.spawn(),
            names: self.policy.names.clone(),
        })
    }
    fn readiness(&self) -> Readiness {
        Readiness::Custom(Arc::new(self.policy.clone()))
    }
    fn correlation_lanes(&self) -> Option<NonZeroUsize> {
        Some(self.lanes)
    }
}
struct Join {
    inner: Box<dyn Process>,
    names: Vec<String>,
}
impl Process for Join {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        let Some(lane) = context.correlation_lane() else {
            return Ok(Step::Complete);
        };
        let prefix = format!("lane{lane}:");
        context.rename_inputs(|name| name.strip_prefix(&prefix).unwrap_or(name).to_owned());
        let result = self.inner.step(context);
        context.rename_inputs(|name| lane_name(lane, name));
        result.map(|step| match step {
            Step::WaitInput(name) => Step::WaitInput(lane_name(lane, &name)),
            Step::WaitInputs(policy) => Step::WaitInputs(match policy {
                Readiness::All | Readiness::CorrelatedAll => Readiness::Optional {
                    required: self.names.iter().map(|n| lane_name(lane, n)).collect(),
                    optional: vec![],
                },
                Readiness::Optional { required, optional } => Readiness::Optional {
                    required: required.iter().map(|n| lane_name(lane, n)).collect(),
                    optional: optional.iter().map(|n| lane_name(lane, n)).collect(),
                },
                policy => Readiness::Custom(Arc::new(LanePolicy {
                    names: self.names.clone(),
                    policy,
                    lane: Some(lane),
                })),
            }),
            step => step,
        })
    }
}
