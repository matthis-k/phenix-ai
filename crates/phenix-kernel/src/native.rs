use libloading::Library;
use phenix_artifact::VerifiedPackage;
use phenix_contract::{
    abi::{Bytes, Header, HostApi, PluginTable, Reply},
    Code, Failure, Value,
};
use std::ffi::c_void;
// Nullable function representations let the loader reject null entries without
// constructing an invalid Rust function pointer. Layout matches the C header.
#[repr(C)]
struct RawTable {
    header: Header,
    create: Option<unsafe extern "C" fn(*mut *mut c_void) -> Reply>,
    invoke: Option<unsafe extern "C" fn(*mut c_void, *const HostApi, Bytes, Bytes) -> Reply>,
    stop: Option<unsafe extern "C" fn(*mut c_void) -> Reply>,
}
pub(crate) struct Native {
    table: PluginTable,
    instance: *mut c_void,
    _image: Library,
}
impl Native {
    pub(crate) fn load(package: &VerifiedPackage) -> Result<Self, Failure> {
        package.recheck()?;
        // The control plane already verified all selected packages and bindings.
        // Constructors are trusted code and may execute here, before negotiation.
        let image = unsafe { Library::new(package.artifact()) }
            .map_err(|e| Failure::new(Code::ArtifactInvalid, e.to_string()))?;
        let entry =
            unsafe { image.get::<unsafe extern "C" fn() -> *const Header>(b"phenix_native_v1\0") }
                .map_err(|e| Failure::new(Code::AbiIncompatible, e.to_string()))?;
        let ptr = unsafe { entry() };
        if ptr.is_null() || !(ptr as usize).is_multiple_of(std::mem::align_of::<RawTable>()) {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "invalid ABI header pointer",
            ));
        }
        let header = unsafe { *ptr };
        if header.major != 1
            || header.minor != 0
            || header.features != 0
            || header.table_bytes < std::mem::size_of::<RawTable>() as u32
        {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "invalid loaded ABI header",
            ));
        }
        let raw = unsafe { &*ptr.cast::<RawTable>() };
        let table = PluginTable {
            header,
            create: raw
                .create
                .ok_or_else(|| Failure::new(Code::AbiIncompatible, "null create entry"))?,
            invoke: raw
                .invoke
                .ok_or_else(|| Failure::new(Code::AbiIncompatible, "null invoke entry"))?,
            stop: raw
                .stop
                .ok_or_else(|| Failure::new(Code::AbiIncompatible, "null stop entry"))?,
        };
        let mut instance = std::ptr::null_mut();
        let result = unsafe { (table.create)(&mut instance).consume() };
        let successful = matches!(&result, Ok((0, Value::Null))) && !instance.is_null();
        if !successful {
            if !instance.is_null() {
                let _ = unsafe { (table.stop)(instance).consume() };
            }
            return Err(match result {
                Ok((2, value)) => Failure::from_value(value)?,
                Err(e) => e,
                _ => Failure::new(Code::PluginFailed, "invalid create result"),
            });
        }
        Ok(Self {
            table,
            instance,
            _image: image,
        })
    }
    pub(crate) fn invoke(
        &self,
        host: &HostApi,
        key: &str,
        input: &[u8],
    ) -> Result<(u32, Value), Failure> {
        unsafe {
            (self.table.invoke)(
                self.instance,
                host,
                Bytes::new(key.as_bytes()),
                Bytes::new(input),
            )
            .consume()
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = unsafe { (self.table.stop)(self.instance).consume() };
    }
}
