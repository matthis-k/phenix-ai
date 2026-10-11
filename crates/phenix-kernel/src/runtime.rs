use crate::{
    native::Native,
    resolve::{Prepared, Selection},
};
use phenix_contract::{
    abi::{boundary, Bytes, HostApi, Reply},
    Authority, Code, Failure, ScopedRequest, Value, RESOURCE_CONTEXT, SCOPED_IMPORT,
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
    authority: Authority,
    resource: Option<String>,
}
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    OfflinePreparation,
    ExecutableAbi,
    Admission,
    Invocation,
    Lifecycle,
}
#[derive(serde::Serialize)]
struct FailureContext {
    caller: String,
    provider: Option<String>,
    operation: Option<String>,
}
#[derive(serde::Serialize)]
struct ObservedFailure {
    #[serde(flatten)]
    failure: Failure,
    phase: Phase,
    context: FailureContext,
}
#[derive(serde::Serialize)]
struct StopFailure {
    provider: String,
    failure: Failure,
}
#[derive(Default)]
struct State {
    active_root: Option<u64>,
    next_root: u64,
    completed_roots: u64,
    calls: u64,
    denied: u64,
    last_failure: Option<ObservedFailure>,
    stack: Vec<Frame>,
    stopped: bool,
    stop_failures: Vec<StopFailure>,
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
    images: RefCell<BTreeMap<String, Native>>,
    thread: ThreadId,
    state: RefCell<State>,
    _serial: PhantomData<Rc<()>>,
}
pub struct Root<'a> {
    kernel: &'a Kernel,
    id: u64,
    entry: Selection,
    scope: ScopedRequest,
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
        for id in &self.startup_order {
            match Native::load(&self.packages[id]) {
                Ok(native) => {
                    images.insert(id.clone(), native);
                }
                Err(failure) => {
                    for acquired in self.startup_order.iter().rev() {
                        if let Some(native) = images.get_mut(acquired) {
                            let _ = native.stop();
                        }
                    }
                    return Err(failure);
                }
            }
        }
        Ok(Kernel {
            prepared: self,
            images: RefCell::new(images),
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
        self.admit_scoped_root(
            entry,
            ScopedRequest {
                authority: self.prepared.authority.clone(),
                resource: self.prepared.resource.clone(),
            },
        )
    }
    /// Trusted admission with an explicitly narrowed authority and resource.
    pub fn admit_scoped_root(
        &self,
        entry: Selection,
        scope: ScopedRequest,
    ) -> Result<Root<'_>, Failure> {
        self.check_thread()?;
        let result = self.admit_root_inner(&entry, scope);
        if let Err(failure) = &result {
            self.observe_failure(
                failure,
                Phase::Admission,
                Self::context(Caller::Root, &entry),
            );
        }
        result
    }
    fn admit_root_inner(
        &self,
        entry: &Selection,
        scope: ScopedRequest,
    ) -> Result<Root<'_>, Failure> {
        if !scope.authority.valid()
            || scope
                .resource
                .as_ref()
                .is_some_and(|id| !phenix_contract::identifier(id))
        {
            return Err(Failure::new(
                Code::SchemaRejected,
                "invalid admission scope",
            ));
        }
        self.prepared.check_entry(entry)?;
        if self.state.borrow().stopped {
            return Err(Failure::new(Code::PluginFailed, "kernel is stopped"));
        }
        if !self
            .prepared
            .grants
            .contains_key(&("root".into(), entry.key()))
        {
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
            entry: entry.clone(),
            scope,
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
        if failure.code == Code::Denied {
            self.state.borrow_mut().denied += 1;
        }
        Err(failure)
    }
    fn context(caller: Caller<'_>, selection: &Selection) -> FailureContext {
        FailureContext {
            caller: caller.name().into(),
            provider: phenix_contract::identifier(&selection.provider)
                .then(|| selection.provider.clone()),
            operation: phenix_contract::operation_key(&selection.key()).then(|| selection.key()),
        }
    }
    fn observe_failure(&self, failure: &Failure, phase: Phase, context: FailureContext) {
        self.state.borrow_mut().last_failure = Some(ObservedFailure {
            failure: failure.clone(),
            phase,
            context,
        });
    }
    fn observe_propagation(&self, failure: &Failure, context: FailureContext) {
        // Preserve a nested observation only while the same failure propagates.
        // A plugin that replaces a handled failure gets its own result context.
        let unchanged = self
            .state
            .borrow()
            .last_failure
            .as_ref()
            .is_some_and(|observed| observed.failure == *failure);
        if !unchanged {
            self.observe_failure(failure, Phase::Invocation, context);
        }
    }
    // Root entry and nested imports converge here. This is the only point of
    // effect. It does not perform provider discovery or permit policy mutation.
    fn dispatch(
        &self,
        root: u64,
        caller: Caller<'_>,
        selection: &Selection,
        input: &[u8],
        restriction: Option<&ScopedRequest>,
    ) -> Result<Outcome, Failure> {
        let result = self.dispatch_inner(root, caller, selection, input, restriction);
        if let Err(failure) = &result {
            self.observe_propagation(failure, Self::context(caller, selection));
        }
        result
    }
    fn dispatch_inner(
        &self,
        root: u64,
        caller: Caller<'_>,
        selection: &Selection,
        input: &[u8],
        restriction: Option<&ScopedRequest>,
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
            if !declared
                || self.prepared.bindings.get(&(plugin.into(), key.clone()))
                    != Some(&selection.provider)
            {
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
        let state = self.state.borrow();
        let mut authority = state
            .stack
            .last()
            .map_or_else(|| self.prepared.authority.clone(), |f| f.authority.clone());
        let inherited_resource = state
            .stack
            .last()
            .map_or_else(|| self.prepared.resource.clone(), |f| f.resource.clone());
        let resource = restriction
            .and_then(|r| r.resource.clone())
            .or_else(|| inherited_resource.clone());
        let switched = inherited_resource
            .as_ref()
            .is_some_and(|id| resource.as_ref() != Some(id));
        if let Some(restriction) = restriction {
            authority = authority.intersect(&restriction.authority);
        }
        let grant = self
            .prepared
            .grants
            .get(&(caller.name().to_owned(), key.clone()));
        let operation_allowed = grant.is_some()
            && authority.operations.contains(&key)
            && package.descriptor().permits(&key)
            && state.stack.iter().all(|f| {
                self.prepared.packages[&f.provider]
                    .descriptor()
                    .permits(&key)
            });
        authority
            .operations
            .retain(|op| package.descriptor().permits(op));
        if let Some(resources) = grant {
            authority.resources = authority
                .resources
                .intersection(resources)
                .cloned()
                .collect();
        }
        let resource_allowed = resource
            .as_ref()
            .is_none_or(|id| authority.resources.contains(id));
        drop(state);
        if !operation_allowed || !resource_allowed || switched {
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
        let guard = CallGuard::new(self, root, &selection.provider, authority, resource)?;
        let api = HostApi {
            scope: guard.scope,
            invoke: invoke_import,
        };
        let (status, value) =
            self.images.borrow()[&selection.provider].invoke(&api, &key, input)?;
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
        if report["format"] == "phenix.inspect.v2" {
            report["lifecycle"] = serde_json::json!({"startup_order":self.prepared.startup_order,"stopped":state.stopped,"stop_failures":state.stop_failures});
        }
        report
    }
    /// Trusted synchronous lifecycle control. No plugin scope exposes this API.
    pub fn shutdown(&self) -> Result<(), Failure> {
        self.check_thread()?;
        if self.state.borrow().active_root.is_some() {
            return Err(Failure::new(
                Code::ReentrancyRejected,
                "shutdown requires all roots released",
            ));
        }
        self.stop_images()
    }
    fn stop_images(&self) -> Result<(), Failure> {
        if self.state.borrow().stopped {
            return Ok(());
        }
        self.state.borrow_mut().stopped = true;
        let mut first_failure = None;
        for id in self.prepared.startup_order.iter().rev() {
            if let Some(mut native) = self.images.borrow_mut().remove(id) {
                if let Err(failure) = native.stop() {
                    if first_failure.is_none() {
                        first_failure = Some(failure.clone());
                    }
                    self.state.borrow_mut().stop_failures.push(StopFailure {
                        provider: id.clone(),
                        failure: failure.clone(),
                    });
                    self.observe_failure(
                        &failure,
                        Phase::Lifecycle,
                        FailureContext {
                            caller: "control".into(),
                            provider: Some(id.clone()),
                            operation: None,
                        },
                    );
                }
            }
        }
        first_failure.map_or(Ok(()), Err)
    }
}
impl Drop for Kernel {
    fn drop(&mut self) {
        let _ = self.stop_images();
    }
}
impl Root<'_> {
    pub fn invoke(&self, request: &Value) -> Result<Outcome, Failure> {
        match request.encode() {
            Ok(bytes) => self.invoke_bytes(&bytes),
            Err(failure) => {
                self.kernel.observe_failure(
                    &failure,
                    Phase::Invocation,
                    Kernel::context(Caller::Root, &self.entry),
                );
                Err(failure)
            }
        }
    }
    pub fn invoke_bytes(&self, request: &[u8]) -> Result<Outcome, Failure> {
        let previous_failure = self.kernel.state.borrow_mut().last_failure.take();
        let result = self.kernel.dispatch(
            self.id,
            Caller::Root,
            &self.entry,
            request,
            Some(&self.scope),
        );
        let mut state = self.kernel.state.borrow_mut();
        if state.last_failure.is_none() {
            state.last_failure = previous_failure;
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
    fn new(
        kernel: &'a Kernel,
        root: u64,
        caller: &str,
        authority: Authority,
        resource: Option<String>,
    ) -> Result<Self, Failure> {
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
            authority,
            resource,
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
            let mut context = FailureContext {
                caller: record.caller.clone(),
                provider: None,
                operation: None,
            };
            let result = (|| -> Result<Reply, Failure> {
                let key = std::str::from_utf8(unsafe { key.read() }?)
                    .map_err(|_| Failure::new(Code::SchemaRejected, "invalid operation key"))?;
                let input = unsafe { input.read() }?;
                if key == RESOURCE_CONTEXT {
                    if Value::decode(input)? != Value::Null {
                        return Err(Failure::new(
                            Code::SchemaRejected,
                            "resource context expects Null",
                        ));
                    }
                    let resource = kernel
                        .state
                        .borrow()
                        .stack
                        .last()
                        .and_then(|f| f.resource.clone());
                    return Ok(Reply::value(0, resource.map_or(Value::Null, Value::String)));
                }
                let scoped = if key == SCOPED_IMPORT {
                    Some(ScopedRequest::decode(input)?)
                } else {
                    None
                };
                let (key, input, restriction) = match &scoped {
                    Some((key, input, scope)) => (key.as_str(), input.as_slice(), Some(scope)),
                    None => (key, input, None),
                };
                context.operation = phenix_contract::operation_key(key).then(|| key.into());
                let Some(provider) = kernel
                    .prepared
                    .bindings
                    .get(&(record.caller.clone(), key.into()))
                else {
                    let optional = kernel.prepared.packages[&record.caller]
                        .descriptor()
                        .imports
                        .iter()
                        .any(|i| i.operation == key && i.optional);
                    return kernel.fail(
                        if optional {
                            Code::MissingBinding
                        } else {
                            Code::Denied
                        },
                        "operation is not a selected declared import",
                    );
                };
                context.provider = Some(provider.clone());
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
                        input,
                        restriction,
                    )? {
                        Outcome::Success(v) => Reply::value(0, v),
                        Outcome::Domain(v) => Reply::value(1, v),
                    },
                )
            })();
            if let Err(failure) = &result {
                kernel.observe_propagation(failure, context);
            }
            result
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
