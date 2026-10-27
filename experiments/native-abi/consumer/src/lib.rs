//! Consumer loads compiled code, never links either provider's implementation crate.
use libloading::Library;
use native_abi_contract::{
    validate, ByteSlice, CompatibilityError, Entry, OpaqueObject, Options, PublicApi, Request,
    Response, STATUS_OK,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);

/// A copied image permits a new build to be loaded while previous instances
/// keep their old library image alive. No hot mutation of existing vtables.
struct Image {
    library: Library,
    staged: PathBuf,
}

impl Drop for Image {
    fn drop(&mut self) {
        // Linux permits unlinking a mapped .so. The experiment targets Linux CI.
        let _ = std::fs::remove_file(&self.staged);
    }
}

pub struct Generation {
    image: Arc<Image>,
    create: unsafe extern "C" fn() -> OpaqueObject,
}

pub struct Instance {
    // Declared before the image so that its FFI destructor runs while loaded.
    object: OpaqueObject,
    image: Arc<Image>,
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: destruction belongs to the same image as creation.
        unsafe { (self.object.destroy)(self.object.context) };
    }
}

impl Generation {
    pub fn open(original: &Path) -> Result<Self, String> {
        let id = NEXT_IMAGE.fetch_add(1, Ordering::Relaxed);
        let filename = format!("phenix-abi-spike-{}-{}.so", std::process::id(), id);
        let staged = std::env::temp_dir().join(filename);
        std::fs::copy(original, &staged).map_err(|e| e.to_string())?;
        let loaded = (|| {
            // SAFETY: this spike only loads trusted local CI artifacts.
            let library = unsafe { Library::new(&staged) }.map_err(|e| e.to_string())?;
            let entry = unsafe {
                library.get::<Entry>(b"phenix_native_experiment_v1\0")
            }
            .map_err(|e| e.to_string())?;
            let table = unsafe { entry() };
            if table.is_null() {
                return Err("null plugin entry".into());
            }
            let header = unsafe { (*table).header };
            validate(header).map_err(|e| format!("incompatible plugin: {e:?}"))?;
            let create = unsafe { (*table).create };
            Ok(Self {
                image: Arc::new(Image { library, staged: staged.clone() }),
                create,
            })
        })();
        if loaded.is_err() {
            let _ = std::fs::remove_file(staged);
        }
        loaded
    }

    pub fn instantiate(&self) -> Instance {
        Instance {
            object: unsafe { (self.create)() },
            image: Arc::clone(&self.image),
        }
    }
}

impl Instance {
    pub fn invoke(&mut self, query: &str, options: Options) -> Result<Response, u32> {
        let request = Request {
            query: ByteSlice { ptr: query.as_ptr(), len: query.len() },
            options,
        };
        let mut response = Response { score: 0, calls: 0, status: 0, reserved: 0 };
        let status = unsafe {
            (self.object.invoke)(self.object.context, request, &mut response)
        };
        if status != STATUS_OK {
            return Err(status);
        }
        Ok(response)
    }
}

pub fn smoke(paths: &[PathBuf]) -> Result<(), String> {
    if paths.len() != 3 {
        return Err("expected two Rust .so providers and one native language adapter".into());
    }
    for path in paths {
        let old = Generation::open(path)?;
        let mut original = old.instantiate();
        let options = Options { multiplier: 7, maximum: 2, mode: 0 };
        let first = original.invoke("alpha beta gamma", options).map_err(|e| e.to_string())?;
        assert_eq!((first.score, first.calls), (15, 1));
        let second = original.invoke("one", options).map_err(|e| e.to_string())?;
        assert_eq!((second.score, second.calls), (9, 2));
        assert_eq!(
            original.invoke("oversized", Options { multiplier: 5, maximum: 1, mode: 99 }),
            Err(native_abi_contract::STATUS_INVALID)
        );

        // A new image can be loaded and selected while old instances remain
        // usable; dropping 'old' must not unload its still-borrowed code.
        let next = Generation::open(path)?;
        let mut newest = next.instantiate();
        drop(old);
        let old_result = original.invoke("a", options).map_err(|e| e.to_string())?;
        assert_eq!((old_result.score, old_result.calls), (10, 3));
        let new_result = newest.invoke("a", options).map_err(|e| e.to_string())?;
        assert_eq!((new_result.score, new_result.calls), (8, 1));
        drop(next);
        drop(original);
        drop(newest);
    }
    Ok(())
}

#[cfg(feature = "hot-reload-adapter")]
pub fn watch_candidate(path: &Path) -> Result<(), String> {
    // Feature-gated compilation probe for hot-lib-reloader. This watcher is
    // deliberately not permitted to swap Phenix production generation handles.
    let stem = path.file_stem().ok_or("library has no stem")?.to_string_lossy();
    let name = stem.strip_prefix("lib").unwrap_or(&stem);
    let _observer = hot_lib_reloader::LibReloader::new(
        path.parent().ok_or("library has no parent")?,
        name,
        None,
        None,
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn reject_mismatches() {
    use native_abi_contract::{ApiHeader, ABI_VERSION, SCHEMA_ID};
    let valid = ApiHeader {
        version: ABI_VERSION,
        table_bytes: std::mem::size_of::<PublicApi>() as u32,
        schema: SCHEMA_ID,
    };
    assert_eq!(validate(ApiHeader { version: 2, ..valid }), Err(CompatibilityError::Version));
    assert_eq!(validate(ApiHeader { schema: SCHEMA_ID + 1, ..valid }), Err(CompatibilityError::Schema));
    assert_eq!(validate(ApiHeader { table_bytes: 0, ..valid }), Err(CompatibilityError::TableSize));
}
