use crate::{
    native::Native,
    resolve::{Prepared, Selection},
};
use phenix_contract::{
    abi::{boundary, Bytes, HostApi, Reply},
    Code, Failure, Value,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    thread::{self, ThreadId},
};
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);
#[derive(Clone)]
struct ScopeRecord {
    thread: ThreadId,
    kernel: usize,
    root: u64,
    caller: String,
}
fn scopes() -> &'static Mutex<BTreeMap<u64, ScopeRecord>> {
    static SCOPES: OnceLock<Mutex<BTreeMap<u64, ScopeRecord>>> = OnceLock::new();
    SCOPES.get_or_init(|| Mutex::new(BTreeMap::new()))
}
struct Frame {
    scope: u64,
    provider: String,
}
#[derive(Default)]
struct State {
    active_root: Option<u64>,
    next_root: u64,
    completed_roots: u64,
    calls: u64,
    denied: u64,
    last_failure: Option<Failure>,
    stack: Vec<Frame>,
}
#[derive(Clone, Copy)]
enum Caller<'a> {
    Root,
    Plugin(&'a str),
}
impl Caller<'_> {
    fn name(&self) -> &str {
        match self {
            Self::Root => "root",
            Self::Plugin(id) => id,
        }
    }
}
pub struct Kernel {
    prepared: Prepared,
    images: BTreeMap<String, Native>,
    thread: ThreadId,
    state: RefCell<State>,
    _serial: PhantomData<Rc<()>>,
}
pub struct Root<'a> {
    kernel: &'a Kernel,
    id: u64,
    entry: Selection,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Success(Value),
    Domain(Value),
}
impl Prepared {
    pub fn load(self) -> Result<Kernel, Failure> {
        // Recheck all artifacts before the first constructor can run. Native::load
        // also checks the exact artifact immediately before loading its image.
        for p in self.packages.values() {
            p.recheck()?;
        }
        let mut images = BTreeMap::new();
        for (id, package) in &self.packages {
            images.insert(id.clone(), Native::load(package)?);
        }
        Ok(Kernel {
            prepared: self,
            images,
            thread: thread::current().id(),
            state: RefCell::new(State::default()),
            _serial: PhantomData,
        })
    }
}
impl Kernel {
    pub fn admit_default_root(&self) -> Result<Root<'_>, Failure> {
        self.admit_root(self.prepared.entry.clone())
    }
    /// Trusted control-plane admission. This API is absent from the plugin SDK.
    pub fn admit_root(&self, entry: Selection) -> Result<Root<'_>, Failure> {
        self.check_thread()?;
        self.prepared.check_entry(&entry)?;
        if !self.prepared.grants.contains(&("root".into(), entry.key())) {
            return self.fail(Code::Denied, "root admission lacks explicit grant");
        }
        let mut state = self.state.borrow_mut();
        if state.active_root.is_some() {
            return Err(Failure::new(
                Code::ReentrancyRejected,
                "one serial root is already admitted",
            ));
        }
        state.next_root = state
            .next_root
            .checked_add(1)
            .ok_or_else(|| Failure::new(Code::InfrastructureFailure, "root identity exhausted"))?;
        let id = state.next_root;
        state.active_root = Some(id);
        Ok(Root {
            kernel: self,
            id,
            entry,
        })
    }
    fn check_thread(&self) -> Result<(), Failure> {
        if self.thread != thread::current().id() {
            return Err(Failure::new(
                Code::ScopeRejected,
                "wrong host admission thread",
            ));
        }
        Ok(())
    }
    fn fail<T>(&self, code: Code, message: impl Into<String>) -> Result<T, Failure> {
        let failure = Failure::new(code, message);
        let mut state = self.state.borrow_mut();
        if failure.code == Code::Denied {
            state.denied += 1;
        }
        state.last_failure = Some(failure.clone());
        Err(failure)
    }
    // Root entry and nested imports converge here. This is the only point of
    // effect. It does not perform provider discovery or permit policy mutation.
    fn dispatch(
        &self,
        root: u64,
        caller: Caller<'_>,
        selection: &Selection,
        input: &[u8],
    ) -> Result<Outcome, Failure> {
        self.check_thread()?;
        if self.state.borrow().active_root != Some(root) {
            return self.fail(Code::ScopeRejected, "root is no longer admitted");
        }
        let key = selection.key();
        let package = self
            .prepared
            .packages
            .get(&selection.provider)
            .ok_or_else(|| {
                Failure::new(Code::MissingBinding, "provider absent from frozen snapshot")
            })?;
        if let Caller::Plugin(plugin) = caller {
            let declared = self
                .prepared
                .packages
                .get(plugin)
                .is_some_and(|p| p.descriptor().imports.iter().any(|i| i.operation == key));
            if !declared || self.prepared.bindings.get(&key) != Some(&selection.provider) {
                return self.fail(Code::Denied, "undeclared import or unselected provider");
            }
        }
        let contract = package
            .descriptor()
            .contract(&key)
            .ok_or_else(|| Failure::new(Code::SchemaRejected, "operation schema absent"))?;
        let request = Value::decode(input)?;
        if !contract.request.accepts(&request) {
            return self.fail(
                Code::SchemaRejected,
                "request does not match declared schema",
            );
        }
        let authority = self
            .prepared
            .grants
            .contains(&(caller.name().to_owned(), key.clone()))
            && self
                .prepared
                .grants
                .iter()
                .any(|(_, operation)| operation == &key)
            && package.descriptor().permits(&key)
            && self.state.borrow().stack.iter().all(|f| {
                self.prepared.packages[&f.provider]
                    .descriptor()
                    .permits(&key)
            });
        if !authority {
            return self.fail(
                Code::Denied,
                format!("{} has no effective grant for {key}", caller.name()),
            );
        }
        if self
            .state
            .borrow()
            .stack
            .iter()
            .any(|f| f.provider == selection.provider)
        {
            return self.fail(Code::ReentrancyRejected, "synchronous instance cycle");
        }
        let guard = CallGuard::new(self, root, &selection.provider)?;
        let api = HostApi {
            scope: guard.scope,
            invoke: invoke_import,
        };
        let (status, value) = self.images[&selection.provider].invoke(&api, &key, input)?;
        match status {
            0 if contract.response.accepts(&value) => Ok(Outcome::Success(value)),
            1 if contract.error.accepts(&value) => Ok(Outcome::Domain(value)),
            2 => Err(Failure::from_value(value)?),
            _ => self.fail(
                Code::SchemaRejected,
                "provider returned an undeclared result",
            ),
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        let mut report = self.prepared.inspect();
        let state = self.state.borrow();
        for p in report["plugins"].as_array_mut().into_iter().flatten() {
            p["abi_negotiated"] = true.into();
        }
        report["roots"] = serde_json::json!({"active":state.active_root,"completed":state.completed_roots,"last_root_id":if state.next_root==0{None}else{Some(state.next_root)}});
        report["calls"] = state.calls.into();
        report["denied_operations"] = state.denied.into();
        report["last_failure"] = serde_json::json!(state.last_failure);
        report
    }
}
impl Root<'_> {
    pub fn invoke(&self, request: &Value) -> Result<Outcome, Failure> {
        self.invoke_bytes(&request.encode()?)
    }
    pub fn invoke_bytes(&self, request: &[u8]) -> Result<Outcome, Failure> {
        let result = self
            .kernel
            .dispatch(self.id, Caller::Root, &self.entry, request);
        if let Err(e) = &result {
            self.kernel.state.borrow_mut().last_failure = Some(e.clone());
        }
        result
    }
}
impl Drop for Root<'_> {
    fn drop(&mut self) {
        let mut state = self.kernel.state.borrow_mut();
        state.active_root = None;
        state.completed_roots += 1;
    }
}
struct CallGuard<'a> {
    kernel: &'a Kernel,
    scope: u64,
}
impl<'a> CallGuard<'a> {
    fn new(kernel: &'a Kernel, root: u64, caller: &str) -> Result<Self, Failure> {
        let mut scope = NEXT_SCOPE.load(Ordering::Relaxed);
        loop {
            let next = scope.checked_add(1).ok_or_else(|| {
                Failure::new(Code::InfrastructureFailure, "call identity exhausted")
            })?;
            match NEXT_SCOPE.compare_exchange_weak(
                scope,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => scope = current,
            }
        }
        scopes()
            .lock()
            .map_err(|_| Failure::new(Code::InfrastructureFailure, "scope registry poisoned"))?
            .insert(
                scope,
                ScopeRecord {
                    thread: thread::current().id(),
                    kernel: kernel as *const Kernel as usize,
                    root,
                    caller: caller.into(),
                },
            );
        let mut state = kernel.state.borrow_mut();
        state.calls += 1;
        state.stack.push(Frame {
            scope,
            provider: caller.into(),
        });
        Ok(Self { kernel, scope })
    }
}
impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut scopes) = scopes().lock() {
            scopes.remove(&self.scope);
        }
        self.kernel.state.borrow_mut().stack.pop();
    }
}
unsafe extern "C" fn invoke_import(scope: u64, key: Bytes, input: Bytes) -> Reply {
    boundary(|| {
        let result = (|| -> Result<Reply, Failure> {
            let record = scopes()
                .lock()
                .map_err(|_| Failure::new(Code::InfrastructureFailure, "scope registry poisoned"))?
                .get(&scope)
                .cloned()
                .ok_or_else(|| Failure::new(Code::ScopeRejected, "expired or unknown scope"))?;
            // Authenticate thread before dereferencing the kernel address. No TLS
            // authority, caller-supplied principal, or shared mutable reference.
            if record.thread != thread::current().id() {
                return Err(Failure::new(
                    Code::ScopeRejected,
                    "host import on an unregistered thread",
                ));
            }
            let kernel = unsafe { &*(record.kernel as *const Kernel) };
            if kernel.state.borrow().stack.last().map(|f| f.scope) != Some(scope) {
                return Err(Failure::new(
                    Code::ScopeRejected,
                    "scope is not the current call",
                ));
            }
            let key = std::str::from_utf8(unsafe { key.read() }?)
                .map_err(|_| Failure::new(Code::SchemaRejected, "invalid operation key"))?;
            let Some(provider) = kernel.prepared.bindings.get(key) else {
                return kernel.fail(Code::Denied, "operation is not a selected declared import");
            };
            let (interface, operation) = key
                .split_once('/')
                .ok_or_else(|| Failure::new(Code::SchemaRejected, "invalid operation key"))?;
            let selection = Selection {
                interface: interface.into(),
                operation: operation.into(),
                provider: provider.clone(),
            };
            Ok(
                match kernel.dispatch(
                    record.root,
                    Caller::Plugin(&record.caller),
                    &selection,
                    unsafe { input.read() }?,
                )? {
                    Outcome::Success(v) => Reply::value(0, v),
                    Outcome::Domain(v) => Reply::value(1, v),
                },
            )
        })();
        match result {
            Ok(reply) => reply,
            Err(e) => Reply::failure(e),
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_thread_and_expired_scopes_reject_before_kernel_access() {
        let scope = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
        scopes().lock().unwrap().insert(
            scope,
            ScopeRecord {
                thread: thread::current().id(),
                kernel: usize::MAX,
                root: 1,
                caller: "forged".into(),
            },
        );
        let code = thread::spawn(move || {
            let reply = unsafe { invoke_import(scope, Bytes::new(b"not/read"), Bytes::new(&[])) };
            let (status, value) = unsafe { reply.consume() }.unwrap();
            assert_eq!(status, 2);
            Failure::from_value(value).unwrap().code
        })
        .join()
        .unwrap();
        assert_eq!(code, Code::ScopeRejected);
        scopes().lock().unwrap().remove(&scope);
        let reply = unsafe { invoke_import(scope, Bytes::new(b"not/read"), Bytes::new(&[])) };
        let (_, value) = unsafe { reply.consume() }.unwrap();
        assert_eq!(
            Failure::from_value(value).unwrap().code,
            Code::ScopeRejected
        );
    }
    #[test]
    fn forged_scope_rejects_before_reading_pointers() {
        let reply = unsafe {
            invoke_import(
                u64::MAX,
                Bytes {
                    ptr: std::ptr::null(),
                    len: 8,
                },
                Bytes {
                    ptr: std::ptr::null(),
                    len: 8,
                },
            )
        };
        let (status, value) = unsafe { reply.consume() }.unwrap();
        assert_eq!(status, 2);
        assert_eq!(
            Failure::from_value(value).unwrap().code,
            Code::ScopeRejected
        );
    }
}
