//! ABI 1.0. Pointers are borrowed for the synchronous call unless explicitly owned.
use crate::{Code, Failure, Value, WireProfile};
use std::ffi::c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bytes {
    pub ptr: *const u8,
    pub len: usize,
}
impl Bytes {
    pub fn new(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }
    /// # Safety
    /// Nonzero input must point to readable bytes for the whole call.
    pub unsafe fn read<'a>(self) -> Result<&'a [u8], Failure> {
        if self.len > crate::MAX_BYTES || (self.len > 0 && self.ptr.is_null()) {
            return Err(Failure::new(
                Code::SchemaRejected,
                "invalid borrowed buffer",
            ));
        }
        if self.len == 0 {
            Ok(&[])
        } else {
            Ok(unsafe { std::slice::from_raw_parts(self.ptr, self.len) })
        }
    }
}
#[repr(C)]
pub struct Reply {
    pub status: u32,
    pub ptr: *mut u8,
    pub len: usize,
    // Nullable representation permits checking a malformed foreign reply
    // before constructing or calling an invalid Rust function pointer.
    pub release: Option<unsafe extern "C" fn(*mut u8, usize)>,
}
unsafe extern "C" fn release(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}
impl Reply {
    pub fn value(status: u32, value: Value) -> Self {
        match value.encode() {
            Ok(x) => Self::bytes(status, x),
            Err(e) => Self::failure(e),
        }
    }
    pub fn state_value(status: u32, value: Value) -> Self {
        match value.encode_profile(WireProfile::V2) {
            Ok(bytes) => Self::bytes(status, bytes),
            Err(error) => Self::bytes(
                2,
                error
                    .to_value()
                    .encode_profile(WireProfile::V2)
                    .unwrap_or_else(|_| vec![0x50, 0x56, 2, 0, 0]),
            ),
        }
    }
    /// # Safety
    /// Producer buffer and release requirements are the same as consume.
    pub unsafe fn consume_state(self) -> Result<(u32, Value), Failure> {
        let (status, value) = unsafe { self.consume_profile(5, WireProfile::V2) }?;
        if (status == 3 || status == 4) && value != Value::Null {
            return Err(Failure::new(
                Code::SchemaRejected,
                "pending/cancelled state reply must be Null",
            ));
        }
        Ok((status, value))
    }
    fn bytes(status: u32, bytes: Vec<u8>) -> Self {
        let mut bytes = bytes.into_boxed_slice();
        let result = Self {
            status,
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            release: Some(release),
        };
        std::mem::forget(bytes);
        result
    }
    pub fn failure(f: Failure) -> Self {
        Self::bytes(2, f.to_value().encode().unwrap_or_else(|_| vec![0]))
    }
    /// # Safety
    /// Non-null buffers and callbacks must belong to the trusted ABI producer.
    /// A null callback rejects before reading the buffer; it cannot be released.
    pub unsafe fn consume(self) -> Result<(u32, Value), Failure> {
        unsafe { self.consume_profile(2, WireProfile::V1) }
    }
    /// # Safety
    /// Same producer-owned pointer and callback requirements as consume.
    pub unsafe fn consume_pending(self) -> Result<(u32, Value), Failure> {
        unsafe { self.consume_profile(5, WireProfile::V1) }
    }
    unsafe fn consume_profile(
        self,
        maximum: u32,
        profile: WireProfile,
    ) -> Result<(u32, Value), Failure> {
        let release = self
            .release
            .ok_or_else(|| Failure::new(Code::AbiIncompatible, "null reply release callback"))?;
        let decoded = unsafe {
            Bytes {
                ptr: self.ptr,
                len: self.len,
            }
            .read()
        }
        .and_then(|bytes| Value::decode_profile(bytes, profile));
        unsafe { release(self.ptr, self.len) };
        if self.status > maximum {
            return Err(Failure::new(Code::AbiIncompatible, "unknown reply status"));
        }
        decoded.map(|v| (self.status, v))
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Header {
    pub major: u32,
    pub minor: u32,
    pub table_bytes: u32,
    pub features: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostApi {
    pub scope: u64,
    pub invoke: unsafe extern "C" fn(u64, Bytes, Bytes) -> Reply,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginTable {
    pub header: Header,
    pub create: unsafe extern "C" fn(*mut *mut c_void) -> Reply,
    pub invoke: unsafe extern "C" fn(*mut c_void, *const HostApi, Bytes, Bytes) -> Reply,
    pub stop: unsafe extern "C" fn(*mut c_void) -> Reply,
}
pub fn boundary(f: impl FnOnce() -> Reply) -> Reply {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Reply::failure(Failure::new(Code::PluginFailed, "caught plugin panic")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RELEASES: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn counted_release(ptr: *mut u8, len: usize) {
        RELEASES.fetch_add(1, Ordering::SeqCst);
        unsafe { release(ptr, len) }
    }
    #[test]
    fn release_success_domain_and_malformed_failure() {
        RELEASES.store(0, Ordering::SeqCst);
        for status in 0..3 {
            let mut r = Reply::value(status, Value::String("owned".into()));
            r.release = Some(counted_release);
            assert_eq!(
                unsafe { r.consume() }.unwrap(),
                (status, Value::String("owned".into()))
            );
        }
        let mut r = Reply::bytes(0, vec![255]);
        r.release = Some(counted_release);
        assert!(unsafe { r.consume() }.is_err());
        let mut r = Reply::value(99, Value::Null);
        r.release = Some(counted_release);
        assert_eq!(
            unsafe { r.consume() }.unwrap_err().code,
            Code::AbiIncompatible
        );
        assert_eq!(RELEASES.load(Ordering::SeqCst), 5);
        for status in 3..=5 {
            let mut reply = Reply::value(status, Value::Null);
            reply.release = Some(counted_release);
            assert_eq!(
                unsafe { reply.consume_pending() }.unwrap(),
                (status, Value::Null)
            );
        }
        let mut reply = Reply::value(6, Value::Null);
        reply.release = Some(counted_release);
        assert_eq!(
            unsafe { reply.consume_pending() }.unwrap_err().code,
            Code::AbiIncompatible
        );
        assert_eq!(RELEASES.load(Ordering::SeqCst), 9);
    }
    #[test]
    fn null_release_rejects_before_reading_foreign_buffer() {
        let r = Reply {
            status: 0,
            ptr: std::ptr::null_mut(),
            len: 1,
            release: None,
        };
        assert_eq!(
            unsafe { r.consume() }.unwrap_err().code,
            Code::AbiIncompatible
        );
    }
    #[test]
    fn repr_c_layout() {
        assert_eq!(std::mem::size_of::<Header>(), 16);
        assert_eq!(std::mem::size_of::<Bytes>(), 16);
        assert_eq!(std::mem::size_of::<Reply>(), 32);
        assert_eq!(std::mem::offset_of!(Reply, ptr), 8);
        assert_eq!(std::mem::size_of::<HostApi>(), 16);
        assert_eq!(std::mem::size_of::<PluginTable>(), 40);
        assert_eq!(std::mem::size_of::<PendingHostApi>(), 40);
        assert_eq!(std::mem::size_of::<PendingPluginTable>(), 56);
        assert_eq!(std::mem::offset_of!(PendingHostApi, ticket), 16);
        assert_eq!(std::mem::offset_of!(PendingPluginTable, poll), 40);
    }
}

/// Versioned pending host. Only wake may outlive a synchronous scope.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PendingHostApi {
    pub host: HostApi,
    pub ticket: u64,
    pub permit: u64,
    pub wake: unsafe extern "C" fn(u64, u64) -> u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PendingPluginTable {
    pub header: Header,
    pub create: unsafe extern "C" fn(*mut *mut c_void) -> Reply,
    pub invoke: unsafe extern "C" fn(*mut c_void, *const PendingHostApi, Bytes, Bytes) -> Reply,
    pub stop: unsafe extern "C" fn(*mut c_void) -> Reply,
    pub poll: unsafe extern "C" fn(*mut c_void, *const PendingHostApi, Bytes) -> Reply,
    pub cancel: unsafe extern "C" fn(*mut c_void, *const PendingHostApi, Bytes) -> Reply,
}

/// ABI 3.0. Owned replies select value v2; no M0 table is extended in place.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StateHostApi {
    pub header: Header,
    pub scope: u64,
    pub ticket: u64,
    pub permit: u64,
    pub invoke: Option<unsafe extern "C" fn(u64, Bytes, Bytes, Bytes) -> Reply>,
    pub state: Option<unsafe extern "C" fn(u64, u32, Bytes) -> Reply>,
    pub wake: Option<unsafe extern "C" fn(u64, u64) -> u32>,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatePluginTable {
    pub header: Header,
    pub create: Option<unsafe extern "C" fn(*mut *mut c_void) -> Reply>,
    pub initialize: Option<unsafe extern "C" fn(*mut c_void, *const StateHostApi) -> Reply>,
    pub invoke:
        Option<unsafe extern "C" fn(*mut c_void, *const StateHostApi, Bytes, Bytes) -> Reply>,
    pub stop: Option<unsafe extern "C" fn(*mut c_void) -> Reply>,
    pub poll: Option<unsafe extern "C" fn(*mut c_void, *const StateHostApi, Bytes) -> Reply>,
    pub cancel: Option<unsafe extern "C" fn(*mut c_void, *const StateHostApi, Bytes) -> Reply>,
}
impl Header {
    pub fn state(table_bytes: usize) -> Self {
        Self {
            major: 3,
            minor: 0,
            table_bytes: table_bytes as u32,
            features: 3,
        }
    }
    fn validate_state(&self, bytes: usize) -> Result<(), Failure> {
        if self.major != 3
            || self.minor != 0
            || self.features != 3
            || self.table_bytes as usize != bytes
        {
            Err(Failure::new(
                Code::AbiIncompatible,
                "state table header mismatch",
            ))
        } else {
            Ok(())
        }
    }
}
impl StatePluginTable {
    /// # Safety
    /// Header must point to valid aligned producer memory. A compatible header
    /// attests the full table is readable. Callbacks belong to trusted code.
    pub unsafe fn negotiate(header: *const Header) -> Result<Self, Failure> {
        if header.is_null() {
            return Err(Failure::new(Code::AbiIncompatible, "null state table"));
        }
        let h = unsafe { *header };
        h.validate_state(std::mem::size_of::<Self>())?;
        let table = unsafe { *header.cast::<Self>() };
        if table.create.is_none()
            || table.initialize.is_none()
            || table.invoke.is_none()
            || table.stop.is_none()
            || table.poll.is_none()
            || table.cancel.is_none()
        {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "null state plugin callback",
            ));
        }
        Ok(table)
    }
}
impl StateHostApi {
    /// # Safety
    /// Header must be aligned/readable; a compatible header attests a complete
    /// borrowed host table. Its callbacks and scope expire on native return.
    pub unsafe fn negotiate(header: *const Header) -> Result<Self, Failure> {
        if header.is_null() {
            return Err(Failure::new(Code::AbiIncompatible, "null state host"));
        }
        let h = unsafe { *header };
        h.validate_state(std::mem::size_of::<Self>())?;
        let host = unsafe { *header.cast::<Self>() };
        if host.invoke.is_none() || host.state.is_none() || host.wake.is_none() || host.scope == 0 {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "invalid state host callback or scope",
            ));
        }
        Ok(host)
    }
}
