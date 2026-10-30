//! Actual Phenix ABI-3 plugin: generated native entry points and the real
//! Host::invoke dispatcher. The language runtime is implementation-private.
use std::{
    cell::RefCell,
    ffi::{c_void, CString},
    sync::atomic::{AtomicU64, Ordering},
};

use example_contracts::{Run, Text, Transform, TransformError};
use phenix_sdk::{CallError, CallResult, Code, Failure, Host};
use pyo3::{
    exceptions::{PyPermissionError, PyRuntimeError, PyTypeError},
    prelude::*,
    types::{PyDict, PyModule},
};

static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH_V2: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph.bin"));
thread_local! {
    static CURRENT: RefCell<Option<(u64, *const c_void)>> = const { RefCell::new(None) };
    static HOST_FAILURE: RefCell<Option<CallError<TransformError>>> = const { RefCell::new(None) };
}

struct ScopeGuard(Option<(u64, *const c_void)>);
impl ScopeGuard {
    fn enter(host: &Host<'_>) -> (Self, u64) {
        let id = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
        HOST_FAILURE.with(|slot| slot.borrow_mut().take());
        let old = CURRENT.with(|slot| {
            slot.borrow_mut()
                .replace((id, host as *const Host<'_> as *const c_void))
        });
        (Self(old), id)
    }
}
impl Drop for ScopeGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| *slot.borrow_mut() = self.0);
    }
}

#[pyclass(unsendable)]
struct HostBridge {
    id: u64,
}
#[pymethods]
impl HostBridge {
    fn invoke(
        &self,
        py: Python<'_>,
        operation: &str,
        request: &Bound<'_, PyDict>,
    ) -> PyResult<Py<PyAny>> {
        let current = CURRENT.with(|slot| *slot.borrow());
        let Some((id, host_ptr)) = current else {
            HOST_FAILURE.with(|slot| {
                *slot.borrow_mut() = Some(CallError::Kernel(Failure::new(
                    Code::Denied,
                    "expired Phenix host handle",
                )));
            });
            return Err(PyPermissionError::new_err("expired Phenix host handle"));
        };
        if id != self.id {
            HOST_FAILURE.with(|slot| {
                *slot.borrow_mut() = Some(CallError::Kernel(Failure::new(
                    Code::Denied,
                    "stale Phenix host handle",
                )));
            });
            return Err(PyPermissionError::new_err("stale Phenix host handle"));
        }
        if operation != "example.transform.apply@1/apply" {
            HOST_FAILURE.with(|slot| {
                *slot.borrow_mut() = Some(CallError::Kernel(Failure::new(
                    Code::Denied,
                    "Python requested an undeclared operation",
                )));
            });
            return Err(PyPermissionError::new_err("undeclared operation"));
        }
        let query: String = request
            .get_item("query")?
            .ok_or_else(|| PyTypeError::new_err("missing query"))?
            .extract()?;

        // SAFETY: CURRENT is set only for this thread and synchronous invocation.
        // This pointer never escapes or crosses an await/thread boundary.
        let host = unsafe { &*(host_ptr as *const Host<'_>) };
        match host.invoke::<Transform>("Provider", &Text { text: query }) {
            Ok(value) => {
                let response = PyDict::new(py);
                response.set_item("text", value.text)?;
                Ok(response.into_any().unbind())
            }
            Err(error) => {
                HOST_FAILURE.with(|slot| *slot.borrow_mut() = Some(error));
                Err(PyRuntimeError::new_err(
                    "Phenix selected provider rejected call",
                ))
            }
        }
    }
}

#[phenix_sdk::plugin(
    id = "example.python-host",
    version = "1.0.0",
    define(example_contracts::RunContract),
    define(example_contracts::TransformContract),
    provide(example_contracts::RunContract, slot = "Run"),
    require(
        example_contracts::TransformContract,
        slot = "Provider",
        provider = "example.provider"
    )
)]
mod python_host {
    use super::*;

    #[derive(Default)]
    pub struct State {
        module: Option<Py<PyModule>>,
        object: Option<Py<PyAny>>,
    }

    #[provide(Run)]
    fn run(state: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        if request.text.is_empty() {
            return Err(CallError::Domain(TransformError::Empty));
        }
        let (_guard, id) = ScopeGuard::enter(host);
        let response = Python::with_gil(|py| -> PyResult<String> {
            if state.module.is_none() {
                let source = CString::new(include_str!("../phenix_plugin.py")).unwrap();
                let module = PyModule::from_code(
                    py,
                    source.as_c_str(),
                    c"phenix_plugin.py",
                    c"phenix_plugin",
                )?;
                let class = module.getattr("Search")?;
                let obj = class.call0()?;
                let identity: String = class.getattr("__phenix_plugin__")?.extract()?;
                if identity != "example.python-host" {
                    return Err(PyPermissionError::new_err(
                        "Python plugin identity mismatch",
                    ));
                }
                state.object = Some(obj.unbind());
                state.module = Some(module.unbind());
            }
            let module = state.module.as_ref().unwrap().bind(py);
            let object = state.object.as_ref().unwrap().bind(py);
            let (method, text) = if let Some(text) = request.text.strip_prefix("enrich:") {
                ("enriched_query", text)
            } else if request.text == "stale" {
                ("enriched_query", "stale")
            } else if request.text == "private" {
                ("private", "private")
            } else {
                ("query", request.text.as_str())
            };
            let function = object.getattr(method)?;
            let exposed: bool = function
                .getattr("__func__")?
                .getattr("__phenix_exposed__")
                .and_then(|value| value.extract())
                .unwrap_or(false);
            if !exposed {
                return Err(PyPermissionError::new_err("Python method is not exposed"));
            }
            let query = module.getattr("Query")?.call1((text, 8))?;
            let result = if method == "enriched_query" {
                let bridge = Py::new(py, HostBridge { id })?;
                function.call1((query, bridge))?
            } else {
                function.call1((query,))?
            };
            let matches: Vec<String> = result.getattr("matches")?.extract()?;
            let _calls: u64 = result.getattr("calls")?.extract()?;
            Ok(matches.join(","))
        });
        if let Some(error) = HOST_FAILURE.with(|slot| slot.borrow_mut().take()) {
            return Err(error);
        }
        match response {
            Ok(text) => Ok(Text { text }),
            Err(error) => Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                format!("Python adapter exception: {error}"),
            ))),
        }
    }
}
