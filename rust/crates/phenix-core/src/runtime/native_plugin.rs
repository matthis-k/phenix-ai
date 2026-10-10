//! Safe kernel adapter for the intrinsic C-compatible native library loader.
//!
//! The only raw operations live in `phenix-native-loader`. Each resolved
//! plugin instance owns an independently prepared native ABI instance, while
//! the existing Component/Service/Layer dispatcher remains authoritative.
use super::{Kernel, PluginHost, PluginInstance, SharedPluginInvocation};
use crate::{ComponentId, KernelError, PluginId, ServiceId};
use phenix_native_loader::{
    NativeInvocation, NativeLoadError, NativePluginInstance, NativePluginLibrary,
};
use phenix_plugin_abi::NativeCallTicket;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

static NEXT_NATIVE_INSTANCE: AtomicU64 = AtomicU64::new(1);
static NEXT_NATIVE_TICKET: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum NativeRegistrationError {
    Kernel(KernelError),
    Loader(NativeLoadError),
}

impl From<KernelError> for NativeRegistrationError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}
impl From<NativeLoadError> for NativeRegistrationError {
    fn from(error: NativeLoadError) -> Self {
        Self::Loader(error)
    }
}

struct NativePluginAdapter {
    instance: Arc<Mutex<NativePluginInstance>>,
    endpoint_id: u64,
}

struct NativeSharedEndpoint {
    instance: Arc<Mutex<NativePluginInstance>>,
    endpoint_id: u64,
}

fn native_library_factory(
    library: Arc<NativePluginLibrary>,
) -> impl Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static {
    move || {
        let endpoint_id = NEXT_NATIVE_INSTANCE.fetch_add(1, Ordering::Relaxed);
        Box::new(NativePluginAdapter {
            instance: Arc::new(Mutex::new(library.instance(endpoint_id))),
            endpoint_id,
        })
    }
}

impl PluginInstance for NativePluginAdapter {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.instance
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .prepare_and_start()
            .map_err(|error| format!("native plugin preparation failed: {error}"))
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(NativeSharedEndpoint {
            instance: Arc::clone(&self.instance),
            endpoint_id: self.endpoint_id,
        }))
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.instance
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stop_and_destroy()
            .map_err(|error| format!("native plugin stop refused: {error}"))
    }
}

impl SharedPluginInvocation for NativeSharedEndpoint {
    fn invoke_component(
        &self,
        component: &ComponentId,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        // A distinct ABI-instance identity also distinguishes resident
        // generations of the same plugin. The call number is never reused
        // across independently admitted kernel roots.
        let ticket = NativeCallTicket {
            root_id: self.endpoint_id,
            call_id: NEXT_NATIVE_TICKET.fetch_add(1, Ordering::Relaxed),
        };
        let cancellation = host.cancellation_token().cloned().map(|token| {
            Arc::new(move || token.is_cancelled()) as Arc<dyn Fn() -> bool + Send + Sync>
        });
        let (begin, wake) = {
            let mut instance = self
                .instance
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let wake = instance.wake_handle();
            let begin = instance
                .begin_with_cancellation(
                    ticket,
                    1,
                    component.as_str(),
                    service.as_str(),
                    input,
                    cancellation,
                )
                .map_err(|error| format!("native begin failed: {error}"))?;
            (begin, wake)
        };
        let mut next = begin;
        let mut cancellation_sent = false;
        loop {
            match next {
                NativeInvocation::Ready(bytes) => return Ok(bytes),
                NativeInvocation::Failed(bytes) => {
                    return Err(String::from_utf8_lossy(&bytes).into_owned());
                }
                NativeInvocation::Pending => {
                    if !cancellation_sent
                        && host
                            .cancellation_token()
                            .is_some_and(|token| token.is_cancelled())
                    {
                        self.instance
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .cancel(ticket)
                            .map_err(|error| format!("native cancel failed: {error}"))?;
                        cancellation_sent = true;
                    }
                    // Wake requests always undergo a correlated poll; they
                    // cannot forge a result. Poll periodically even for a
                    // provider that omits wakes. The wait interval is neither
                    // a provider timeout nor an execution settlement.
                    wake.wait_for(ticket, Duration::from_millis(250));
                    next = self
                        .instance
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .poll(ticket)
                        .map_err(|error| format!("native poll failed: {error}"))?;
                }
            }
        }
    }
}

impl Kernel {
    /// Load one independently compiled ABI-v1 native artifact and register
    /// its versioned native invoker as a replacement implementation in the
    /// canonical component dispatch path. The plugin must be selected by the
    /// generation and explicitly registered; no manifest discovery, authority
    /// expansion, or implicit provider fallback is permitted here.
    ///
    /// The transitional manifest kind is `Embedded` because the native ABI
    /// loader is an intrinsic bootstrap and is not yet a separate resolved
    /// artifact kind. This method never turns a guest adapter into bootstrap.
    pub fn register_native_shared_library(
        &mut self,
        plugin: PluginId,
        path: &Path,
    ) -> Result<(), NativeRegistrationError> {
        let library = NativePluginLibrary::load(path)?;
        self.register_embedded_factory(plugin, native_library_factory(library))?;
        Ok(())
    }
}
