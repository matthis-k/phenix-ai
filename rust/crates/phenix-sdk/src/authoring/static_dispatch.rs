use super::{EventContext, StaticPluginComponents, StaticPluginResources};
use phenix_core::{
    Authority, ComponentId, EventEnvelope, EventTypeId, Exact, GenerationId, InterfaceId,
    LayerResult, PhenixValue, PluginContext, PluginHost, PluginId, PluginInstance, PluginListener,
    Project, ResolvedListener, ServiceId, SharedPluginInvocation, ValueError,
};
use std::{
    error::Error,
    future::Future,
    sync::{Arc, Weak},
    task::{Context, Poll, Wake, Waker},
    thread,
};

const STRUCTURAL_MISMATCH_EVENT: &str = "kernel.structural_value_mismatch";
const STRUCTURAL_MISMATCH_EVENT_VERSION: u32 = 1;

/// Erased runtime dispatch generated from a component's typed exports.
pub trait StaticComponentDispatch {
    fn dispatch(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String>;

    fn into_plugin_instance(self) -> Box<dyn PluginInstance>
    where
        Self: Sized + StaticComponentRuntimeDispatch + Send + Sync + 'static,
    {
        Box::new(StaticDispatchInstance(Arc::new(self)))
    }

    fn default_plugin_instance() -> Box<dyn PluginInstance>
    where
        Self: Sized + StaticComponentRuntimeDispatch + Default + Send + Sync + 'static,
    {
        Self::default().into_plugin_instance()
    }
}

/// Live runtime callbacks generated from a component impl.
pub trait StaticComponentRuntimeDispatch {
    fn dispatch_runtime(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String>;

    fn dispatch_layer_runtime(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Option<Result<LayerResult, String>>;

    #[doc(hidden)]
    fn dispatch_listener_runtime(
        &self,
        _listener: &str,
        _context: &EventContext<'_, '_>,
        _payload: &[u8],
    ) -> Option<Result<(), Box<dyn Error + Send + Sync>>> {
        None
    }

    #[doc(hidden)]
    fn decode_projected_listener_runtime<T>(
        payload: &[u8],
    ) -> Result<T, Box<dyn Error + Send + Sync>>
    where
        Self: Sized,
        for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    {
        decode_listener_runtime(payload, |value| T::try_from(Project(value)))
    }

    #[doc(hidden)]
    fn decode_exact_listener_runtime<T>(payload: &[u8]) -> Result<T, Box<dyn Error + Send + Sync>>
    where
        Self: Sized,
        for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        decode_listener_runtime(payload, |value| T::try_from(Exact(value)))
    }
}

struct StaticDispatchInstance<T>(Arc<T>);

struct StaticComponentSharedInvocation<T>(Weak<T>);

impl<T> StaticComponentSharedInvocation<T> {
    fn component(&self) -> Result<Arc<T>, String> {
        self.0
            .upgrade()
            .ok_or_else(|| "static component state is unavailable".to_owned())
    }
}

impl<T> SharedPluginInvocation for StaticComponentSharedInvocation<T>
where
    T: StaticComponentDispatch + StaticComponentRuntimeDispatch + Send + Sync + 'static,
{
    fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        self.component()?.dispatch_runtime(service, input, host)
    }

    fn invoke_layer(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.component()?
            .dispatch_layer_runtime(service, input, host)
            .unwrap_or_else(|| Err(format!("unsupported component layer: {service}")))
    }
}

impl<T> PluginInstance for StaticDispatchInstance<T>
where
    T: StaticComponentDispatch + StaticComponentRuntimeDispatch + Send + Sync + 'static,
{
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(StaticComponentSharedInvocation(Arc::downgrade(
            &self.0,
        ))))
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        self.0.dispatch_runtime(service, input, host)
    }

    fn invoke_layer(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.0
            .dispatch_layer_runtime(service, input, host)
            .unwrap_or_else(|| Err(format!("unsupported component layer: {service}")))
    }
}

/// Kernel-scoped context for one layer invocation.
pub struct LayerContext<'host, 'runtime> {
    host: &'host PluginHost<'runtime>,
}

impl<'host, 'runtime> LayerContext<'host, 'runtime> {
    #[doc(hidden)]
    #[must_use]
    pub fn from_host(host: &'host PluginHost<'runtime>) -> Self {
        Self { host }
    }

    #[must_use]
    pub fn authority(&self) -> &Authority {
        self.host.authority()
    }

    #[must_use]
    pub fn graph_generation(&self) -> Option<&phenix_core::GenerationId> {
        self.host.graph_generation()
    }

    pub fn delegate<T>(&self, request: &T) -> Result<LayerResult, String>
    where
        for<'value> PhenixValue: From<&'value T>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(request))
            .map_err(|error| format!("layer request encoding failed: {error}"))?;
        self.continue_input(&input)
    }

    pub fn handle<T>(&self, response: &T) -> Result<LayerResult, String>
    where
        for<'value> PhenixValue: From<&'value T>,
    {
        let output = serde_json::to_vec(&PhenixValue::from(response))
            .map_err(|error| format!("layer response encoding failed: {error}"))?;
        Ok(LayerResult::Handled(output))
    }

    #[must_use]
    pub fn deny(message: impl Into<String>) -> LayerResult {
        LayerResult::Denied(message.into())
    }

    #[doc(hidden)]
    pub fn continue_input(&self, input: &[u8]) -> Result<LayerResult, String> {
        self.host
            .continue_service(input, self.host.authority())
            .map(LayerResult::Handled)
            .map_err(|error| error.to_string())
    }
}

#[doc(hidden)]
pub fn block_on_static<F: Future>(future: F) -> F::Output {
    struct ThreadWake(thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

#[doc(hidden)]
pub fn decode_projected_runtime<T>(
    host: &PluginHost<'_>,
    interface: &InterfaceId,
    input: &[u8],
) -> Result<T, String>
where
    for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    PluginContext::new(host, (), (), ())
        .kernel
        .decode_projected(interface, input)
        .map_err(|error| error.to_string())
}

#[doc(hidden)]
pub fn decode_exact_runtime<T>(
    host: &PluginHost<'_>,
    interface: &InterfaceId,
    input: &[u8],
) -> Result<T, String>
where
    for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
{
    PluginContext::new(host, (), (), ())
        .kernel
        .decode_exact(interface, input)
        .map_err(|error| error.to_string())
}

#[doc(hidden)]
pub fn encode_runtime<T>(host: &PluginHost<'_>, response: &T) -> Result<Vec<u8>, String>
where
    for<'value> PhenixValue: From<&'value T>,
{
    PluginContext::new(host, (), (), ())
        .kernel
        .encode_value(response)
        .map_err(|error| error.to_string())
}

#[doc(hidden)]
pub fn encode_result_runtime<T, E>(
    _host: &PluginHost<'_>,
    response: &Result<T, E>,
) -> Result<Vec<u8>, String>
where
    for<'value> PhenixValue: From<&'value T>,
    for<'value> PhenixValue: From<&'value E>,
{
    let outcome = match response {
        Ok(response) => phenix_core::InvocationOutcome::success(PhenixValue::from(response)),
        Err(error) => phenix_core::InvocationOutcome::domain_error(PhenixValue::from(error)),
    };
    serde_json::to_vec(&outcome.into_transport_value()).map_err(|error| error.to_string())
}

fn decode_listener_runtime<T, F>(
    payload: &[u8],
    decode: F,
) -> Result<T, Box<dyn Error + Send + Sync>>
where
    F: FnOnce(&PhenixValue) -> Result<T, ValueError>,
{
    let value = serde_json::from_slice::<PhenixValue>(payload).map_err(|error| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("listener payload decoding failed: {error}"),
        )) as Box<dyn Error + Send + Sync>
    })?;
    decode(&value).map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
}

/// Author-free component routing generated for a stateful plugin struct.
pub trait StaticPluginComponentDispatch {
    fn dispatch_component(
        &self,
        component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String>;

    fn dispatch_layer(
        &self,
        service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        Err(format!("unsupported static plugin layer: {service}"))
    }

    #[doc(hidden)]
    fn listener_handler(
        _state: Weak<Self>,
        _owner: &PluginId,
        _component: &ComponentId,
        _method: &str,
        _generation: &GenerationId,
    ) -> Option<Arc<dyn PluginListener>>
    where
        Self: Sized + Send + Sync + 'static,
    {
        None
    }

    /// Adapt an already-constructed stateful plugin value to the kernel's erased runtime ABI.
    fn into_plugin_instance(self) -> Box<dyn PluginInstance>
    where
        Self: Sized + StaticPluginComponents + StaticPluginResources + Send + Sync + 'static,
    {
        Box::new(StaticPluginInstance::from_component_dispatch(self))
    }
}

pub type StaticPluginStart<T> = fn(&mut T, &PluginHost<'_>) -> Result<(), String>;
pub type StaticPluginStop<T> = fn(&mut T, &PluginHost<'_>) -> Result<(), String>;
pub type StaticPluginInvoke<T> =
    fn(&T, &ComponentId, &ServiceId, &[u8], &PluginHost<'_>) -> Result<Vec<u8>, String>;

/// Kernel adapter for an already-constructed Rust plugin value.
pub struct StaticPluginInstance<T> {
    plugin: Arc<T>,
    start: Option<StaticPluginStart<T>>,
    stop: Option<StaticPluginStop<T>>,
    invoke: StaticPluginInvoke<T>,
}

impl<T> StaticPluginInstance<T>
where
    T: StaticPluginResources,
{
    #[must_use]
    pub fn new(plugin: T, invoke: StaticPluginInvoke<T>) -> Self {
        Self {
            plugin: Arc::new(plugin),
            start: None,
            stop: None,
            invoke,
        }
    }
}

impl<T> StaticPluginInstance<T>
where
    T: StaticPluginResources + StaticPluginComponentDispatch,
{
    #[must_use]
    pub fn from_component_dispatch(plugin: T) -> Self {
        Self::new(plugin, T::dispatch_component)
    }
}

struct StaticListenerEventHandler<F> {
    owner: PluginId,
    listener: &'static str,
    handler: F,
}

impl<F> PluginListener for StaticListenerEventHandler<F>
where
    F: for<'runtime> Fn(
            &EventEnvelope,
            &PluginHost<'runtime>,
        ) -> Result<(), Box<dyn Error + Send + Sync>>
        + Send
        + Sync,
{
    fn handle(&self, event: &EventEnvelope, host: &PluginHost<'_>) -> Result<(), String> {
        match (self.handler)(event, host) {
            Ok(()) => Ok(()),
            Err(error) => {
                if let Some(value_error) = error.downcast_ref::<ValueError>() {
                    report_listener_mismatch(host, &self.owner, self.listener, event, value_error);
                }
                Err(error.to_string())
            }
        }
    }
}

fn report_listener_mismatch(
    host: &PluginHost<'_>,
    owner: &PluginId,
    listener: &str,
    source: &EventEnvelope,
    error: &ValueError,
) {
    if host.plugin() != owner {
        return;
    }
    let Ok(payload) = serde_json::to_vec(&serde_json::json!({
        "event": source.event_type.as_str(),
        "listener": listener,
        "direction": "listener",
        "error": error.to_string(),
    })) else {
        return;
    };
    let event_type = EventTypeId::parse(STRUCTURAL_MISMATCH_EVENT)
        .expect("static structural mismatch event type is valid");
    let _ = host.dispatch_event(
        event_type,
        STRUCTURAL_MISMATCH_EVENT_VERSION,
        source.causality_id,
        source.kernel_policy_revision,
        payload,
    );
}

impl<T> StaticPluginInstance<T> {
    #[must_use]
    pub fn with_start(mut self, start: StaticPluginStart<T>) -> Self {
        self.start = Some(start);
        self
    }

    #[must_use]
    pub fn with_stop(mut self, stop: StaticPluginStop<T>) -> Self {
        self.stop = Some(stop);
        self
    }

    #[must_use]
    pub fn into_inner(self) -> T {
        Arc::try_unwrap(self.plugin)
            .unwrap_or_else(|_| panic!("static plugin state still has live strong references"))
    }

    #[doc(hidden)]
    pub fn listener_handler<F>(
        owner: PluginId,
        listener: &'static str,
        handler: F,
    ) -> Arc<dyn PluginListener>
    where
        F: for<'runtime> Fn(
                &EventEnvelope,
                &PluginHost<'runtime>,
            ) -> Result<(), Box<dyn Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        Arc::new(StaticListenerEventHandler {
            owner,
            listener,
            handler,
        })
    }
}

fn static_plugin_service<T>(service: &ServiceId) -> Result<ComponentId, String>
where
    T: StaticPluginComponents,
{
    let mut matches = T::components().into_iter().filter(|component| {
        component
            .exports()
            .iter()
            .any(|export| export.terminal && export.interface.as_str() == service.as_str())
    });
    let component = matches
        .next()
        .ok_or_else(|| format!("unsupported static plugin service: {service}"))?;
    if matches.next().is_some() {
        return Err(format!("ambiguous static plugin service: {service}"));
    }
    Ok(component.id)
}

struct StaticPluginSharedInvocation<T> {
    plugin: Weak<T>,
    invoke: StaticPluginInvoke<T>,
}

impl<T> StaticPluginSharedInvocation<T> {
    fn plugin(&self) -> Result<Arc<T>, String> {
        self.plugin
            .upgrade()
            .ok_or_else(|| "static plugin state is unavailable".to_owned())
    }
}

impl<T> SharedPluginInvocation for StaticPluginSharedInvocation<T>
where
    T: StaticPluginComponents
        + StaticPluginResources
        + StaticPluginComponentDispatch
        + Send
        + Sync
        + 'static,
{
    fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let component = static_plugin_service::<T>(service)?;
        self.invoke_component(&component, service, input, host)
    }

    fn invoke_component(
        &self,
        component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let plugin = self.plugin()?;
        (self.invoke)(&plugin, component, service, input, host)
    }

    fn invoke_layer(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.plugin()?.dispatch_layer(service, input, host)
    }
}

impl<T> PluginInstance for StaticPluginInstance<T>
where
    T: StaticPluginComponents
        + StaticPluginResources
        + StaticPluginComponentDispatch
        + Send
        + Sync
        + 'static,
{
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let Some(start) = self.start else {
            return Ok(());
        };
        let plugin = Arc::get_mut(&mut self.plugin).ok_or_else(|| {
            "static plugin has live invocation references during start".to_owned()
        })?;
        start(plugin, host)
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(StaticPluginSharedInvocation {
            plugin: Arc::downgrade(&self.plugin),
            invoke: self.invoke,
        }))
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let component = static_plugin_service::<T>(service)?;
        self.invoke_component(&component, service, input, host)
    }

    fn invoke_component(
        &mut self,
        component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        (self.invoke)(&self.plugin, component, service, input, host)
    }

    fn invoke_layer(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        self.plugin.dispatch_layer(service, input, host)
    }

    fn bind_plugin_listener(
        &mut self,
        listener: &ResolvedListener,
        generation: &GenerationId,
    ) -> Option<Result<Arc<dyn PluginListener>, String>> {
        Some(
            T::listener_handler(
                Arc::downgrade(&self.plugin),
                &listener.owning_plugin,
                &listener.component,
                &listener.declaration.method,
                generation,
            )
            .ok_or_else(|| {
                format!(
                    "unsupported component listener: {}/{}",
                    listener.component, listener.declaration.method
                )
            }),
        )
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let Some(stop) = self.stop else {
            return Ok(());
        };
        let plugin = Arc::get_mut(&mut self.plugin)
            .ok_or_else(|| "static plugin has live invocation references during stop".to_owned())?;
        stop(plugin, host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StaticComponentDescriptor, StaticResourceDescriptor};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Component;

    impl StaticComponentDispatch for Component {
        fn dispatch(
            &self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }
    }

    impl StaticComponentRuntimeDispatch for Component {
        fn dispatch_runtime(
            &self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }

        fn dispatch_layer_runtime(
            &self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Option<Result<LayerResult, String>> {
            None
        }
    }

    #[test]
    fn generated_component_dispatch_adapts_to_plugin_instance() {
        let instance: Box<dyn PluginInstance> = Component.into_plugin_instance();
        assert!(instance.shared_invocation().is_some());
    }

    #[test]
    fn default_component_builds_a_kernel_plugin_instance() {
        let instance: Box<dyn PluginInstance> = Component::default_plugin_instance();
        assert!(instance.shared_invocation().is_some());
    }

    struct StatefulPlugin {
        calls: AtomicUsize,
    }

    impl StaticPluginComponents for StatefulPlugin {
        fn components() -> Vec<StaticComponentDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginResources for StatefulPlugin {
        fn resources() -> Vec<StaticResourceDescriptor> {
            Vec::new()
        }
    }

    impl StaticPluginComponentDispatch for StatefulPlugin {
        fn dispatch_component(
            &self,
            _component: &ComponentId,
            _service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(input.to_vec())
        }
    }

    static STARTS: AtomicUsize = AtomicUsize::new(0);
    static STOPS: AtomicUsize = AtomicUsize::new(0);

    fn start(_plugin: &mut StatefulPlugin, _host: &PluginHost<'_>) -> Result<(), String> {
        STARTS.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn stop(_plugin: &mut StatefulPlugin, _host: &PluginHost<'_>) -> Result<(), String> {
        STOPS.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn invoke(
        plugin: &StatefulPlugin,
        _component: &ComponentId,
        _service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        plugin.calls.fetch_add(1, Ordering::Relaxed);
        Ok(input.to_vec())
    }

    #[test]
    fn generated_stateful_plugin_dispatch_adapts_to_plugin_instance() {
        let instance: Box<dyn PluginInstance> = StatefulPlugin {
            calls: AtomicUsize::new(7),
        }
        .into_plugin_instance();
        assert!(instance.shared_invocation().is_some());
    }

    #[test]
    fn stateful_plugin_adapter_preserves_non_default_state_and_callbacks() {
        let instance = StaticPluginInstance::new(
            StatefulPlugin {
                calls: AtomicUsize::new(7),
            },
            invoke,
        )
        .with_start(start)
        .with_stop(stop);

        assert_eq!(instance.plugin.calls.load(Ordering::Relaxed), 7);
        assert!(instance.start.is_some());
        assert!(instance.stop.is_some());
    }
}
