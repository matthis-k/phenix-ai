//! Shared public declarations only: no implementation or Rust layout crosses the boundary.
use std::ffi::c_void;

pub const ABI_VERSION: u32 = 1;
pub const SCHEMA_ID: u64 = 0x5048_454e_4958_0001;
pub const MAX_INPUT: usize = 4096;
pub const STATUS_OK: u32 = 0;
pub const STATUS_INVALID: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ByteSlice {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub multiplier: i64,
    pub maximum: u32,
    pub mode: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub query: ByteSlice,
    pub options: Options,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub score: i64,
    pub calls: u64,
    pub status: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApiHeader {
    pub version: u32,
    pub table_bytes: u32,
    pub schema: u64,
}

#[repr(C)]
pub struct OpaqueObject {
    pub context: *mut c_void,
    pub invoke: unsafe extern "C" fn(*mut c_void, Request, *mut Response) -> u32,
    pub destroy: unsafe extern "C" fn(*mut c_void),
}

#[repr(C)]
pub struct PublicApi {
    pub header: ApiHeader,
    pub create: unsafe extern "C" fn() -> OpaqueObject,
}

pub type Entry = unsafe extern "C" fn() -> *const PublicApi;

#[derive(Debug, PartialEq, Eq)]
pub enum CompatibilityError {
    Version,
    TableSize,
    Schema,
}

pub fn validate(header: ApiHeader) -> Result<(), CompatibilityError> {
    if header.version != ABI_VERSION {
        return Err(CompatibilityError::Version);
    }
    if header.table_bytes as usize != std::mem::size_of::<PublicApi>() {
        return Err(CompatibilityError::TableSize);
    }
    if header.schema != SCHEMA_ID {
        return Err(CompatibilityError::Schema);
    }
    Ok(())
}

pub fn score(request: Request) -> Result<i64, u32> {
    if request.query.len > MAX_INPUT || (request.query.len != 0 && request.query.ptr.is_null()) {
        return Err(STATUS_INVALID);
    }
    if request.options.mode != 0 {
        return Err(STATUS_INVALID);
    }
    let bytes = if request.query.len == 0 {
        &[][..]
    } else {
        // SAFETY: the caller guarantees the input is readable for the synchronous call.
        unsafe { std::slice::from_raw_parts(request.query.ptr, request.query.len) }
    };
    let text = std::str::from_utf8(bytes).map_err(|_| STATUS_INVALID)?;
    let word_count = text
        .split_whitespace()
        .count()
        .min(request.options.maximum as usize);
    (word_count as i64)
        .checked_mul(request.options.multiplier)
        .ok_or(STATUS_INVALID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_version_size_and_schema_gate() {
        let valid = ApiHeader {
            version: ABI_VERSION,
            table_bytes: std::mem::size_of::<PublicApi>() as u32,
            schema: SCHEMA_ID,
        };
        assert_eq!(validate(valid), Ok(()));
        assert_eq!(
            validate(ApiHeader {
                version: 2,
                ..valid
            }),
            Err(CompatibilityError::Version)
        );
        assert_eq!(
            validate(ApiHeader {
                table_bytes: 1,
                ..valid
            }),
            Err(CompatibilityError::TableSize)
        );
        assert_eq!(
            validate(ApiHeader { schema: 0, ..valid }),
            Err(CompatibilityError::Schema)
        );
    }

    #[test]
    fn nested_request_is_bounded_and_checked() {
        let words = b"one two three";
        let options = Options {
            multiplier: 5,
            maximum: 2,
            mode: 0,
        };
        assert_eq!(
            score(Request {
                query: ByteSlice {
                    ptr: words.as_ptr(),
                    len: words.len()
                },
                options
            }),
            Ok(10)
        );
        assert_eq!(
            score(Request {
                query: ByteSlice {
                    ptr: std::ptr::null(),
                    len: 1
                },
                options
            }),
            Err(STATUS_INVALID)
        );
    }
}
