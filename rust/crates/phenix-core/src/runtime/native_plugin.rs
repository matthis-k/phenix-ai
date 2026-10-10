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
    ArtifactRead(String),
    LocatorMismatch { declared: String, selected: String },
    RevisionMismatch {
        expected: crate::ArtifactRevision,
        actual: crate::ArtifactRevision,
    },
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
}

fn native_library_factory(
    library: Arc<NativePluginLibrary>,
) -> impl Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static {
    move || {
        let endpoint_id = NEXT_NATIVE_INSTANCE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1))
            .expect("native plugin instance identity space exhausted");
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
        // Core supplies the actual root identity and the globally unique
        // callback number. A plugin never invents either correlation value.
        let call_id = NEXT_NATIVE_TICKET
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1))
            .map_err(|_| "native ABI call identity space exhausted".to_owned())?;
        let ticket = NativeCallTicket {
            root_id: host.root_id(),
            call_id,
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
    /// The declaration must explicitly select `PluginExecution::Native`
    /// with the exact artifact locator and SHA-256 content revision. Dynamic
    /// module bytes are never discovered from plugin code at resolution time.
    pub fn register_native_shared_library(
        &mut self,
        plugin: PluginId,
        path: &Path,
    ) -> Result<(), NativeRegistrationError> {
        let manifest = self.config().manifest(&plugin).ok_or_else(|| {
            NativeRegistrationError::Kernel(KernelError::UnknownPlugin(plugin.clone()))
        })?;
        let artifact = match &manifest.execution {
            crate::PluginExecution::Native { artifact } => artifact,
            _ => {
                return Err(NativeRegistrationError::Kernel(
                    KernelError::WrongExecutionKind(plugin),
                ));
            }
        };
        let selected = path.to_string_lossy().into_owned();
        if artifact.locator != selected {
            return Err(NativeRegistrationError::LocatorMismatch {
                declared: artifact.locator.clone(),
                selected,
            });
        }
        let content = std::fs::read(path)
            .map_err(|error| NativeRegistrationError::ArtifactRead(error.to_string()))?;
        let actual = crate::ArtifactRevision::from_content(&content);
        if actual != artifact.revision {
            return Err(NativeRegistrationError::RevisionMismatch {
                expected: artifact.revision.clone(),
                actual,
            });
        }
        let library = NativePluginLibrary::load(path)?;
        // This is an already-resolved Native artifact. The ordinary factory
        // registration API intentionally remains Embedded-only.
        self.preload_embedded_factory(plugin, native_library_factory(library));
        Ok(())
    }
}
