use abi::{Bytes, HostApi, Reply};
pub use phenix_contract::*;
pub use phenix_macros::{operation, plugin, Portable};
use std::{marker::PhantomData, rc::Rc};
/// An import host borrowed for one synchronous call. It cannot cross threads.
/// Unsupported authoring forms fail at compile time.
///
/// ```compile_fail
/// use phenix_sdk::Portable;
/// #[derive(Portable)]
/// struct Tuple(String);
/// ```
/// ```compile_fail
/// use phenix_sdk::Portable;
/// #[derive(Portable)]
/// struct Pointer { value: *const u8 }
/// ```
/// ```compile_fail
/// use phenix_sdk::{operation,Portable};
/// #[derive(Portable)] struct Text { text:String }
/// #[derive(Portable)] enum Error { Empty }
/// #[operation(interface="invalid",operation="run")]
/// type Invalid=(Text,Text,Error);
/// ```
/// ```compile_fail
/// #[phenix_sdk::plugin(id="example.invalid",version="1.0.0")]
/// mod invalid { }
/// ```
/// ```compile_fail
/// fn send<T: Send>() {}
/// send::<phenix_sdk::Host<'static>>();
/// ```
/// ```compile_fail
/// use phenix_sdk::{operation,Portable};
/// #[derive(Portable)] struct Text { text:String }
/// #[derive(Portable)] enum Error { Empty }
/// #[operation(interface="example.text@1",operation="apply")]
/// type Transform=(Text,Text,Error);
/// #[phenix_sdk::plugin(id="example.duplicate",version="1.0.0")]
/// mod duplicate {
///     use super::*;
///     #[derive(Default)] pub struct State;
///     #[export(Transform)] fn first(_:&mut State,_:&phenix_sdk::Host<'_>,r:Text)->phenix_sdk::CallResult<Text,Error>{Ok(r)}
///     #[export(Transform)] fn second(_:&mut State,_:&phenix_sdk::Host<'_>,r:Text)->phenix_sdk::CallResult<Text,Error>{Ok(r)}
/// }
/// ```
pub struct Host<'call> {
    api: &'call HostApi,
    _serial: PhantomData<Rc<()>>,
}
impl<'call> Host<'call> {
    /// # Safety
    /// The host API must be issued by the kernel and remain live for this call.
    pub unsafe fn from_api(api: &'call HostApi) -> Self {
        Self {
            api,
            _serial: PhantomData,
        }
    }
    pub fn invoke<O: Operation>(&self, request: &O::Request) -> CallResult<O::Response, O::Error> {
        self.call::<O>(O::DEFINITION.key, request.to_value().encode()?)
    }
    pub fn invoke_scoped<O: Operation>(
        &self,
        scope: &ScopedRequest,
        request: &O::Request,
    ) -> CallResult<O::Response, O::Error> {
        self.call::<O>(
            SCOPED_IMPORT,
            scope.encode(O::DEFINITION.key, request.to_value().encode()?)?,
        )
    }
    /// Current admitted resource identity, never a scope token or capability.
    pub fn resource(&self) -> Result<Option<String>, Failure> {
        let (status, value) = self.raw(RESOURCE_CONTEXT, &Value::Null.encode()?)?;
        match (status, value) {
            (0, Value::Null) => Ok(None),
            (0, Value::String(id)) if identifier(&id) => Ok(Some(id)),
            (2, value) => Err(Failure::from_value(value)?),
            _ => Err(Failure::new(
                Code::AbiIncompatible,
                "invalid resource context reply",
            )),
        }
    }
    fn raw(&self, key: &str, bytes: &[u8]) -> Result<(u32, Value), Failure> {
        let reply = unsafe {
            (self.api.invoke)(
                self.api.scope,
                Bytes::new(key.as_bytes()),
                Bytes::new(bytes),
            )
        };
        unsafe { reply.consume() }
    }
    fn call<O: Operation>(&self, key: &str, bytes: Vec<u8>) -> CallResult<O::Response, O::Error> {
        let (status, value) = self.raw(key, &bytes)?;
        match status {
            0 => Ok(O::Response::from_value(value)?),
            1 => Err(CallError::Domain(O::Error::from_value(value)?)),
            _ => Err(CallError::Kernel(Failure::from_value(value)?)),
        }
    }
}
#[doc(hidden)]
pub fn reply<T: Portable, E: Portable>(result: CallResult<T, E>) -> Reply {
    match result {
        Ok(v) => Reply::value(0, v.to_value()),
        Err(CallError::Domain(e)) => Reply::value(1, e.to_value()),
        Err(CallError::Kernel(e)) => Reply::failure(e),
    }
}
