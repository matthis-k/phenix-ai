use crate::{
    native::Native,
    resolve::{EffectTarget, Prepared, Selection},
};
use phenix_contract::{
    abi::{Header, StateHostApi},
    Authority, Code, Failure, ScopedRequest, Value,
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
pub(super) struct ScopeRecord {
    thread: ThreadId,
    pub(super) kernel: usize,
    pub(super) root: u64,
    pub(super) caller: String,
}
fn scopes() -> &'static Mutex<BTreeMap<u64, ScopeRecord>> {
    static SCOPES: OnceLock<Mutex<BTreeMap<u64, ScopeRecord>>> = OnceLock::new();
    SCOPES.get_or_init(|| Mutex::new(BTreeMap::new()))
}
pub(super) struct Frame {
    pub(super) ticket: Option<u64>,
    pub(super) revoked: bool,
    scope: u64,
    pub(super) provider: String,
    pub(super) authority: Authority,
    pub(super) resource: Option<String>,
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
pub(super) struct FailureContext {
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
pub(super) struct State {
    pub(super) tickets: BTreeMap<u64, super::pending::Record>,
    pub(super) root_live: bool,
    pub(super) active_root: Option<u64>,
    next_root: u64,
    pub(super) completed_roots: u64,
    calls: u64,
    denied: u64,
    last_failure: Option<ObservedFailure>,
    pub(super) stack: Vec<Frame>,
    stopped: bool,
    stop_failures: Vec<StopFailure>,
    // Reserved graph lifecycle sources are generation-scoped and one-shot.
    pub(super) lifecycle_begin: bool,
    pub(super) lifecycle_loaded: std::collections::BTreeSet<String>,
    pub(super) lifecycle_unloaded: std::collections::BTreeSet<String>,
    pub(super) lifecycle_reloaded: std::collections::BTreeSet<String>,
    pub(super) lifecycle_shutdown: bool,
}
#[derive(Clone, Copy)]
pub(super) enum Caller<'a> {
    Root,
    Plugin(&'a str, &'a str),
    Graph(&'a str),
}
impl Caller<'_> {
    fn name(&self) -> &str {
        match self {
            Self::Root => "root",
            Self::Plugin(id, _) | Self::Graph(id) => id,
        }
    }
}
pub struct Kernel {
    pub(super) prepared: Prepared,
    pub(super) images: RefCell<BTreeMap<String, Native>>,
    thread: ThreadId,
    pub(super) state: RefCell<State>,
    pub(super) cells: RefCell<super::value_state::Cells>,
    pub(super) initializing: RefCell<Option<String>>,
    _serial: PhantomData<Rc<()>>,
}
pub(super) enum KernelLease<'a> {
    Borrowed(&'a Kernel),
    Pinned(Rc<Kernel>),
}
impl std::ops::Deref for KernelLease<'_> {
    type Target = Kernel;
    fn deref(&self) -> &Kernel {
        match self {
            Self::Borrowed(k) => k,
            Self::Pinned(k) => k,
        }
    }
}
pub struct Root<'a> {
    pub(super) kernel: KernelLease<'a>,
    owns_admission: bool,
    generation: Option<String>,
    pub(super) id: u64,
    pub(super) entry: Selection,
    pub(super) scope: ScopedRequest,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Success(Value),
    Domain(Value),
}
impl Prepared {
    pub fn load(self) -> Result<Kernel, Failure> {
        let identity = self.generation_identity("standalone")?;
        self.load_generation(identity.as_str())
    }
    pub(crate) fn load_generation(self, identity: &str) -> Result<Kernel, Failure> {
        // Recheck all artifacts before the first constructor can run. Native::load
        // also checks the exact artifact immediately before loading its image.
        for p in self.packages.values() {
            p.recheck()?;
        }
        let digest = super::value_state::digest_bytes(identity);
        let mut cells = super::value_state::Cells::new(&digest)?;
        for (id, p) in &self.packages {
            let _ = p;
            cells.register_owner(id)?;
        }
        let kernel = Kernel {
            prepared: self,
            images: RefCell::new(BTreeMap::new()),
            thread: thread::current().id(),
            state: RefCell::new(State::default()),
            cells: RefCell::new(cells),
            initializing: RefCell::new(None),
            _serial: PhantomData,
        };
        for id in &kernel.prepared.startup_order {
            let native = Native::load(&kernel.prepared.packages[id])?;
            kernel.images.borrow_mut().insert(id.clone(), native);
            let metadata = kernel.prepared.packages[id].metadata();
            *kernel.initializing.borrow_mut() = Some(id.clone());
            let guard = CallGuard::new(&kernel, 0, id, Authority::default(), None, false)?;
            let initialized = kernel.images.borrow()[id].initialize(guard.scope);
            drop(guard);
            *kernel.initializing.borrow_mut() = None;
            initialized?;
            if !kernel
                .cells
                .borrow()
                .initialized(id, metadata.properties.len())
            {
                return Err(Failure::new(
                    Code::PluginFailed,
                    "initialization did not create all declared properties",
                ));
            }
        }
        Ok(kernel)
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
        state.root_live = true;
        Ok(Root {
            kernel: KernelLease::Borrowed(self),
            owns_admission: true,
            generation: None,
            id,
            entry: entry.clone(),
            scope,
        })
    }
    pub(super) fn check_thread(&self) -> Result<(), Failure> {
        if self.thread != thread::current().id() {
            return Err(Failure::new(
                Code::ScopeRejected,
                "wrong host admission thread",
            ));
        }
        Ok(())
    }
    pub(super) fn fail<T>(&self, code: Code, message: impl Into<String>) -> Result<T, Failure> {
        let failure = Failure::new(code, message);
        if failure.code == Code::Denied {
            self.state.borrow_mut().denied += 1;
        }
        Err(failure)
    }
    pub(super) fn context(caller: Caller<'_>, selection: &Selection) -> FailureContext {
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
    pub(super) fn observe_import_failure(
        &self,
        caller: &str,
        key: Option<&str>,
        provider: Option<&str>,
        failure: &Failure,
    ) {
        self.observe_propagation(
            failure,
            FailureContext {
                caller: caller.into(),
                provider: provider.map(str::to_owned),
                operation: key.map(str::to_owned),
            },
        );
    }
    pub(super) fn observe_propagation(&self, failure: &Failure, context: FailureContext) {
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
        let result = self
            .dispatch_call(root, caller, selection, input, restriction, false)
            .map(|call| match call {
                super::pending::Call::Ready(outcome) => outcome,
                super::pending::Call::Pending(_) => {
                    unreachable!("synchronous pending entry rejected")
                }
            });
        if let Err(failure) = &result {
            self.observe_propagation(failure, Self::context(caller, selection));
        }
        result
    }
    pub(super) fn dispatch_call(
        &self,
        root: u64,
        caller: Caller<'_>,
        selection: &Selection,
        input: &[u8],
        restriction: Option<&ScopedRequest>,
        pending_allowed: bool,
    ) -> Result<super::pending::Call, Failure> {
        self.check_thread()?;
        if self.state.borrow().active_root != Some(root) {
            return self.fail(Code::ScopeRejected, "root is no longer admitted");
        }
        let key = selection.key();
        if self.state.borrow().stack.last().is_some_and(|f| f.revoked) {
            return self.fail(
                Code::ScopeRejected,
                "cancelled call has no host effect authority",
            );
        }
        if let Caller::Plugin(plugin, import_slot) = caller {
            if self.prepared.bindings.get(&(
                plugin.into(),
                phenix_contract::import_identity(import_slot, &key),
            )) != Some(&selection.provider)
            {
                return self.fail(Code::Denied, "undeclared import or unselected provider");
            }
        }
        let operation = self.prepared.operation(selection)?;
        let target = &operation.target;
        let contract = &operation.schema;
        let request = Value::decode(input)?;
        if !contract.request.accepts(&request) {
            return self.fail(
                Code::SchemaRejected,
                "request does not match declared schema",
            );
        }
        let ScopedRequest {
            authority,
            resource,
        } = self.effect_scope(caller.name(), &selection.provider, &key, restriction)?;
        if let EffectTarget::Cell {
            property,
            operation,
        } = target
        {
            self.require_property_resource(selection, &authority)?;
            let outcome = self.cell_dispatch(
                root,
                caller.name(),
                selection,
                property,
                *operation,
                &request,
            )?;
            return match &outcome {
                Outcome::Success(value) if contract.response.accepts(value) => {
                    Ok(super::pending::Call::Ready(outcome))
                }
                Outcome::Domain(value) if contract.error.accepts(value) => {
                    Ok(super::pending::Call::Ready(outcome))
                }
                _ => self.fail(Code::SchemaRejected, "cell returned an undeclared result"),
            };
        }
        let pending = pending_allowed;
        if self
            .state
            .borrow()
            .tickets
            .values()
            .any(|r| !r.quiescent && r.selection.provider == selection.provider)
        {
            return self.fail(
                Code::ReentrancyRejected,
                "instance has a physical pending call",
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
        let guard = CallGuard::new(
            self,
            root,
            &selection.provider,
            authority.clone(),
            resource.clone(),
            false,
        )?;
        if pending {
            return self.begin_pending(
                root,
                selection,
                match caller {
                    Caller::Plugin(_, slot) => slot,
                    Caller::Root | Caller::Graph(_) => "",
                },
                input,
                &guard,
                ScopedRequest {
                    authority,
                    resource,
                },
            );
        }
        let (status, value) =
            self.images.borrow()[&selection.provider].invoke(guard.scope, &key, input)?;
        match status {
            0 if contract.response.accepts(&value) => {
                Ok(super::pending::Call::Ready(Outcome::Success(value)))
            }
            1 if contract.error.accepts(&value) => {
                Ok(super::pending::Call::Ready(Outcome::Domain(value)))
            }
            2 => Err(Failure::from_value(value)?),
            5 => Err(Failure::from_value(value)?),
            _ => self.fail(
                Code::SchemaRejected,
                "provider returned an undeclared result",
            ),
        }
    }
    /// The single operation/resource authority check for all runtime effects.
    pub(super) fn effect_scope(
        &self,
        caller: &str,
        provider: &str,
        key: &str,
        restriction: Option<&ScopedRequest>,
    ) -> Result<ScopedRequest, Failure> {
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
            .get(&(caller.to_owned(), key.to_owned()));
        let operation_allowed = grant.is_some()
            && authority.operations.contains(key)
            && self.prepared.permits(provider, key)
            && state
                .stack
                .iter()
                .all(|f| self.prepared.permits(&f.provider, key));
        authority
            .operations
            .retain(|op| self.prepared.permits(provider, op));
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
                format!("{} has no effective grant for {key}", caller),
            );
        }
        Ok(ScopedRequest {
            authority,
            resource,
        })
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
        drop(state);
        if !self.prepared.state_identity.is_empty() {
            report["live_state"] = self.cells.borrow().inspect();
        }
        {
            report["pending"] = self.pending_inspection();
        }
        report
    }
    /// Trusted synchronous lifecycle control. No plugin scope exposes this API.
    pub fn shutdown(&self) -> Result<(), Failure> {
        self.check_thread()?;
        if self.state.borrow().tickets.values().any(|r| !r.quiescent) {
            return Err(Failure::new(
                Code::ScopeRejected,
                format!(
                    "physical leases block shutdown: {}",
                    self.pending_inspection()
                ),
            ));
        }
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
        self.cells.borrow_mut().clear();
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
        self.invalidate_permits();
        if self.state.borrow().tickets.values().any(|r| !r.quiescent) {
            // Noncooperative native work may still execute. Retain the whole
            // selected dependency closure; never stop or unload live instances.
            for (_, native) in std::mem::take(self.images.get_mut()) {
                std::mem::forget(native);
            }
        } else {
            let _ = self.stop_images();
        }
    }
}
impl Root<'_> {
    pub(super) fn pin(mut self, kernel: Rc<Kernel>, identity: &str) -> Root<'static> {
        assert!(std::ptr::eq(&*self.kernel, &*kernel));
        self.owns_admission = false;
        Root {
            kernel: KernelLease::Pinned(kernel),
            owns_admission: true,
            generation: Some(identity.into()),
            id: self.id,
            entry: self.entry.clone(),
            scope: self.scope.clone(),
        }
    }
    pub fn generation_identity(&self) -> Option<&str> {
        self.generation.as_deref()
    }
    pub fn invoke(&self, request: &Value) -> Result<Outcome, Failure> {
        self.invoke_selected(&self.entry, request)
    }
    /// Trusted graph execution inside the admitted root. The graph compiler
    /// has already authenticated the declaring owner, target and grants.
    /// Dispatch still applies the owner's grant and the root's exact scope.
    pub(super) fn call_graph(
        &self,
        owner: &str,
        selection: &Selection,
        request: &Value,
        pending_allowed: bool,
    ) -> Result<super::pending::Call, Failure> {
        self.kernel.prepared.check_entry(selection)?;
        let result = self.kernel.dispatch_call(
            self.id,
            Caller::Graph(owner),
            selection,
            &request.encode()?,
            Some(&self.scope),
            pending_allowed,
        );
        if let Err(failure) = &result {
            self.kernel
                .observe_propagation(failure, Kernel::context(Caller::Graph(owner), selection));
        }
        result
    }
    /// Trusted control-plane selection within this root's pinned generation.
    /// The selected call inherits this root's exact authority and resource.
    pub fn invoke_selected(
        &self,
        selection: &Selection,
        request: &Value,
    ) -> Result<Outcome, Failure> {
        let encoded = self
            .kernel
            .prepared
            .check_entry(selection)
            .and_then(|()| request.encode());
        match encoded {
            Ok(bytes) => self.invoke_selected_bytes(selection, &bytes),
            Err(failure) => {
                self.kernel.observe_failure(
                    &failure,
                    Phase::Invocation,
                    Kernel::context(Caller::Root, selection),
                );
                Err(failure)
            }
        }
    }
    pub fn invoke_bytes(&self, request: &[u8]) -> Result<Outcome, Failure> {
        self.invoke_selected_bytes(&self.entry, request)
    }
    fn invoke_selected_bytes(
        &self,
        selection: &Selection,
        request: &[u8],
    ) -> Result<Outcome, Failure> {
        let previous_failure = self.kernel.state.borrow_mut().last_failure.take();
        let result =
            self.kernel
                .dispatch(self.id, Caller::Root, selection, request, Some(&self.scope));
        let mut state = self.kernel.state.borrow_mut();
        if state.last_failure.is_none() {
            state.last_failure = previous_failure;
        }
        result
    }
}
impl Drop for Root<'_> {
    fn drop(&mut self) {
        if !self.owns_admission {
            return;
        }
        self.kernel.state.borrow_mut().root_live = false;
        self.kernel.abandon_root(self.id);
    }
}
pub(super) struct CallGuard<'a> {
    pub(super) kernel: &'a Kernel,
    pub(super) scope: u64,
}
impl<'a> CallGuard<'a> {
    pub(super) fn new(
        kernel: &'a Kernel,
        root: u64,
        caller: &str,
        authority: Authority,
        resource: Option<String>,
        revoked: bool,
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
        if root != 0 {
            state.calls += 1;
        }
        state.stack.push(Frame {
            ticket: None,
            revoked,
            scope,
            provider: caller.into(),
            authority,
            resource,
        });
        Ok(Self { kernel, scope })
    }
}
impl CallGuard<'_> {
    pub(super) fn attach_ticket(&self, id: u64) {
        self.kernel
            .state
            .borrow_mut()
            .stack
            .last_mut()
            .unwrap()
            .ticket = Some(id);
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
pub(super) fn authenticate_scope(scope: u64) -> Result<ScopeRecord, Failure> {
    authenticate_scope_inner(scope, false)
}
pub(super) fn authenticate_import_scope(scope: u64) -> Result<ScopeRecord, Failure> {
    authenticate_scope_inner(scope, true)
}
fn authenticate_scope_inner(scope: u64, allow_cleanup: bool) -> Result<ScopeRecord, Failure> {
    let record = scopes()
        .lock()
        .map_err(|_| Failure::new(Code::InfrastructureFailure, "scope registry poisoned"))?
        .get(&scope)
        .cloned()
        .ok_or_else(|| Failure::new(Code::ScopeRejected, "expired or unknown scope"))?;
    if record.thread != thread::current().id() {
        return Err(Failure::new(
            Code::ScopeRejected,
            "host callback on an unregistered thread",
        ));
    }
    let kernel = unsafe { &*(record.kernel as *const Kernel) };
    let state = kernel.state.borrow();
    if !state
        .stack
        .last()
        .is_some_and(|f| f.scope == scope && (allow_cleanup || !f.revoked))
    {
        return Err(Failure::new(
            Code::ScopeRejected,
            "scope is not the current live call",
        ));
    }
    Ok(record)
}
pub(super) fn state_host(scope: u64, ticket: u64, permit: u64) -> StateHostApi {
    StateHostApi {
        header: Header::state(std::mem::size_of::<StateHostApi>()),
        scope,
        ticket,
        permit,
        invoke: Some(crate::state_effects::invoke),
        state: Some(super::state_effects::state),
        wake: Some(super::pending::wake),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_contract::abi::Bytes;
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
            let reply = unsafe {
                crate::state_effects::invoke(
                    scope,
                    Bytes::new(b"slot"),
                    Bytes::new(b"not/read"),
                    Bytes::new(&[]),
                )
            };
            let (status, value) = unsafe { reply.consume() }.unwrap();
            assert_eq!(status, 2);
            Failure::from_value(value).unwrap().code
        })
        .join()
        .unwrap();
        assert_eq!(code, Code::ScopeRejected);
        thread::spawn(move || {
            let invalid = Bytes {
                ptr: std::ptr::null(),
                len: 1,
            };
            let reply = unsafe { crate::state_effects::invoke(scope, invalid, invalid, invalid) };
            let (status, value) = unsafe { reply.consume() }.unwrap();
            assert_eq!(status, 2);
            assert_eq!(
                Failure::from_value(value).unwrap().code,
                Code::ScopeRejected
            );
        })
        .join()
        .unwrap();
        scopes().lock().unwrap().remove(&scope);
        let reply = unsafe {
            crate::state_effects::invoke(
                scope,
                Bytes::new(b"slot"),
                Bytes::new(b"not/read"),
                Bytes::new(&[]),
            )
        };
        let (_, value) = unsafe { reply.consume() }.unwrap();
        assert_eq!(
            Failure::from_value(value).unwrap().code,
            Code::ScopeRejected
        );
    }
    #[test]
    fn forged_scope_rejects_before_reading_pointers() {
        let reply = unsafe {
            crate::state_effects::invoke(
                u64::MAX,
                Bytes {
                    ptr: std::ptr::null(),
                    len: 8,
                },
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
