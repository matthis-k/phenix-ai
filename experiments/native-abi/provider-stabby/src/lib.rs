//! Stabby-backed implementation behind the same language-neutral C entry table.
use native_abi_contract::{
    score, ApiHeader, OpaqueObject, PublicApi, Request, Response, ABI_VERSION, SCHEMA_ID,
    STATUS_INVALID, STATUS_OK,
};
use std::ffi::c_void;

#[stabby::stabby(checked)]
pub trait Compute {
    fn compute(&mut self, input: i64) -> i64;
}

#[derive(Default)]
struct PrivateState {
    calls: u64,
}

impl Compute for PrivateState {
    extern "C" fn compute(&mut self, input: i64) -> i64 {
        self.calls += 1;
        input + self.calls as i64
    }
}

unsafe extern "C" fn invoke(context: *mut c_void, request: Request, response: *mut Response) -> u32 {
    if context.is_null() || response.is_null() {
        return STATUS_INVALID;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = unsafe { &mut *context.cast::<PrivateState>() };
        let input = score(request)?;
        let result = state.compute(input);
        Ok::<Response, u32>(Response {
            score: result,
            calls: state.calls,
            status: STATUS_OK,
            reserved: 0,
        })
    }));
    match result {
        Ok(Ok(value)) => {
            unsafe { *response = value };
            STATUS_OK
        }
        _ => STATUS_INVALID,
    }
}

unsafe extern "C" fn destroy(context: *mut c_void) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context.cast::<PrivateState>()) });
    }
}

unsafe extern "C" fn create() -> OpaqueObject {
    OpaqueObject {
        context: Box::into_raw(Box::new(PrivateState::default())).cast(),
        invoke,
        destroy,
    }
}

static API: PublicApi = PublicApi {
    header: ApiHeader {
        version: ABI_VERSION,
        table_bytes: std::mem::size_of::<PublicApi>() as u32,
        schema: SCHEMA_ID,
    },
    create,
};

#[no_mangle]
pub extern "C" fn phenix_native_experiment_v1() -> *const PublicApi {
    &API
}
