use super::ServiceInvocationProvenance;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    panic::{catch_unwind, AssertUnwindSafe},
};

pub const DEFAULT_RUNTIME_TRACE_CAPACITY: usize = 256;
pub const DEFAULT_PROVENANCE_CAPACITY: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeTraceParticipant {
    pub plugin: String,
    pub role: String,
    pub outcome: String,
}

/// Metadata-only runtime diagnostics. Request/response payloads and secret values must never be
/// added to these records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RuntimeTraceEvent {
    ServiceInvocation {
        service: String,
        input_bytes: usize,
        output_bytes: Option<usize>,
        success: bool,
        error: Option<String>,
        terminal_reached: bool,
        participants: Vec<RuntimeTraceParticipant>,
    },
    PolicyStage {
        policy: String,
        stage: String,
        outcome: String,
        subject: Option<String>,
        revision: Option<String>,
        reason: Option<String>,
    },
    DataMutation {
        resource: String,
        stage: String,
        operation_count: usize,
        outcome: String,
        error: Option<String>,
    },
}

/// Infallible destination for kernel runtime diagnostics.
///
/// Core isolates sink panics at the call site so diagnostics cannot change execution outcomes.
pub trait RuntimeTraceSink: Send + Sync {
    fn record(&self, event: RuntimeTraceEvent);
}

pub struct RuntimeTraceBuffer {
    capacity: NonZeroUsize,
    events: Mutex<VecDeque<RuntimeTraceEvent>>,
}

impl RuntimeTraceBuffer {
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            events: Mutex::new(VecDeque::with_capacity(capacity.get())),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<RuntimeTraceEvent> {
        self.events.lock().iter().cloned().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.lock().is_empty()
    }
}

impl Default for RuntimeTraceBuffer {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(DEFAULT_RUNTIME_TRACE_CAPACITY)
                .expect("default runtime trace capacity is non-zero"),
        )
    }
}

impl RuntimeTraceSink for RuntimeTraceBuffer {
    fn record(&self, event: RuntimeTraceEvent) {
        let mut events = self.events.lock();
        if events.len() == self.capacity.get() {
            events.pop_front();
        }
        events.push_back(event);
    }
}

pub struct ProvenanceBuffer {
    capacity: NonZeroUsize,
    entries: Mutex<VecDeque<ServiceInvocationProvenance>>,
}

impl ProvenanceBuffer {
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            entries: Mutex::new(VecDeque::with_capacity(capacity.get())),
        }
    }

    pub(crate) fn record(&self, provenance: ServiceInvocationProvenance) {
        let mut entries = self.entries.lock();
        if entries.len() == self.capacity.get() {
            entries.pop_front();
        }
        entries.push_back(provenance);
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<ServiceInvocationProvenance> {
        self.entries.lock().iter().cloned().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }
}

impl Default for ProvenanceBuffer {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(DEFAULT_PROVENANCE_CAPACITY)
                .expect("default provenance capacity is non-zero"),
        )
    }
}

pub(super) fn record_runtime_trace(sink: &dyn RuntimeTraceSink, event: RuntimeTraceEvent) {
    let _ = catch_unwind(AssertUnwindSafe(|| sink.record(event)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Authority, KernelConfig, PluginId, ProviderBinding, ResolvedServiceChain, ServiceId,
    };

    fn policy_trace(stage: &str) -> RuntimeTraceEvent {
        RuntimeTraceEvent::PolicyStage {
            policy: "fixture".to_owned(),
            stage: stage.to_owned(),
            outcome: "allowed".to_owned(),
            subject: None,
            revision: None,
            reason: None,
        }
    }

    fn provenance(service: &str) -> ServiceInvocationProvenance {
        let service = ServiceId::parse(service).unwrap();
        let policy_identity = KernelConfig::empty().policy_identity();
        let terminal = ProviderBinding {
            service: service.clone(),
            plugin: PluginId::parse("fixture.provider").unwrap(),
            priority: 0,
        };
        ServiceInvocationProvenance {
            graph_generation: None,
            policy_identity,
            service: service.clone(),
            planned_chain: ResolvedServiceChain {
                policy_identity,
                service,
                layers: Vec::new(),
                terminal,
            },
            component_provider: None,
            caller_authority: Authority::default(),
            participants: Vec::new(),
            terminal_reached: false,
        }
    }

    #[test]
    fn trace_buffer_evicts_oldest_record() {
        let buffer = RuntimeTraceBuffer::new(NonZeroUsize::new(2).unwrap());
        buffer.record(policy_trace("one"));
        buffer.record(policy_trace("two"));
        buffer.record(policy_trace("three"));

        let events = buffer.snapshot();
        assert_eq!(events, vec![policy_trace("two"), policy_trace("three")]);
    }

    #[test]
    fn provenance_buffer_evicts_oldest_record() {
        let buffer = ProvenanceBuffer::new(NonZeroUsize::new(2).unwrap());
        buffer.record(provenance("fixture.one@1"));
        buffer.record(provenance("fixture.two@1"));
        buffer.record(provenance("fixture.three@1"));

        let services = buffer
            .snapshot()
            .into_iter()
            .map(|entry| entry.service.as_str().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(services, ["fixture.two@1", "fixture.three@1"]);
    }

    #[test]
    fn trace_sink_panic_is_isolated() {
        struct PanicSink;

        impl RuntimeTraceSink for PanicSink {
            fn record(&self, _event: RuntimeTraceEvent) {
                panic!("diagnostic sink failed");
            }
        }

        record_runtime_trace(&PanicSink, policy_trace("panic"));
    }
}
