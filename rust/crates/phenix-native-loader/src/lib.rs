#![deny(unsafe_op_in_unsafe_fn)]

//! Intrinsic loader for the versioned Phenix native-plugin ABI.
//!
//! This is the *only* package allowed to load shared libraries and call the
//! ABI's raw pointers. Core and the authoring SDK remain safe Rust. A native
//! plugin is trusted code, not an isolation boundary. Unsafe operations are
//! confined to opening/closing the image, reading its prevalidated function
//! table, and transferring its explicitly owned result buffers.

use phenix_plugin_abi::{
    FEATURE_HOST_CANCELLATION, FEATURE_PENDING_CALLS, FEATURE_WAKE_POLL, NativeAbiError,
    NativeAbiHeader, NativeCallRequest, NativeCallResult, NativeCallTicket, NativeHostV1,
    NativeOwnedBuffer, NativePluginEntryV1, NativePluginV1, NativeSlice, RESULT_ERROR,
    RESULT_PENDING, RESULT_READY,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString, c_char, c_int, c_void},
    fmt,
    path::Path,
    sync::{
        Arc, Condvar, LazyLock, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Sender},
    },
    time::Duration,
};

pub const NATIVE_SUPPORTED_FEATURES: u64 =
    FEATURE_PENDING_CALLS | FEATURE_WAKE_POLL | FEATURE_HOST_CANCELLATION;

/// Opaque IDs, not pointers, are passed as host callback contexts. The global
/// registry stores weak references and cannot dereference a callback after
/// its generation was retired. There is no TLS-derived authority.
static NEXT_HOST_ID: AtomicUsize = AtomicUsize::new(1);
static HOST_REGISTRY: LazyLock<Mutex<BTreeMap<usize, Weak<WakeHub>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

struct WakeHub {
    calls: Mutex<BTreeMap<(u64, u64), CallPermit>>,
    notified: Condvar,
}

struct CallPermit {
    wake: bool,
    cancelled: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    import_sender: Option<Sender<NativeHostImportRequest>>,
}

/// One independently authorized native import request. The owning Core
/// call thread must dispatch this through its *current* scoped PluginHost;
/// a guest callback does not carry an authority handle in the ABI.
pub struct NativeHostImportRequest {
    pub interface: String,
    pub input: Vec<u8>,
    pub reply: Sender<Result<Vec<u8>, String>>,
}

unsafe extern "C" fn native_release_bytes(_: *mut c_void, ptr: *mut u8, len: usize) {
    if len != 0 {
        // SAFETY: exactly these bytes were allocated as Box<[u8]> by
        // host_owned_result, with no reallocation or allocator crossing.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}

fn host_owned_result(ticket: NativeCallTicket, result: Result<Vec<u8>, String>) -> NativeCallResult {
    let (status, bytes) = match result {
        Ok(value) => (RESULT_READY, value),
        Err(error) => (RESULT_ERROR, error.into_bytes()),
    };
    let bytes = bytes.into_boxed_slice();
    let len = bytes.len();
    let ptr = if len == 0 {
        std::ptr::null_mut()
    } else {
        Box::into_raw(bytes).cast::<u8>()
    };
    NativeCallResult {
        status,
        ticket,
        payload: NativeOwnedBuffer {
            ptr,
            len,
            owner: std::ptr::null_mut(),
            release: Some(native_release_bytes),
        },
    }
}

unsafe extern "C" fn native_invoke_import(
    context: *mut c_void,
    ticket: NativeCallTicket,
    requested_interface: NativeSlice,
    input: NativeSlice,
) -> NativeCallResult {
    let Some(hub) = hub_for(context) else {
        return host_owned_result(ticket, Err("native host scope has retired".into()));
    };
    let sender = {
        let calls = hub.calls.lock().unwrap_or_else(|error| error.into_inner());
        calls
            .get(&(ticket.root_id, ticket.call_id))
            .and_then(|permit| permit.import_sender.clone())
    };
    let Some(sender) = sender else {
        return host_owned_result(ticket, Err("native import call is not admitted".into()));
    };
    if (requested_interface.len != 0 && requested_interface.ptr.is_null())
        || (input.len != 0 && input.ptr.is_null())
        || requested_interface.len > isize::MAX as usize
        || input.len > isize::MAX as usize
    {
        return host_owned_result(ticket, Err("invalid native import buffer".into()));
    }
    let bytes = |value: NativeSlice| {
        if value.len == 0 {
            &[][..]
        } else {
            // SAFETY: an admitted trusted native plugin guarantees the
            // borrowed slice remains valid through this callback.
            unsafe { std::slice::from_raw_parts(value.ptr, value.len) }
        }
    };
    let interface = match std::str::from_utf8(bytes(requested_interface)) {
        Ok(interface) => interface.to_owned(),
        Err(_) => return host_owned_result(ticket, Err("non-UTF8 interface ID".into())),
    };
    let (reply, receiver) = mpsc::channel();
    if sender
        .send(NativeHostImportRequest {
            interface,
            input: bytes(input).to_vec(),
            reply,
        })
        .is_err()
    {
        return host_owned_result(ticket, Err("native host dispatcher has closed".into()));
    }
    host_owned_result(
        ticket,
        receiver
            .recv()
            .unwrap_or_else(|_| Err("native import dispatcher disconnected".into())),
    )
}

/// Clonable wake receiver for a root-owned native worker. An untrusted plugin
/// may not fabricate a settlement with wake; Core always polls its result.
#[derive(Clone)]
pub struct NativeWakeHandle {
    hub: Arc<WakeHub>,
}

impl NativeWakeHandle {
    /// Wait for a matching wake, or return to check cancellation and poll.
    /// No timeout ever settles the invocation or retires a generation.
    pub fn wait_for(&self, ticket: NativeCallTicket, interval: Duration) -> bool {
        let mut calls = self
            .hub
            .calls
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let key = (ticket.root_id, ticket.call_id);
        if !calls.get(&key).is_some_and(|call| call.wake) {
            calls = self
                .hub
                .notified
                .wait_timeout(calls, interval)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        if let Some(call) = calls.get_mut(&key) {
            let signalled = call.wake;
            call.wake = false;
            signalled
        } else {
            false
        }
    }
}

fn hub_for(context: *mut c_void) -> Option<Arc<WakeHub>> {
    // A numeric registry identity is never dereferenced as a C pointer.
    HOST_REGISTRY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(context as usize))
        .and_then(Weak::upgrade)
}

unsafe extern "C" fn native_wake(context: *mut c_void, ticket: NativeCallTicket) {
    if let Some(hub) = hub_for(context) {
        let mut calls = hub.calls.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(call) = calls.get_mut(&(ticket.root_id, ticket.call_id)) {
            call.wake = true;
            hub.notified.notify_all();
        }
    }
}

unsafe extern "C" fn native_is_cancelled(context: *mut c_void, ticket: NativeCallTicket) -> u32 {
    let Some(hub) = hub_for(context) else {
        return 1; // A retired host cannot authorize a late callback.
    };
    let permitted = {
        let calls = hub.calls.lock().unwrap_or_else(|error| error.into_inner());
        calls
            .get(&(ticket.root_id, ticket.call_id))
            .map(|permit| permit.cancelled.clone())
    };
    match permitted {
        None => 1,
        Some(Some(predicate)) if predicate() => 1,
        Some(_) => 0,
    }
}

#[derive(Debug)]
pub enum NativeLoadError {
    UnsupportedPlatform,
    InvalidPath,
    Open(String),
    MissingEntrypoint(String),
    NullEntrypoint,
    IncompatibleAbi(NativeAbiError),
    InvalidResult(NativeAbiError),
    OversizedResult(usize),
    MissingOrDuplicateCall(NativeCallTicket),
    UnexpectedStatus {
        expected_pending: bool,
        observed: u32,
    },
    Lifecycle(&'static str),
    PluginFailed(u32),
}

impl fmt::Display for NativeLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for NativeLoadError {}

#[cfg(unix)]
mod image {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[cfg_attr(not(target_os = "macos"), link(name = "dl"))]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    pub struct NativeImage {
        pub handle: *mut c_void,
    }

    // POSIX native library handles are valid from any thread. The enclosing
    // NativePluginLibrary serializes every call into its negotiated table,
    // and drop closes the image only after the last Arc owner disappears.
    unsafe impl Send for NativeImage {}
    unsafe impl Sync for NativeImage {}

    impl NativeImage {
        pub fn open(path: &Path) -> Result<Self, NativeLoadError> {
            let filename = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| NativeLoadError::InvalidPath)?;
            // SAFETY: filename is terminated, flags have POSIX-defined values.
            let handle = unsafe { dlopen(filename.as_ptr(), 2) };
            if handle.is_null() {
                // SAFETY: dlerror returns either null or a terminated loader error.
                let message = unsafe {
                    let error = dlerror();
                    if error.is_null() {
                        "dlopen failed".into()
                    } else {
                        CStr::from_ptr(error).to_string_lossy().into_owned()
                    }
                };
                return Err(NativeLoadError::Open(message));
            }
            Ok(Self { handle })
        }

        pub fn entry(&self) -> Result<NativePluginEntryV1, NativeLoadError> {
            // SAFETY: the symbol name is terminated; self owns a live handle.
            let symbol = unsafe { dlsym(self.handle, c"phenix_plugin_entry_v1".as_ptr()) };
            if symbol.is_null() {
                return Err(NativeLoadError::MissingEntrypoint(
                    "phenix_plugin_entry_v1".into(),
                ));
            }
            // SAFETY: the versioned plugin contract requires this symbol to
            // have precisely NativePluginEntryV1's extern-C signature. The
            // loader never guesses a Rust vtable layout or transmutes data.
            Ok(unsafe { std::mem::transmute::<*mut c_void, NativePluginEntryV1>(symbol) })
        }
    }

    impl Drop for NativeImage {
        fn drop(&mut self) {
            // SAFETY: this is the final Arc owner of the dlopen handle.
            unsafe { dlclose(self.handle) };
        }
    }
}

#[cfg(not(unix))]
mod image {
    use super::*;

    pub struct NativeImage;

    impl NativeImage {
        pub fn open(_: &Path) -> Result<Self, NativeLoadError> {
            Err(NativeLoadError::UnsupportedPlatform)
        }

        pub fn entry(&self) -> Result<NativePluginEntryV1, NativeLoadError> {
            Err(NativeLoadError::UnsupportedPlatform)
        }
    }
}

pub struct NativePluginLibrary {
    _image: image::NativeImage,
    table: *const NativePluginV1,
    gate: Mutex<()>,
}

// The image is resident for the lifetime of every cloned Arc, and the gate
// serializes all plugin callbacks. Native plugins are trusted in-process code:
// a plugin that violates the C ABI's own lifetime/threading promises cannot be
// memory-isolated by these Rust wrappers.
unsafe impl Send for NativePluginLibrary {}
unsafe impl Sync for NativePluginLibrary {}

impl NativePluginLibrary {
    pub fn load(path: &Path) -> Result<Arc<Self>, NativeLoadError> {
        let image = image::NativeImage::open(path)?;
        let entry = image.entry()?;
        // SAFETY: native plugin entry is the previously negotiated C symbol.
        let table = unsafe { entry() };
        if table.is_null() {
            return Err(NativeLoadError::NullEntrypoint);
        }
        // SAFETY: the ABI requires an entrypoint to return at least a readable
        // NativeAbiHeader prefix, even if the rest of its table is absent.
        let header = unsafe { std::ptr::read_unaligned(table.cast::<NativeAbiHeader>()) };
        header
            .validate(
                std::mem::size_of::<NativePluginV1>(),
                NATIVE_SUPPORTED_FEATURES,
            )
            .map_err(NativeLoadError::IncompatibleAbi)?;
        // SAFETY: header's table-size claim permits reading the full static
        // function table. A malicious native library is trusted, not sandboxed.
        unsafe { (*table).validate(NATIVE_SUPPORTED_FEATURES) }
            .map_err(NativeLoadError::IncompatibleAbi)?;
        Ok(Arc::new(Self {
            _image: image,
            table,
            gate: Mutex::new(()),
        }))
    }

    /// Load from the already-verified artifact bytes, not from a mutable
    /// locator that could be swapped after content-revision validation.
    ///
    /// Stage into a newly created private directory, map the image, and
    /// unlink the staging file immediately. POSIX keeps loaded pages resident
    /// until the last handle closes. Relative dynamic dependencies are not
    /// discovered or implicitly trusted; native artifacts must be self-
    /// contained or use explicit absolute runtime library references.
    pub fn load_staged(content: &[u8]) -> Result<Arc<Self>, NativeLoadError> {
        static NEXT_STAGE: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "phenix-native-{}-{id}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| NativeLoadError::Open(error.to_string()))?
                .as_nanos(),
        ));
        std::fs::create_dir(&directory)
            .map_err(|error| NativeLoadError::Open(error.to_string()))?;
        let staged = directory.join(format!(
            "module.{}",
            std::env::consts::DLL_EXTENSION
        ));
        let result = (|| {
            std::fs::write(&staged, content)
                .map_err(|error| NativeLoadError::Open(error.to_string()))?;
            Self::load(&staged)
        })();
        let _ = std::fs::remove_file(staged);
        let _ = std::fs::remove_dir(directory);
        result
    }

    pub fn header(&self) -> NativeAbiHeader {
        let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: the table and image are resident for self's lifetime.
        unsafe { (*self.table).header }
    }

    /// Construct a generation-scoped instance. Host callback pointers must
    /// remain valid through stop + destroy, including all in-flight calls.
    /// This constructor supplies no host capabilities beyond an inert table.
    #[must_use]
    pub fn instance(self: &Arc<Self>, generation: u64) -> NativePluginInstance {
        let id = NEXT_HOST_ID.fetch_add(1, Ordering::Relaxed);
        assert_ne!(id, 0, "native host identity space exhausted");
        let hub = Arc::new(WakeHub {
            calls: Mutex::new(BTreeMap::new()),
            notified: Condvar::new(),
        });
        HOST_REGISTRY
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, Arc::downgrade(&hub));
        NativePluginInstance {
            module: Arc::clone(self),
            hub,
            registry_id: id,
            host: Box::new(NativeHostV1 {
                header: NativeAbiHeader {
                    major: phenix_plugin_abi::NATIVE_ABI_MAJOR,
                    minor: phenix_plugin_abi::NATIVE_ABI_MINOR,
                    table_bytes: std::mem::size_of::<NativeHostV1>(),
                    required_features: 0,
                },
                context: id as *mut c_void,
                is_cancelled: Some(native_is_cancelled),
                wake: Some(native_wake),
                invoke_import: Some(native_invoke_import),
            }),
            generation,
            state: NativeInstanceState::Created,
            pending: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeInstanceState {
    Created,
    Active,
    Stopped,
    Destroyed,
}

#[derive(Debug, Eq, PartialEq)]
pub enum NativeInvocation {
    Ready(Vec<u8>),
    Failed(Vec<u8>),
    Pending,
}

/// Generation-owned plugin state; the only raw C callbacks are made through a
/// guarded, validated table resident in NativePluginLibrary.
pub struct NativePluginInstance {
    module: Arc<NativePluginLibrary>,
    hub: Arc<WakeHub>,
    registry_id: usize,
    host: Box<NativeHostV1>,
    generation: u64,
    state: NativeInstanceState,
    pending: BTreeSet<(u64, u64)>,
}

// The opaque foreign context and boxed host table may be moved between
// kernel-managed worker threads only under the owner's instance mutex. The
// resident module separately serializes all calls into its C function table.
// Neither the context pointer nor borrowed request pointers are dereferenced
// by the host without checking their lifetime first.
unsafe impl Send for NativePluginInstance {}

impl NativePluginInstance {
    pub fn wake_handle(&self) -> NativeWakeHandle {
        NativeWakeHandle {
            hub: Arc::clone(&self.hub),
        }
    }

    pub fn state(&self) -> NativeInstanceState {
        self.state
    }

    pub fn prepare_and_start(&mut self) -> Result<(), NativeLoadError> {
        if self.state != NativeInstanceState::Created {
            return Err(NativeLoadError::Lifecycle(
                "prepare requires created instance",
            ));
        }
        let _guard = self.module.gate.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: the table was negotiated at load, host is boxed and remains
        // resident through destroy, and C ABI callbacks must not unwind.
        let table = unsafe { &*self.module.table };
        let prepared = unsafe {
            table.prepare.expect("validated ABI table")(table.context, &*self.host, self.generation)
        };
        if prepared != 0 {
            return Err(NativeLoadError::PluginFailed(prepared));
        }
        let started =
            unsafe { table.start.expect("validated ABI table")(table.context, self.generation) };
        if started != 0 {
            return Err(NativeLoadError::PluginFailed(started));
        }
        self.state = NativeInstanceState::Active;
        Ok(())
    }

    /// Native request buffers are borrowed *only* for this call; no public
    /// API accepts arbitrary raw input pointers.
    pub fn begin_component(
        &mut self,
        ticket: NativeCallTicket,
        scope: u64,
        component: &str,
        interface: &str,
        input: &[u8],
    ) -> Result<NativeInvocation, NativeLoadError> {
        self.begin_with_cancellation(ticket, scope, component, interface, input, None, None)
    }

    /// Register this callback's attenuation/cancellation predicate before
    /// entering the native plugin, including synchronous reentrant wakeups.
    pub fn begin_with_cancellation(
        &mut self,
        ticket: NativeCallTicket,
        scope: u64,
        component: &str,
        interface: &str,
        input: &[u8],
        cancellation: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
        import_sender: Option<Sender<NativeHostImportRequest>>,
    ) -> Result<NativeInvocation, NativeLoadError> {
        if self.state != NativeInstanceState::Active {
            return Err(NativeLoadError::Lifecycle("begin requires active instance"));
        }
        if ticket.root_id == 0 || ticket.call_id == 0 {
            return Err(NativeLoadError::InvalidResult(
                NativeAbiError::InvalidTicket,
            ));
        }
        let key = (ticket.root_id, ticket.call_id);
        if self.pending.contains(&key) {
            return Err(NativeLoadError::MissingOrDuplicateCall(ticket));
        }
        {
            let mut calls = self.hub.calls.lock().unwrap_or_else(|e| e.into_inner());
            if calls
                .insert(
                    key,
                    CallPermit {
                        wake: false,
                        cancelled: cancellation,
                        import_sender,
                    },
                )
                .is_some()
            {
                return Err(NativeLoadError::MissingOrDuplicateCall(ticket));
            }
        }
        let _guard = self.module.gate.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: request's borrowed bytes must remain valid for this call;
        // native plugin must not retain borrowed pointers after begin returns.
        let table = unsafe { &*self.module.table };
        let borrowed = |bytes: &[u8]| NativeSlice {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        };
        let request = NativeCallRequest {
            ticket,
            scope,
            component: borrowed(component.as_bytes()),
            interface: borrowed(interface.as_bytes()),
            input: borrowed(input),
        };
        let result = unsafe { table.begin.expect("validated ABI table")(table.context, request) };
        let value = decode_result(result, ticket);
        if matches!(value, Ok(NativeInvocation::Pending)) {
            self.pending.insert(key);
        } else {
            self.hub
                .calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
        }
        value
    }

    pub fn poll(&mut self, ticket: NativeCallTicket) -> Result<NativeInvocation, NativeLoadError> {
        if self.state != NativeInstanceState::Active
            || !self.pending.contains(&(ticket.root_id, ticket.call_id))
        {
            return Err(NativeLoadError::MissingOrDuplicateCall(ticket));
        }
        let _guard = self.module.gate.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: pending ticket was admitted by this instance, and the image
        // remains resident for its lifetime.
        let table = unsafe { &*self.module.table };
        let raw = unsafe { table.poll.expect("validated ABI table")(table.context, ticket) };
        let result = decode_result(raw, ticket)?;
        if !matches!(result, NativeInvocation::Pending) {
            self.pending.remove(&(ticket.root_id, ticket.call_id));
            self.hub
                .calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&(ticket.root_id, ticket.call_id));
        }
        Ok(result)
    }

    pub fn cancel(&mut self, ticket: NativeCallTicket) -> Result<(), NativeLoadError> {
        if !self.pending.contains(&(ticket.root_id, ticket.call_id)) {
            return Err(NativeLoadError::MissingOrDuplicateCall(ticket));
        }
        let _guard = self.module.gate.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: cancellation is cooperative and cannot retire the task.
        let table = unsafe { &*self.module.table };
        unsafe { table.cancel.expect("validated ABI table")(table.context, ticket) };
        Ok(())
    }

    pub fn stop_and_destroy(&mut self) -> Result<(), NativeLoadError> {
        if !self.pending.is_empty() {
            return Err(NativeLoadError::Lifecycle(
                "pending callbacks must settle before stop",
            ));
        }
        if self.state != NativeInstanceState::Active {
            return Err(NativeLoadError::Lifecycle("stop requires active instance"));
        }
        let _guard = self.module.gate.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: no active tickets remain and the host remains valid through
        // both lifecycle calls; the image is pinned by Arc.
        let table = unsafe { &*self.module.table };
        let stopped =
            unsafe { table.stop.expect("validated ABI table")(table.context, self.generation) };
        if stopped != 0 {
            return Err(NativeLoadError::PluginFailed(stopped));
        }
        self.state = NativeInstanceState::Stopped;
        unsafe { table.destroy.expect("validated ABI table")(table.context, self.generation) };
        self.state = NativeInstanceState::Destroyed;
        Ok(())
    }
}

impl Drop for NativePluginInstance {
    fn drop(&mut self) {
        if self.state == NativeInstanceState::Active && self.pending.is_empty() {
            let _ = self.stop_and_destroy();
        }
        HOST_REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.registry_id);
        if self.state != NativeInstanceState::Destroyed {
            // Unfinished or rejected stop leaves native code potentially
            // running. Prefer a deliberately retained image/host to a UAF:
            // caller cannot treat dropping a result as proof of settlement.
            std::mem::forget(Arc::clone(&self.module));
            let inactive = Box::new(NativeHostV1 {
                header: self.host.header,
                context: std::ptr::null_mut(),
                is_cancelled: None,
                wake: None,
                invoke_import: None,
            });
            let still_referenced = std::mem::replace(&mut self.host, inactive);
            std::mem::forget(still_referenced);
        }
    }
}

/// Borrowed input survives a C call only; owned output is copied and released
/// with the exact producer callback. Every result is correlated *before* the
/// host can read or interpret its bytes.
fn decode_result(
    result: NativeCallResult,
    expected: NativeCallTicket,
) -> Result<NativeInvocation, NativeLoadError> {
    result
        .validate_for(expected)
        .map_err(NativeLoadError::InvalidResult)?;
    if result.status == RESULT_PENDING {
        return Ok(NativeInvocation::Pending);
    }
    let NativeOwnedBuffer {
        ptr,
        len,
        owner,
        release,
    } = result.payload;
    if len > isize::MAX as usize {
        return Err(NativeLoadError::OversizedResult(len));
    }
    // SAFETY: terminal payload validation requires ptr != null if len > 0.
    // Every byte is borrowed only until the mandatory matching release.
    let bytes = if len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    };
    // SAFETY: ABI promises a paired allocator release for each owned buffer.
    unsafe { release.expect("validated result requires release")(owner, ptr, len) };
    match result.status {
        RESULT_READY => Ok(NativeInvocation::Ready(bytes)),
        RESULT_ERROR => Ok(NativeInvocation::Failed(bytes)),
        other => Err(NativeLoadError::UnexpectedStatus {
            expected_pending: false,
            observed: other,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_native_image_fails_without_executing_untrusted_code() {
        let invalid =
            NativePluginLibrary::load(Path::new("/path/that/does/not/exist/phenix-plugin.so"));
        assert!(matches!(
            invalid,
            Err(NativeLoadError::Open(_)) | Err(NativeLoadError::UnsupportedPlatform)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn independently_compiled_native_library_executes_deferred_v1_call() {
        use std::{
            process::Command,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = std::env::temp_dir().join(format!(
            "phenix-native-loader-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&out).unwrap();
        let lib = out.join(format!(
            "libnative_fixture.{}",
            std::env::consts::DLL_EXTENSION
        ));
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pending_plugin.rs");
        let build = Command::new("rustc")
            .arg("--edition=2024")
            .arg("--crate-type=cdylib")
            .arg("--crate-name=phenix_native_test_fixture")
            .arg(&fixture)
            .arg("-o")
            .arg(&lib)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "native fixture could not compile: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let module = NativePluginLibrary::load(&lib).unwrap();
        let mut instance = module.instance(17);
        instance.prepare_and_start().unwrap();
        assert_eq!(instance.state(), NativeInstanceState::Active);
        let ticket = NativeCallTicket {
            root_id: 3,
            call_id: 9,
        };
        let first = instance
            .begin_component(ticket, 5, "fixture.native", "fixture.test@1", b"hello")
            .unwrap();
        assert_eq!(first, NativeInvocation::Pending);
        assert_eq!(
            instance.poll(ticket).unwrap(),
            NativeInvocation::Ready(b"fixture finished".to_vec())
        );
        assert!(matches!(
            instance.poll(ticket),
            Err(NativeLoadError::MissingOrDuplicateCall(_))
        ));
        instance.stop_and_destroy().unwrap();
        assert_eq!(instance.state(), NativeInstanceState::Destroyed);
        drop(instance);
        drop(module);
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn result_correlations_fail_closed_before_deserialization() {
        let ticket = NativeCallTicket {
            root_id: 1,
            call_id: 2,
        };
        let result = NativeCallResult {
            ticket: NativeCallTicket {
                root_id: 3,
                call_id: 2,
            },
            status: RESULT_PENDING,
            payload: NativeOwnedBuffer {
                ptr: std::ptr::null_mut(),
                len: 0,
                owner: std::ptr::null_mut(),
                release: None,
            },
        };
        assert!(matches!(
            decode_result(result, ticket),
            Err(NativeLoadError::InvalidResult(
                NativeAbiError::WrongCorrelation { .. }
            ))
        ));
    }
}
