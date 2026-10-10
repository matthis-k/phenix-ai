#![forbid(unsafe_code)]
//! Stable C-compatible plugin contract. This crate intentionally contains no
//! dynamic loader, allocator bridge, Rust trait objects, or guest runtime.
//!
//! An intrinsic loader must inspect the package descriptor without executing
//! code, check the fixed ABI header *before* loading its full function table,
//! then bind every host callback to a checked, attenuated root scope. Raw
//! pointers below are opaque ABI fields and must never be dereferenced by
//! ordinary plugin authoring code.
use core::ffi::c_void;

/// ABI 1 is incompatible with Rust vtables, SDK struct layouts and unwind.
pub const NATIVE_ABI_MAJOR: u16 = 1;
pub const NATIVE_ABI_MINOR: u16 = 0;

/// Feature bit: one terminal settlement per correlated pending call.
pub const FEATURE_PENDING_CALLS: u64 = 1;
/// Feature bit: a wake notification prompts a host poll, never delivers data.
pub const FEATURE_WAKE_POLL: u64 = 1 << 1;
/// Feature bit: host cancellation is visible during outstanding calls.
pub const FEATURE_HOST_CANCELLATION: u64 = 1 << 2;

/// Common ABI prefix, safely readable independently from the full table.
///
/// `table_bytes` denotes the *entire* exported table, including this header.
/// The loader may not read bytes outside that range. A separately sized table
/// avoids assuming a newer revision has the exact same C struct layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAbiHeader {
    pub major: u16,
    pub minor: u16,
    pub table_bytes: usize,
    pub required_features: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeAbiError {
    UnsupportedMajor { offered: u16, supported: u16 },
    UnsupportedMinor { offered: u16, supported: u16 },
    TruncatedTable { offered: usize, required: usize },
    UnavailableFeatures(u64),
    MissingEntrypoint(&'static str),
    InvalidTicket,
    WrongCorrelation {
        expected: NativeCallTicket,
        observed: NativeCallTicket,
    },
    UnknownCallStatus(u32),
    InvalidTerminalBuffer,
    InvalidPendingBuffer,
}

impl NativeAbiHeader {
    /// Reject incompatible or undersized tables without touching function
    /// pointers, invoking callbacks, or executing plugin discovery code.
    pub fn validate(
        self,
        required_bytes: usize,
        available_features: u64,
    ) -> Result<(), NativeAbiError> {
        if self.major != NATIVE_ABI_MAJOR {
            return Err(NativeAbiError::UnsupportedMajor {
                offered: self.major,
                supported: NATIVE_ABI_MAJOR,
            });
        }
        if self.minor > NATIVE_ABI_MINOR {
            return Err(NativeAbiError::UnsupportedMinor {
                offered: self.minor,
                supported: NATIVE_ABI_MINOR,
            });
        }
        if self.table_bytes < required_bytes {
            return Err(NativeAbiError::TruncatedTable {
                offered: self.table_bytes,
                required: required_bytes,
            });
        }
        let missing = self.required_features & !available_features;
        if missing != 0 {
            return Err(NativeAbiError::UnavailableFeatures(missing));
        }
        Ok(())
    }
}

/// Borrowed bytes belong to the caller and remain valid only during an ABI
/// call. A null pointer is legal only when len is zero.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NativeSlice {
    pub ptr: *const u8,
    pub len: usize,
}

/// Owned provider bytes must be released via the producer's paired callback.
/// The consumer must copy or decode before releasing. Unwinding and Rust
/// allocator ownership never cross this boundary.
#[repr(C)]
pub struct NativeOwnedBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub owner: *mut c_void,
    pub release: Option<unsafe extern "C" fn(*mut c_void, *mut u8, usize)>,
}

impl NativeOwnedBuffer {
    pub fn has_valid_shape(&self) -> bool {
        (self.len == 0 || !self.ptr.is_null()) && self.release.is_some()
    }
}

/// Correlation is scoped to one root and selected generation; no raw pointer
/// or TLS state is used to infer authority.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCallTicket {
    pub root_id: u64,
    pub call_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NativeCallRequest {
    pub ticket: NativeCallTicket,
    pub scope: u64,
    pub input: NativeSlice,
}

/// Status values are integers, not foreign-created Rust enums: unknown tags
/// can be rejected rather than causing invalid discriminants/undefined behavior.
pub const RESULT_READY: u32 = 1;
pub const RESULT_ERROR: u32 = 2;
pub const RESULT_PENDING: u32 = 3;

#[repr(C)]
pub struct NativeCallResult {
    pub status: u32,
    pub ticket: NativeCallTicket,
    pub payload: NativeOwnedBuffer,
}

impl NativeCallResult {
    /// Validate one native callback against the selected root's ticket before
    /// reading its payload, recording a normal outcome or releasing any lease.
    /// Polling `Pending` is not a terminal settlement.
    pub fn validate_for(&self, expected: NativeCallTicket) -> Result<(), NativeAbiError> {
        if expected.root_id == 0 || expected.call_id == 0 {
            return Err(NativeAbiError::InvalidTicket);
        }
        if self.ticket != expected {
            return Err(NativeAbiError::WrongCorrelation {
                expected,
                observed: self.ticket,
            });
        }
        match self.status {
            RESULT_READY | RESULT_ERROR => {
                if self.payload.has_valid_shape() {
                    Ok(())
                } else {
                    Err(NativeAbiError::InvalidTerminalBuffer)
                }
            }
            RESULT_PENDING => {
                if self.payload.ptr.is_null()
                    && self.payload.len == 0
                    && self.payload.release.is_none()
                {
                    Ok(())
                } else {
                    Err(NativeAbiError::InvalidPendingBuffer)
                }
            }
            other => Err(NativeAbiError::UnknownCallStatus(other)),
        }
    }
}

/// The host verifies each scoped operation against selected imports, plugin
/// provenance, authority, and active generation. The `wake` callback carries
/// no response bytes and never grants the plugin any additional capability.
#[repr(C)]
pub struct NativeHostV1 {
    pub header: NativeAbiHeader,
    pub context: *mut c_void,
    pub is_cancelled: Option<unsafe extern "C" fn(*mut c_void, NativeCallTicket) -> u32>,
    pub wake: Option<unsafe extern "C" fn(*mut c_void, NativeCallTicket)>,
    pub invoke_import: Option<
        unsafe extern "C" fn(
            *mut c_void,
            NativeCallTicket,
            NativeSlice,
            NativeSlice,
        ) -> NativeCallResult,
    >,
}

/// Versioned plugin table. The binary exports `phenix_plugin_entry_v1`
/// returning a pointer to a static table. The loader owns table and library
/// residency and may unload only after all roots and pending callbacks settle.
///
/// A `begin` call returns Ready/Error/Pending. Every pending ticket is polled
/// after a host wake; `poll` may return Pending again, and can return a terminal
/// result only once. `cancel` is cooperative; it never releases residency.
#[repr(C)]
pub struct NativePluginV1 {
    pub header: NativeAbiHeader,
    pub context: *mut c_void,
    pub prepare: Option<unsafe extern "C" fn(*mut c_void, *const NativeHostV1, u64) -> u32>,
    pub start: Option<unsafe extern "C" fn(*mut c_void, u64) -> u32>,
    pub begin: Option<
        unsafe extern "C" fn(*mut c_void, NativeCallRequest) -> NativeCallResult,
    >,
    pub poll: Option<unsafe extern "C" fn(*mut c_void, NativeCallTicket) -> NativeCallResult>,
    pub cancel: Option<unsafe extern "C" fn(*mut c_void, NativeCallTicket)>,
    pub stop: Option<unsafe extern "C" fn(*mut c_void, u64) -> u32>,
    pub destroy: Option<unsafe extern "C" fn(*mut c_void, u64)>,
}

pub type NativePluginEntryV1 = unsafe extern "C" fn() -> *const NativePluginV1;

impl NativePluginV1 {
    /// Validate table compatibility and required callback slots. The loader
    /// must first validate a separately copied header before forming `&Self`.
    pub fn validate(&self, supported_features: u64) -> Result<(), NativeAbiError> {
        self.header.validate(core::mem::size_of::<Self>(), supported_features)?;
        for (name, present) in [
            ("prepare", self.prepare.is_some()),
            ("start", self.start.is_some()),
            ("begin", self.begin.is_some()),
            ("poll", self.poll.is_some()),
            ("cancel", self.cancel.is_some()),
            ("stop", self.stop.is_some()),
            ("destroy", self.destroy.is_some()),
        ] {
            if !present {
                return Err(NativeAbiError::MissingEntrypoint(name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_negotiates_without_invoking_any_plugin_code() {
        let header = NativeAbiHeader {
            major: NATIVE_ABI_MAJOR,
            minor: NATIVE_ABI_MINOR,
            table_bytes: core::mem::size_of::<NativePluginV1>(),
            required_features: FEATURE_PENDING_CALLS | FEATURE_WAKE_POLL,
        };
        assert_eq!(
            header.validate(core::mem::size_of::<NativePluginV1>(), u64::MAX),
            Ok(())
        );
        assert!(matches!(
            NativeAbiHeader { major: 2, ..header }.validate(1, u64::MAX),
            Err(NativeAbiError::UnsupportedMajor { .. })
        ));
        assert!(matches!(
            NativeAbiHeader { minor: 1, ..header }.validate(1, u64::MAX),
            Err(NativeAbiError::UnsupportedMinor { .. })
        ));
        assert!(matches!(
            NativeAbiHeader { table_bytes: 1, ..header }.validate(
                core::mem::size_of::<NativePluginV1>(), u64::MAX
            ),
            Err(NativeAbiError::TruncatedTable { .. })
        ));
        assert_eq!(
            header.validate(core::mem::size_of::<NativePluginV1>(), 0),
            Err(NativeAbiError::UnavailableFeatures(
                FEATURE_PENDING_CALLS | FEATURE_WAKE_POLL
            ))
        );
    }

    #[test]
    fn opaque_call_ticket_preserves_root_and_callback_identity() {
        let ticket = NativeCallTicket { root_id: 10, call_id: 5 };
        assert_ne!(ticket, NativeCallTicket { root_id: 11, call_id: 5 });
        assert_ne!(ticket, NativeCallTicket { root_id: 10, call_id: 6 });
    }

    #[test]
    fn native_callback_rejects_foreign_tickets_unknown_tags_and_bad_buffers() {
        unsafe extern "C" fn release(_: *mut c_void, _: *mut u8, _: usize) {}
        let expected = NativeCallTicket { root_id: 42, call_id: 9 };
        let empty = || NativeOwnedBuffer {
            ptr: core::ptr::null_mut(),
            len: 0,
            owner: core::ptr::null_mut(),
            release: Some(release),
        };
        let valid = NativeCallResult {
            ticket: expected,
            status: RESULT_READY,
            payload: empty(),
        };
        assert_eq!(valid.validate_for(expected), Ok(()));
        assert_eq!(
            valid.validate_for(NativeCallTicket { root_id: 43, call_id: 9 }),
            Err(NativeAbiError::WrongCorrelation {
                expected: NativeCallTicket { root_id: 43, call_id: 9 },
                observed: expected,
            })
        );
        assert_eq!(
            valid.validate_for(NativeCallTicket { root_id: 0, call_id: 9 }),
            Err(NativeAbiError::InvalidTicket)
        );
        let unknown = NativeCallResult { status: 999, ..valid };
        assert_eq!(
            unknown.validate_for(expected),
            Err(NativeAbiError::UnknownCallStatus(999))
        );
        let bad_pending = NativeCallResult { status: RESULT_PENDING, ..unknown };
        assert_eq!(
            bad_pending.validate_for(expected),
            Err(NativeAbiError::InvalidPendingBuffer)
        );
        let legal_pending = NativeCallResult {
            status: RESULT_PENDING,
            ticket: expected,
            payload: NativeOwnedBuffer {
                ptr: core::ptr::null_mut(),
                len: 0,
                owner: core::ptr::null_mut(),
                release: None,
            },
        };
        assert_eq!(legal_pending.validate_for(expected), Ok(()));
    }

    #[test]
    fn zero_length_owned_buffers_still_require_release_contract() {
        let invalid = NativeOwnedBuffer {
            ptr: core::ptr::null_mut(),
            len: 0,
            owner: core::ptr::null_mut(),
            release: None,
        };
        assert!(!invalid.has_valid_shape());
    }
}
