use crate::{Authority, EventTypeId, GraphGenerationId, PluginId, SubscriptionId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Condvar, Mutex,
    },
    thread,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelEvent {
    PluginActivated(PluginId),
    PluginStopped(PluginId),
    TaskCancelled(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventEnvelope {
    pub event_type: EventTypeId,
    pub version: u32,
    pub emitter: PluginId,
    pub causality_id: u64,
    pub kernel_policy_revision: u64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventFailurePolicy {
    Ignore,
    Warn,
    FailDelivery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionSpec {
    pub id: SubscriptionId,
    pub owner: PluginId,
    pub event_type: EventTypeId,
    pub event_version: u32,
    pub dependencies: Vec<SubscriptionId>,
    pub failure_policy: EventFailurePolicy,
    pub required_authority: Authority,
    pub maximum_authority: Authority,
    pub kernel_policy_revision: u64,
}

pub trait EventHandler: Send + Sync {
    fn handle(&self, event: &EventEnvelope, authority: &Authority) -> Result<(), String>;

    #[doc(hidden)]
    fn handle_with_bus(
        &self,
        _bus: &EventBus,
        event: &EventEnvelope,
        authority: &Authority,
    ) -> Result<(), String> {
        self.handle(event, authority)
    }

    #[doc(hidden)]
    fn handle_with_provenance(
        &self,
        bus: &EventBus,
        event: &EventEnvelope,
        authority: &Authority,
        _graph_generation: Option<&GraphGenerationId>,
    ) -> Result<(), String> {
        self.handle_with_bus(bus, event, authority)
    }
}

impl<F> EventHandler for F
where
    F: Fn(&EventEnvelope, &Authority) -> Result<(), String> + Send + Sync,
{
    fn handle(&self, event: &EventEnvelope, authority: &Authority) -> Result<(), String> {
        self(event, authority)
    }
}

#[derive(Clone)]
pub struct EventSubscription {
    pub spec: SubscriptionSpec,
    pub handler: Arc<dyn EventHandler>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventDispatchReport {
    pub delivered: Vec<SubscriptionId>,
    pub failures: Vec<(SubscriptionId, String)>,
    pub warnings: Vec<(SubscriptionId, String)>,
    pub graph_generation: Option<GraphGenerationId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventDeliveryCancellation {
    SubscriptionSetChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventDeliveryStatus {
    Accepted,
    Succeeded(EventDispatchReport),
    Failed(EventError),
    Cancelled(EventDeliveryCancellation),
}

struct EventReceiptState {
    status: Mutex<EventDeliveryStatus>,
    ready: Condvar,
}

#[derive(Clone)]
pub struct EventAdmissionReceipt {
    id: u64,
    state: Arc<EventReceiptState>,
}

impl fmt::Debug for EventAdmissionReceipt {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventAdmissionReceipt")
            .field("id", &self.id)
            .field("status", &self.status())
            .finish()
    }
}

impl EventAdmissionReceipt {
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn status(&self) -> EventDeliveryStatus {
        self.state
            .status
            .lock()
            .expect("event receipt lock poisoned")
            .clone()
    }

    pub fn wait(&self) -> EventDeliveryStatus {
        let mut status = self
            .state
            .status
            .lock()
            .expect("event receipt lock poisoned");
        while matches!(*status, EventDeliveryStatus::Accepted) {
            status = self
                .state
                .ready
                .wait(status)
                .expect("event receipt lock poisoned while waiting");
        }
        status.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventError {
    DuplicateSubscription(SubscriptionId),
    CrossEventDependency {
        subscription: SubscriptionId,
        dependency: SubscriptionId,
    },
    UnknownDependency {
        subscription: SubscriptionId,
        dependency: SubscriptionId,
    },
    DependencyCycle(SubscriptionId),
    CausalReentry(SubscriptionId),
    AuthorityDenied(SubscriptionId),
    HandlerFailed {
        subscription: SubscriptionId,
        message: String,
    },
    QueueSaturated {
        capacity: usize,
    },
    QueueUnavailable(String),
    DeliveryCancelled(EventDeliveryCancellation),
}

impl Display for EventError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSubscription(id) => write!(f, "duplicate event subscription: {id}"),
            Self::CrossEventDependency {
                subscription,
                dependency,
            } => write!(
                f,
                "event subscription {subscription} has cross-event dependency {dependency}"
            ),
            Self::UnknownDependency {
                subscription,
                dependency,
            } => write!(
                f,
                "event subscription {subscription} depends on unknown subscription {dependency}"
            ),
            Self::DependencyCycle(id) => write!(f, "event subscription cycle includes {id}"),
            Self::CausalReentry(id) => write!(f, "event subscription causal re-entry: {id}"),
            Self::AuthorityDenied(id) => write!(f, "event subscription authority denied: {id}"),
            Self::HandlerFailed {
                subscription,
                message,
            } => write!(f, "event subscription {subscription} failed: {message}"),
            Self::QueueSaturated { capacity } => {
                write!(f, "event delivery queue is full at capacity {capacity}")
            }
            Self::QueueUnavailable(message) => {
                write!(f, "event delivery queue is unavailable: {message}")
            }
            Self::DeliveryCancelled(EventDeliveryCancellation::SubscriptionSetChanged) => {
                f.write_str("event delivery cancelled because subscriptions changed")
            }
        }
    }
}

impl Error for EventError {}

const DEFAULT_DELIVERY_CAPACITY: NonZeroUsize = NonZeroUsize::new(64).unwrap();

struct PendingDelivery {
    event: EventEnvelope,
    emitter_authority: Authority,
    graph_generation: Option<GraphGenerationId>,
    subscriptions: BTreeMap<SubscriptionId, EventSubscription>,
    levels: Vec<Vec<SubscriptionId>>,
    ancestry: BTreeSet<SubscriptionId>,
    revision: u64,
}

#[derive(Clone)]
pub struct EventBus {
    kernel_subscribers: Arc<Mutex<Vec<Sender<KernelEvent>>>>,
    subscriptions: Arc<Mutex<BTreeMap<SubscriptionId, EventSubscription>>>,
    active_causality: Arc<Mutex<BTreeMap<u64, BTreeSet<SubscriptionId>>>>,
    next_root_causality: Arc<AtomicU64>,
    next_delivery: Arc<AtomicU64>,
    subscription_revision: Arc<AtomicU64>,
    in_flight: Arc<AtomicUsize>,
    delivery_capacity: NonZeroUsize,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_DELIVERY_CAPACITY)
    }
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventBus")
            .field("delivery_capacity", &self.delivery_capacity)
            .field("in_flight", &self.in_flight.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl EventBus {
    #[must_use]
    pub fn with_capacity(delivery_capacity: NonZeroUsize) -> Self {
        Self {
            kernel_subscribers: Arc::new(Mutex::new(Vec::new())),
            subscriptions: Arc::new(Mutex::new(BTreeMap::new())),
            active_causality: Arc::new(Mutex::new(BTreeMap::new())),
            next_root_causality: Arc::new(AtomicU64::new(0)),
            next_delivery: Arc::new(AtomicU64::new(0)),
            subscription_revision: Arc::new(AtomicU64::new(0)),
            in_flight: Arc::new(AtomicUsize::new(0)),
            delivery_capacity,
        }
    }

    pub fn validate_subscriptions(
        subscriptions: impl IntoIterator<Item = EventSubscription>,
    ) -> Result<(), EventError> {
        index_subscriptions(subscriptions)
            .and_then(|subscriptions| validate_dependencies(&subscriptions))
    }

    pub fn subscribe(&self) -> Receiver<KernelEvent> {
        let (sender, receiver) = mpsc::channel();
        self.kernel_subscribers
            .lock()
            .expect("event subscriber lock poisoned")
            .push(sender);
        receiver
    }

    pub fn publish(&self, event: KernelEvent) {
        self.kernel_subscribers
            .lock()
            .expect("event subscriber lock poisoned")
            .retain(|sender| sender.send(event.clone()).is_ok());
    }

    pub fn replace_subscriptions(
        &self,
        subscriptions: impl IntoIterator<Item = EventSubscription>,
    ) -> Result<(), EventError> {
        let indexed = index_subscriptions(subscriptions)?;
        validate_dependencies(&indexed)?;
        let mut current = self
            .subscriptions
            .lock()
            .expect("event subscription lock poisoned");
        *current = indexed;
        self.subscription_revision.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn install_subscriptions(
        &self,
        subscriptions: impl IntoIterator<Item = EventSubscription>,
    ) -> Result<Vec<SubscriptionId>, EventError> {
        let mut current = self
            .subscriptions
            .lock()
            .expect("event subscription lock poisoned");
        let mut candidate = current.clone();
        let mut installed = Vec::new();
        for subscription in subscriptions {
            let id = subscription.spec.id.clone();
            if candidate.insert(id.clone(), subscription).is_some() {
                return Err(EventError::DuplicateSubscription(id));
            }
            installed.push(id);
        }
        validate_dependencies(&candidate)?;
        *current = candidate;
        self.subscription_revision.fetch_add(1, Ordering::AcqRel);
        Ok(installed)
    }

    pub fn remove_subscriptions(
        &self,
        subscriptions: impl IntoIterator<Item = SubscriptionId>,
    ) -> Result<(), EventError> {
        let mut current = self
            .subscriptions
            .lock()
            .expect("event subscription lock poisoned");
        let mut candidate = current.clone();
        for subscription in subscriptions {
            candidate.remove(&subscription);
        }
        validate_dependencies(&candidate)?;
        *current = candidate;
        self.subscription_revision.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn dispatch(
        &self,
        event: &EventEnvelope,
        emitter_authority: &Authority,
    ) -> Result<EventDispatchReport, EventError> {
        self.dispatch_in_generation(event, emitter_authority, None)
    }

    pub fn dispatch_in_generation(
        &self,
        event: &EventEnvelope,
        emitter_authority: &Authority,
        graph_generation: Option<&GraphGenerationId>,
    ) -> Result<EventDispatchReport, EventError> {
        match self
            .admit_in_generation(event, emitter_authority, graph_generation)?
            .wait()
        {
            EventDeliveryStatus::Succeeded(report) => Ok(report),
            EventDeliveryStatus::Failed(error) => Err(error),
            EventDeliveryStatus::Cancelled(reason) => Err(EventError::DeliveryCancelled(reason)),
            EventDeliveryStatus::Accepted => unreachable!("wait returns a terminal status"),
        }
    }

    pub fn admit(
        &self,
        event: &EventEnvelope,
        emitter_authority: &Authority,
    ) -> Result<EventAdmissionReceipt, EventError> {
        self.admit_in_generation(event, emitter_authority, None)
    }

    pub fn admit_in_generation(
        &self,
        event: &EventEnvelope,
        emitter_authority: &Authority,
        graph_generation: Option<&GraphGenerationId>,
    ) -> Result<EventAdmissionReceipt, EventError> {
        let event = if event.causality_id == 0 {
            EventEnvelope {
                causality_id: self.next_root_causality_id(),
                ..event.clone()
            }
        } else {
            event.clone()
        };
        let (subscriptions, revision) = {
            let current = self
                .subscriptions
                .lock()
                .expect("event subscription lock poisoned");
            (
                current.clone(),
                self.subscription_revision.load(Ordering::Acquire),
            )
        };
        let levels = dependency_levels(&subscriptions, &event.event_type, event.version)?;
        for level in &levels {
            for id in level {
                let subscription = &subscriptions[id];
                if !emitter_authority.permits_all(&subscription.spec.required_authority) {
                    return Err(EventError::AuthorityDenied(id.clone()));
                }
            }
        }
        self.reserve_delivery()?;
        let ancestry = self
            .active_causality
            .lock()
            .expect("event causality lock poisoned")
            .get(&event.causality_id)
            .cloned()
            .unwrap_or_default();
        let id = self.next_delivery_id();
        let state = Arc::new(EventReceiptState {
            status: Mutex::new(EventDeliveryStatus::Accepted),
            ready: Condvar::new(),
        });
        let receipt = EventAdmissionReceipt {
            id,
            state: Arc::clone(&state),
        };
        let bus = self.clone();
        let authority = emitter_authority.clone();
        let delivery = PendingDelivery {
            event,
            emitter_authority: authority,
            graph_generation: graph_generation.cloned(),
            subscriptions,
            levels,
            ancestry,
            revision,
        };
        let spawn = thread::Builder::new()
            .name(format!("phenix-event-{id}"))
            .spawn(move || {
                let status = bus.execute_delivery(delivery);
                *state.status.lock().expect("event receipt lock poisoned") = status;
                state.ready.notify_all();
                bus.in_flight.fetch_sub(1, Ordering::AcqRel);
            });
        if let Err(error) = spawn {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            return Err(EventError::QueueUnavailable(error.to_string()));
        }
        Ok(receipt)
    }

    fn execute_delivery(&self, delivery: PendingDelivery) -> EventDeliveryStatus {
        let PendingDelivery {
            event,
            emitter_authority,
            graph_generation,
            subscriptions,
            levels,
            ancestry,
            revision,
        } = delivery;
        let mut report = EventDispatchReport {
            graph_generation: graph_generation.clone(),
            ..EventDispatchReport::default()
        };
        for level in levels {
            if self.subscription_revision.load(Ordering::Acquire) != revision {
                return EventDeliveryStatus::Cancelled(
                    EventDeliveryCancellation::SubscriptionSetChanged,
                );
            }
            if let Some(id) = level.iter().find(|id| ancestry.contains(*id)) {
                return EventDeliveryStatus::Failed(EventError::CausalReentry(id.clone()));
            }
            if let Err(error) = self.enter_causality(event.causality_id, &level) {
                return EventDeliveryStatus::Failed(error);
            }
            let event = &event;
            let graph_generation = graph_generation.as_ref();
            let bus = self;
            let results = thread::scope(|scope| {
                let handles = level
                    .iter()
                    .map(|id| {
                        let subscription = &subscriptions[id];
                        let handler_authority =
                            emitter_authority.attenuate(&subscription.spec.maximum_authority);
                        scope.spawn(move || {
                            subscription.handler.handle_with_provenance(
                                bus,
                                event,
                                &handler_authority,
                                graph_generation,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| match handle.join() {
                        Ok(result) => result,
                        Err(_) => Err("event listener panicked during delivery".into()),
                    })
                    .collect::<Vec<_>>()
            });
            self.leave_causality(event.causality_id, &level);
            if self.subscription_revision.load(Ordering::Acquire) != revision {
                return EventDeliveryStatus::Cancelled(
                    EventDeliveryCancellation::SubscriptionSetChanged,
                );
            }

            for (id, result) in level.into_iter().zip(results) {
                let subscription = &subscriptions[&id];
                match result {
                    Ok(()) => report.delivered.push(id),
                    Err(message) => {
                        report.failures.push((id.clone(), message.clone()));
                        match subscription.spec.failure_policy {
                            EventFailurePolicy::Ignore => {}
                            EventFailurePolicy::Warn => report.warnings.push((id, message)),
                            EventFailurePolicy::FailDelivery => {
                                return EventDeliveryStatus::Failed(EventError::HandlerFailed {
                                    subscription: id,
                                    message,
                                });
                            }
                        }
                    }
                }
            }
        }
        EventDeliveryStatus::Succeeded(report)
    }

    fn reserve_delivery(&self) -> Result<(), EventError> {
        let mut current = self.in_flight.load(Ordering::Acquire);
        loop {
            if current >= self.delivery_capacity.get() {
                return Err(EventError::QueueSaturated {
                    capacity: self.delivery_capacity.get(),
                });
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => current = actual,
            }
        }
    }

    fn next_root_causality_id(&self) -> u64 {
        next_nonzero_id(&self.next_root_causality)
    }

    fn next_delivery_id(&self) -> u64 {
        next_nonzero_id(&self.next_delivery)
    }

    fn enter_causality(
        &self,
        causality_id: u64,
        subscriptions: &[SubscriptionId],
    ) -> Result<(), EventError> {
        let mut active = self
            .active_causality
            .lock()
            .expect("event causality lock poisoned");
        let active_for_cause = active.entry(causality_id).or_default();
        let mut inserted = Vec::new();
        for id in subscriptions {
            if !active_for_cause.insert(id.clone()) {
                for inserted_id in inserted {
                    active_for_cause.remove(&inserted_id);
                }
                if active_for_cause.is_empty() {
                    active.remove(&causality_id);
                }
                return Err(EventError::CausalReentry(id.clone()));
            }
            inserted.push(id.clone());
        }
        Ok(())
    }

    fn leave_causality(&self, causality_id: u64, subscriptions: &[SubscriptionId]) {
        let mut active = self
            .active_causality
            .lock()
            .expect("event causality lock poisoned");
        let Some(active_for_cause) = active.get_mut(&causality_id) else {
            return;
        };
        for id in subscriptions {
            active_for_cause.remove(id);
        }
        if active_for_cause.is_empty() {
            active.remove(&causality_id);
        }
    }
}

fn next_nonzero_id(counter: &AtomicU64) -> u64 {
    loop {
        let id = counter.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        if id != 0 {
            return id;
        }
    }
}

fn index_subscriptions(
    subscriptions: impl IntoIterator<Item = EventSubscription>,
) -> Result<BTreeMap<SubscriptionId, EventSubscription>, EventError> {
    let mut indexed = BTreeMap::new();
    for subscription in subscriptions {
        let id = subscription.spec.id.clone();
        if indexed.insert(id.clone(), subscription).is_some() {
            return Err(EventError::DuplicateSubscription(id));
        }
    }
    Ok(indexed)
}

fn validate_dependencies(
    subscriptions: &BTreeMap<SubscriptionId, EventSubscription>,
) -> Result<(), EventError> {
    for subscription in subscriptions.values() {
        for dependency in &subscription.spec.dependencies {
            let Some(target) = subscriptions.get(dependency) else {
                return Err(EventError::UnknownDependency {
                    subscription: subscription.spec.id.clone(),
                    dependency: dependency.clone(),
                });
            };
            if target.spec.event_type != subscription.spec.event_type
                || target.spec.event_version != subscription.spec.event_version
            {
                return Err(EventError::CrossEventDependency {
                    subscription: subscription.spec.id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    for event in subscriptions
        .values()
        .map(|subscription| {
            (
                subscription.spec.event_type.clone(),
                subscription.spec.event_version,
            )
        })
        .collect::<BTreeSet<_>>()
    {
        dependency_order(subscriptions, &event.0, event.1)?;
    }
    Ok(())
}

fn dependency_levels(
    subscriptions: &BTreeMap<SubscriptionId, EventSubscription>,
    event_type: &EventTypeId,
    event_version: u32,
) -> Result<Vec<Vec<SubscriptionId>>, EventError> {
    let mut remaining = subscriptions
        .values()
        .filter(|subscription| {
            &subscription.spec.event_type == event_type
                && subscription.spec.event_version == event_version
        })
        .map(|subscription| subscription.spec.id.clone())
        .collect::<BTreeSet<_>>();
    let mut levels = Vec::new();

    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|id| {
                subscriptions[*id]
                    .spec
                    .dependencies
                    .iter()
                    .all(|dependency| !remaining.contains(dependency))
            })
            .cloned()
            .collect::<Vec<_>>();
        let Some(first) = ready.first() else {
            return Err(EventError::DependencyCycle(
                remaining
                    .iter()
                    .next()
                    .expect("remaining is not empty")
                    .clone(),
            ));
        };
        debug_assert!(remaining.contains(first));
        for id in &ready {
            remaining.remove(id);
        }
        levels.push(ready);
    }

    Ok(levels)
}

fn dependency_order(
    subscriptions: &BTreeMap<SubscriptionId, EventSubscription>,
    event_type: &EventTypeId,
    event_version: u32,
) -> Result<Vec<SubscriptionId>, EventError> {
    Ok(dependency_levels(subscriptions, event_type, event_version)?
        .into_iter()
        .flatten()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityId, ResolvedHarness};
    use std::{
        sync::{Condvar, Mutex},
        time::Duration,
    };

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn event_type(value: &str) -> EventTypeId {
        EventTypeId::parse(value).unwrap()
    }

    fn subscription(value: &str) -> SubscriptionId {
        SubscriptionId::parse(value).unwrap()
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn envelope(causality_id: u64) -> EventEnvelope {
        EventEnvelope {
            event_type: event_type("demo.changed"),
            version: 1,
            emitter: plugin("emitter"),
            causality_id,
            kernel_policy_revision: 7,
            payload: b"payload".to_vec(),
        }
    }

    fn spec(id: &str, dependencies: &[&str]) -> SubscriptionSpec {
        SubscriptionSpec {
            id: subscription(id),
            owner: plugin(id),
            event_type: event_type("demo.changed"),
            event_version: 1,
            dependencies: dependencies.iter().map(|id| subscription(id)).collect(),
            failure_policy: EventFailurePolicy::FailDelivery,
            required_authority: Authority::default(),
            maximum_authority: Authority::default(),
            kernel_policy_revision: 7,
        }
    }

    fn subscription_with(id: &str, dependencies: &[&str]) -> EventSubscription {
        EventSubscription {
            spec: spec(id, dependencies),
            handler: Arc::new(|_: &EventEnvelope, _: &Authority| Ok(())),
        }
    }

    #[test]
    fn kernel_subscribers_receive_generic_kernel_events() {
        let bus = EventBus::default();
        let receiver = bus.subscribe();
        let event = KernelEvent::TaskCancelled(7);
        bus.publish(event.clone());
        assert_eq!(receiver.recv().unwrap(), event);
    }

    #[test]
    fn fail_delivery_policy_uses_delivery_wire_name() {
        assert_eq!(
            serde_json::to_value(EventFailurePolicy::FailDelivery).unwrap(),
            serde_json::Value::String("fail_delivery".into())
        );
    }

    #[test]
    fn root_causality_is_assigned_before_listener_delivery() {
        let bus = EventBus::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_handler = Arc::clone(&seen);
        bus.replace_subscriptions([EventSubscription {
            spec: spec("listener", &[]),
            handler: Arc::new(move |event: &EventEnvelope, _: &Authority| {
                seen_by_handler.lock().unwrap().push(event.causality_id);
                Ok(())
            }),
        }])
        .unwrap();

        bus.dispatch(&envelope(0), &Authority::default()).unwrap();
        bus.dispatch(&envelope(0), &Authority::default()).unwrap();
        let seen = seen.lock().unwrap();
        assert_ne!(seen[0], 0);
        assert_ne!(seen[1], 0);
        assert_ne!(seen[0], seen[1]);
    }

    #[test]
    fn dispatch_report_records_graph_generation() {
        let generation = ResolvedHarness::resolve([], [], [], &Authority::default())
            .unwrap()
            .generation()
            .clone();
        let bus = EventBus::default();
        let report = bus
            .dispatch_in_generation(&envelope(1), &Authority::default(), Some(&generation))
            .unwrap();
        assert_eq!(report.graph_generation.as_ref(), Some(&generation));
    }

    #[test]
    fn all_subscribers_receive_event_in_dependency_order() {
        let bus = EventBus::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_a = Arc::clone(&seen);
        let seen_b = Arc::clone(&seen);
        bus.replace_subscriptions([
            EventSubscription {
                spec: spec("second", &["first"]),
                handler: Arc::new(move |_: &EventEnvelope, _: &Authority| {
                    seen_b.lock().unwrap().push("second");
                    Ok(())
                }),
            },
            EventSubscription {
                spec: spec("first", &[]),
                handler: Arc::new(move |_: &EventEnvelope, _: &Authority| {
                    seen_a.lock().unwrap().push("first");
                    Ok(())
                }),
            },
        ])
        .unwrap();

        let report = bus.dispatch(&envelope(1), &Authority::default()).unwrap();
        assert_eq!(&*seen.lock().unwrap(), &["first", "second"]);
        assert_eq!(
            report.delivered,
            vec![subscription("first"), subscription("second")]
        );
    }

    #[test]
    fn independent_subscribers_run_concurrently() {
        #[derive(Default)]
        struct GateState {
            started: usize,
            release: bool,
        }

        let bus = Arc::new(EventBus::default());
        let gate = Arc::new((Mutex::new(GateState::default()), Condvar::new()));
        let make_handler = || {
            let gate = Arc::clone(&gate);
            Arc::new(move |_: &EventEnvelope, _: &Authority| {
                let (state, changed) = &*gate;
                let mut state = state.lock().unwrap();
                state.started += 1;
                changed.notify_all();
                while !state.release {
                    state = changed.wait(state).unwrap();
                }
                Ok(())
            }) as Arc<dyn EventHandler>
        };
        bus.replace_subscriptions([
            EventSubscription {
                spec: spec("first", &[]),
                handler: make_handler(),
            },
            EventSubscription {
                spec: spec("second", &[]),
                handler: make_handler(),
            },
        ])
        .unwrap();

        let dispatch_bus = Arc::clone(&bus);
        let dispatch =
            thread::spawn(move || dispatch_bus.dispatch(&envelope(3), &Authority::default()));
        let (state, changed) = &*gate;
        let state = state.lock().unwrap();
        let (mut state, timeout) = changed
            .wait_timeout_while(state, Duration::from_secs(1), |state| state.started < 2)
            .unwrap();
        let both_started = !timeout.timed_out() && state.started == 2;
        state.release = true;
        changed.notify_all();
        drop(state);

        let report = dispatch.join().unwrap().unwrap();
        assert!(both_started, "independent listeners did not overlap");
        assert_eq!(
            report.delivered,
            vec![subscription("first"), subscription("second")]
        );
    }

    #[test]
    fn dependency_cycles_are_rejected_atomically() {
        let bus = EventBus::default();
        let noop: Arc<dyn EventHandler> = Arc::new(|_: &EventEnvelope, _: &Authority| Ok(()));
        let error = bus
            .replace_subscriptions([
                EventSubscription {
                    spec: spec("a", &["b"]),
                    handler: Arc::clone(&noop),
                },
                EventSubscription {
                    spec: spec("b", &["a"]),
                    handler: noop,
                },
            ])
            .unwrap_err();
        assert!(matches!(error, EventError::DependencyCycle(_)));
    }

    #[test]
    fn installing_subscriptions_preserves_existing_entries_and_is_atomic() {
        let bus = EventBus::default();
        bus.replace_subscriptions([subscription_with("first", &[])])
            .unwrap();
        let installed = bus
            .install_subscriptions([subscription_with("second", &["first"])])
            .unwrap();
        assert_eq!(installed, vec![subscription("second")]);

        let duplicate = bus
            .install_subscriptions([
                subscription_with("third", &[]),
                subscription_with("second", &[]),
            ])
            .unwrap_err();
        assert_eq!(
            duplicate,
            EventError::DuplicateSubscription(subscription("second"))
        );

        let report = bus.dispatch(&envelope(11), &Authority::default()).unwrap();
        assert_eq!(
            report.delivered,
            vec![subscription("first"), subscription("second")]
        );
    }

    #[test]
    fn removing_subscriptions_is_atomic_when_dependencies_would_break() {
        let bus = EventBus::default();
        bus.replace_subscriptions([
            subscription_with("first", &[]),
            subscription_with("second", &["first"]),
        ])
        .unwrap();

        let error = bus
            .remove_subscriptions([subscription("first")])
            .unwrap_err();
        assert_eq!(
            error,
            EventError::UnknownDependency {
                subscription: subscription("second"),
                dependency: subscription("first"),
            }
        );

        let report = bus.dispatch(&envelope(12), &Authority::default()).unwrap();
        assert_eq!(
            report.delivered,
            vec![subscription("first"), subscription("second")]
        );
    }

    #[test]
    fn event_handler_authority_is_attenuated() {
        let bus = EventBus::default();
        let read = capability("fs.read");
        let write = capability("fs.write");
        let mut handler_spec = spec("handler", &[]);
        handler_spec.required_authority = Authority::new([read.clone()]);
        handler_spec.maximum_authority = Authority::new([read.clone()]);
        bus.replace_subscriptions([EventSubscription {
            spec: handler_spec,
            handler: Arc::new(move |_: &EventEnvelope, authority: &Authority| {
                assert!(authority.permits(&read));
                assert!(!authority.permits(&write));
                Ok(())
            }),
        }])
        .unwrap();

        bus.dispatch(
            &envelope(2),
            &Authority::new([capability("fs.read"), capability("fs.write")]),
        )
        .unwrap();
    }

    #[test]
    fn same_subscription_causal_reentry_is_blocked() {
        let bus = Arc::new(EventBus::default());
        let nested_bus = Arc::clone(&bus);
        bus.replace_subscriptions([EventSubscription {
            spec: spec("recursive", &[]),
            handler: Arc::new(move |event: &EventEnvelope, authority: &Authority| {
                assert_eq!(
                    nested_bus.dispatch(event, authority),
                    Err(EventError::CausalReentry(subscription("recursive")))
                );
                Ok(())
            }),
        }])
        .unwrap();

        bus.dispatch(&envelope(0), &Authority::default()).unwrap();
    }

    #[test]
    fn warn_policy_records_failure_without_veto() {
        let bus = EventBus::default();
        let mut warning = spec("warning", &[]);
        warning.failure_policy = EventFailurePolicy::Warn;
        bus.replace_subscriptions([EventSubscription {
            spec: warning,
            handler: Arc::new(|_: &EventEnvelope, _: &Authority| Err("expected".into())),
        }])
        .unwrap();

        let report = bus.dispatch(&envelope(10), &Authority::default()).unwrap();
        assert!(report.delivered.is_empty());
        assert_eq!(
            report.failures,
            vec![(subscription("warning"), "expected".into())]
        );
        assert_eq!(
            report.warnings,
            vec![(subscription("warning"), "expected".into())]
        );
    }

    #[test]
    fn admission_is_bounded_and_reports_a_terminal_status() {
        let bus = EventBus::with_capacity(NonZeroUsize::new(1).unwrap());
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let handler_gate = Arc::clone(&gate);
        bus.replace_subscriptions([EventSubscription {
            spec: spec("blocked", &[]),
            handler: Arc::new(move |_: &EventEnvelope, _: &Authority| {
                let (state, changed) = &*handler_gate;
                let mut state = state.lock().unwrap();
                state.0 = true;
                changed.notify_all();
                while !state.1 {
                    state = changed.wait(state).unwrap();
                }
                Ok(())
            }),
        }])
        .unwrap();

        let receipt = bus.admit(&envelope(20), &Authority::default()).unwrap();
        let (state, changed) = &*gate;
        let state = state.lock().unwrap();
        let (mut state, timeout) = changed
            .wait_timeout_while(state, Duration::from_secs(1), |state| !state.0)
            .unwrap();
        assert!(!timeout.timed_out(), "listener did not start");
        assert_eq!(receipt.status(), EventDeliveryStatus::Accepted);
        assert!(matches!(
            bus.admit(&envelope(21), &Authority::default()),
            Err(EventError::QueueSaturated { capacity: 1 })
        ));
        state.1 = true;
        changed.notify_all();
        drop(state);

        assert!(matches!(
            receipt.wait(),
            EventDeliveryStatus::Succeeded(EventDispatchReport { .. })
        ));
    }

    #[test]
    fn fail_delivery_receipt_preserves_the_handler_failure() {
        let bus = EventBus::default();
        bus.replace_subscriptions([EventSubscription {
            spec: spec("failed", &[]),
            handler: Arc::new(|_: &EventEnvelope, _: &Authority| Err("failed".into())),
        }])
        .unwrap();

        let receipt = bus.admit(&envelope(22), &Authority::default()).unwrap();
        assert_eq!(
            receipt.wait(),
            EventDeliveryStatus::Failed(EventError::HandlerFailed {
                subscription: subscription("failed"),
                message: "failed".into(),
            })
        );
    }

    #[test]
    fn subscription_replacement_cancels_an_in_flight_old_delivery() {
        let bus = EventBus::default();
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let handler_gate = Arc::clone(&gate);
        bus.replace_subscriptions([
            EventSubscription {
                spec: spec("first", &[]),
                handler: Arc::new(move |_: &EventEnvelope, _: &Authority| {
                    let (state, changed) = &*handler_gate;
                    let mut state = state.lock().unwrap();
                    state.0 = true;
                    changed.notify_all();
                    while !state.1 {
                        state = changed.wait(state).unwrap();
                    }
                    Ok(())
                }),
            },
            subscription_with("second", &["first"]),
        ])
        .unwrap();

        let receipt = bus.admit(&envelope(23), &Authority::default()).unwrap();
        let (state, changed) = &*gate;
        let state = state.lock().unwrap();
        let (mut state, timeout) = changed
            .wait_timeout_while(state, Duration::from_secs(1), |state| !state.0)
            .unwrap();
        assert!(!timeout.timed_out(), "listener did not start");
        bus.replace_subscriptions([subscription_with("replacement", &[])])
            .unwrap();
        state.1 = true;
        changed.notify_all();
        drop(state);

        assert_eq!(
            receipt.wait(),
            EventDeliveryStatus::Cancelled(EventDeliveryCancellation::SubscriptionSetChanged)
        );
    }
}
