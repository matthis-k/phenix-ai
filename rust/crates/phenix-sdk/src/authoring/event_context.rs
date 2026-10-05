use super::{PhenixPluginContext, phenix_context};
use phenix_core::{
    Authority, ComponentId, EventEnvelope, EventTypeId, GenerationId, PluginHost, PluginId,
};
use std::ops::Deref;

/// Scoped runtime context supplied to one generated event listener.
///
/// Event metadata is observational. Execution authority comes from the
/// generation-pinned `PluginHost` used to build the embedded plugin context.
pub struct EventContext<'host, 'runtime> {
    plugin_context: PhenixPluginContext<'host, 'runtime>,
    event_type: EventTypeId,
    emitter: PluginId,
    causality_id: u64,
    kernel_policy_revision: u64,
}

impl<'host, 'runtime> EventContext<'host, 'runtime> {
    #[doc(hidden)]
    #[must_use]
    pub fn from_event(
        host: &'host PluginHost<'runtime>,
        component: ComponentId,
        event: &EventEnvelope,
    ) -> Self {
        Self {
            plugin_context: phenix_context(host, component, (), ()),
            event_type: event.event_type.clone(),
            emitter: event.emitter.clone(),
            causality_id: event.causality_id,
            kernel_policy_revision: event.kernel_policy_revision,
        }
    }

    #[must_use]
    pub fn plugin_context(&self) -> &PhenixPluginContext<'host, 'runtime> {
        &self.plugin_context
    }

    #[must_use]
    pub fn authority(&self) -> &Authority {
        self.plugin_context.call.authority
    }

    #[must_use]
    pub fn graph_generation(&self) -> Option<&GenerationId> {
        self.plugin_context.call.graph_generation
    }

    #[must_use]
    pub fn event_type(&self) -> &EventTypeId {
        &self.event_type
    }

    #[must_use]
    pub fn emitter(&self) -> &PluginId {
        &self.emitter
    }

    #[must_use]
    pub fn causality_id(&self) -> u64 {
        self.causality_id
    }

    #[must_use]
    pub fn kernel_policy_revision(&self) -> u64 {
        self.kernel_policy_revision
    }
}

impl<'host, 'runtime> Deref for EventContext<'host, 'runtime> {
    type Target = PhenixPluginContext<'host, 'runtime>;

    fn deref(&self) -> &Self::Target {
        &self.plugin_context
    }
}
