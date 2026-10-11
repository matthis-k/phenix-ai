use libloading::Library;
use phenix_artifact::VerifiedPackage;
use phenix_contract::{
    abi::{Bytes, Header, HostApi, PendingHostApi, PendingPluginTable, PluginTable, Reply},
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
#[repr(C)]
struct RawPendingTable {
    header: Header,
    create: Option<unsafe extern "C" fn(*mut *mut c_void) -> Reply>,
    invoke: Option<unsafe extern "C" fn(*mut c_void, *const PendingHostApi, Bytes, Bytes) -> Reply>,
    stop: Option<unsafe extern "C" fn(*mut c_void) -> Reply>,
    poll: Option<unsafe extern "C" fn(*mut c_void, *const PendingHostApi, Bytes) -> Reply>,
    cancel: Option<unsafe extern "C" fn(*mut c_void, *const PendingHostApi, Bytes) -> Reply>,
}
enum Table {
    Sync(PluginTable),
    Pending(PendingPluginTable),
}
impl Table {
    unsafe fn create(&self, out: *mut *mut c_void) -> Reply {
        unsafe {
            match self {
                Self::Sync(t) => (t.create)(out),
                Self::Pending(t) => (t.create)(out),
            }
        }
    }
    unsafe fn stop(&self, instance: *mut c_void) -> Reply {
        unsafe {
            match self {
                Self::Sync(t) => (t.stop)(instance),
                Self::Pending(t) => (t.stop)(instance),
            }
        }
    }
}
pub(crate) struct Native {
    table: Table,
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
        let entry = unsafe {
            image.get::<unsafe extern "C" fn() -> *const Header>(
                if package.descriptor().abi_major == 2 {
                    b"phenix_native_v2\0"
                } else {
                    b"phenix_native_v1\0"
                },
            )
        }
        .map_err(|e| Failure::new(Code::AbiIncompatible, e.to_string()))?;
        let ptr = unsafe { entry() };
        if ptr.is_null() || !(ptr as usize).is_multiple_of(std::mem::align_of::<RawTable>()) {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "invalid ABI header pointer",
            ));
        }
        let header = unsafe { *ptr };
        if header.major != package.descriptor().abi_major
            || header.minor != 0
            || header.features != package.descriptor().features
            || header.table_bytes
                < (if header.major == 2 {
                    std::mem::size_of::<RawPendingTable>()
                } else {
                    std::mem::size_of::<RawTable>()
                }) as u32
        {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "invalid loaded ABI header",
            ));
        }
        let table = if header.major == 2 {
            let raw = unsafe { &*ptr.cast::<RawPendingTable>() };
            let missing = || Failure::new(Code::AbiIncompatible, "null pending table entry");
            Table::Pending(PendingPluginTable {
                header,
                create: raw.create.ok_or_else(missing)?,
                invoke: raw.invoke.ok_or_else(missing)?,
                stop: raw.stop.ok_or_else(missing)?,
                poll: raw.poll.ok_or_else(missing)?,
                cancel: raw.cancel.ok_or_else(missing)?,
            })
        } else {
            let raw = unsafe { &*ptr.cast::<RawTable>() };
            Table::Sync(PluginTable {
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
            })
        };
        let mut instance = std::ptr::null_mut();
        let result = unsafe { table.create(&mut instance).consume() };
        let successful = matches!(&result, Ok((0, Value::Null))) && !instance.is_null();
        if !successful {
            if !instance.is_null() {
                let _ = unsafe { table.stop(instance).consume() };
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
        if self.instance.is_null() {
            return Err(Failure::new(Code::PluginFailed, "instance already stopped"));
        }
        let Table::Sync(table) = self.table else {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "pending provider requires a ticket",
            ));
        };
        unsafe {
            (table.invoke)(
                self.instance,
                host,
                Bytes::new(key.as_bytes()),
                Bytes::new(input),
            )
            .consume()
        }
    }
    pub(crate) fn pending(
        &self,
        host: &PendingHostApi,
        key: &str,
        input: Option<&[u8]>,
        cancel: bool,
    ) -> Result<(u32, Value), Failure> {
        let Table::Pending(table) = self.table else {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "provider has no pending ABI",
            ));
        };
        if self.instance.is_null() {
            return Err(Failure::new(Code::PluginFailed, "instance already stopped"));
        }
        unsafe {
            let key = Bytes::new(key.as_bytes());
            let reply = match input {
                Some(input) => (table.invoke)(self.instance, host, key, Bytes::new(input)),
                None if cancel => (table.cancel)(self.instance, host, key),
                None => (table.poll)(self.instance, host, key),
            };
            reply.consume_pending()
        }
    }
    pub(crate) fn stop(&mut self) -> Result<(), Failure> {
        let instance = std::mem::replace(&mut self.instance, std::ptr::null_mut());
        if instance.is_null() {
            return Err(Failure::new(Code::PluginFailed, "instance already stopped"));
        }
        match unsafe { self.table.stop(instance).consume() }? {
            (0, Value::Null) => Ok(()),
            (2, value) => Err(Failure::from_value(value)?),
            _ => Err(Failure::new(Code::AbiIncompatible, "invalid stop result")),
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        if !self.instance.is_null() {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static STOPS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn stop(instance: *mut c_void) -> Reply {
        STOPS.fetch_add(1, Ordering::SeqCst);
        drop(unsafe { Box::from_raw(instance.cast::<u8>()) });
        Reply::value(0, Value::Null)
    }
    unsafe extern "C" fn create(_: *mut *mut c_void) -> Reply {
        unreachable!()
    }
    unsafe extern "C" fn invoke(_: *mut c_void, _: *const HostApi, _: Bytes, _: Bytes) -> Reply {
        unreachable!()
    }
    #[test]
    fn repeated_stop_rejects_without_reentering_producer() {
        STOPS.store(0, Ordering::SeqCst);
        let mut native = Native {
            table: Table::Sync(PluginTable {
                header: Header {
                    major: 1,
                    minor: 0,
                    table_bytes: 40,
                    features: 0,
                },
                create,
                invoke,
                stop,
            }),
            instance: Box::into_raw(Box::new(0u8)).cast(),
            _image: libloading::os::unix::Library::this().into(),
        };
        native.stop().unwrap();
        assert_eq!(native.stop().unwrap_err().code, Code::PluginFailed);
        drop(native);
        assert_eq!(STOPS.load(Ordering::SeqCst), 1);
    }
}
