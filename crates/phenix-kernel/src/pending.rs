use super::runtime::{CallGuard, Kernel, Root};
use crate::resolve::Selection;
use phenix_contract::{abi::StateHostApi, Authority, Code, Failure, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
static NEXT_PERMIT: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct Notification {
    requests: AtomicU64,
}
struct Permit {
    ticket: u64,
    notification: Arc<Notification>,
    ancestors: Vec<Arc<Notification>>,
}
fn permits() -> &'static Mutex<BTreeMap<u64, Permit>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<u64, Permit>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}
pub(super) unsafe extern "C" fn wake(permit: u64, ticket: u64) -> u32 {
    let Ok(registry) = permits().lock() else {
        return 1;
    };
    let Some(record) = registry.get(&permit) else {
        return 1;
    };
    if record.ticket != ticket {
        return 1;
    }
    record.notification.requests.fetch_add(1, Ordering::Relaxed);
    for notification in &record.ancestors {
        notification.requests.fetch_add(1, Ordering::Relaxed);
    }
    0
}
fn identity(counter: &AtomicU64) -> Result<u64, Failure> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| Failure::new(Code::InfrastructureFailure, "pending identity exhausted"))
}
/// Opaque control-plane handle. It carries no transferable plugin authority.
#[derive(Debug)]
pub struct Ticket {
    pub(super) id: u64,
    root: u64,
}
#[derive(Debug)]
pub enum Call {
    Ready(super::runtime::Outcome),
    Pending(Ticket),
}
#[derive(Debug)]
pub enum Poll {
    Pending,
    Ready(Result<super::runtime::Outcome, Failure>),
    Cancelled { quiescent: bool },
    Draining,
}
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Logical {
    Pending,
    Ready,
    Failed,
    Cancelled,
}
pub(super) struct Record {
    pub root: u64,
    pub parent: Option<u64>,
    pub import_slot: String,
    pub native_quiescent: bool,
    pub selection: Selection,
    pub authority: Authority,
    pub resource: Option<String>,
    pub logical: Logical,
    pub quiescent: bool,
    pub delivered: bool,
    pub cancelled: bool,
    pub abandoned: bool,
    result: Option<Result<super::runtime::Outcome, Failure>>,
    permit: u64,
    notification: Arc<Notification>,
}
impl Drop for Record {
    fn drop(&mut self) {
        if let Ok(mut registry) = permits().lock() {
            registry.remove(&self.permit);
        }
    }
}
impl Kernel {
    pub(super) fn begin_pending(
        &self,
        root: u64,
        selection: &Selection,
        import_slot: &str,
        input: &[u8],
        guard: &CallGuard<'_>,
        scope: phenix_contract::ScopedRequest,
    ) -> Result<Call, Failure> {
        let phenix_contract::ScopedRequest {
            authority,
            resource,
        } = scope;
        let parent = self
            .state
            .borrow()
            .stack
            .iter()
            .rev()
            .nth(1)
            .and_then(|f| f.ticket);
        let mut ancestors = Vec::new();
        let mut ancestor = parent;
        while let Some(id) = ancestor {
            let state = self.state.borrow();
            let record = &state.tickets[&id];
            ancestors.push(record.notification.clone());
            ancestor = record.parent;
        }
        let id = identity(&NEXT_TICKET)?;
        let permit = identity(&NEXT_PERMIT)?;
        let notification = Arc::new(Notification::default());
        permits()
            .lock()
            .map_err(|_| Failure::new(Code::InfrastructureFailure, "permit registry poisoned"))?
            .insert(
                permit,
                Permit {
                    ticket: id,
                    notification: notification.clone(),
                    ancestors,
                },
            );
        let record = Record {
            root,
            parent,
            import_slot: import_slot.into(),
            native_quiescent: false,
            selection: selection.clone(),
            authority,
            resource,
            logical: Logical::Pending,
            quiescent: false,
            delivered: false,
            cancelled: false,
            abandoned: false,
            result: None,
            permit,
            notification,
        };
        self.state.borrow_mut().tickets.insert(id, record);
        guard.attach_ticket(id);
        let api = self.pending_api(id, guard.scope);
        let reply = self.images.borrow()[&selection.provider].pending(
            &api,
            &selection.key(),
            Some(input),
            false,
        );
        self.accept_reply(id, reply);
        let ready = {
            let mut state = self.state.borrow_mut();
            let record = state.tickets.get_mut(&id).unwrap();
            if parent.is_some()
                && record.quiescent
                && record.result.as_ref().is_some_and(Result::is_ok)
            {
                record.result.take()
            } else {
                None
            }
        };
        if let Some(Ok(outcome)) = ready {
            self.release_tree(id);
            return Ok(Call::Ready(outcome));
        }
        Ok(Call::Pending(Ticket { id, root }))
    }
    fn pending_api(&self, id: u64, scope: u64) -> StateHostApi {
        super::runtime::state_host(scope, id, self.state.borrow().tickets[&id].permit)
    }
    fn accept_reply(&self, id: u64, reply: Result<(u32, Value), Failure>) {
        let live_children = self
            .children(id)
            .iter()
            .any(|child| !self.state.borrow().tickets[child].quiescent);
        let mut state = self.state.borrow_mut();
        let record = state.tickets.get_mut(&id).unwrap();
        let terminal = record.logical != Logical::Pending;
        let mut result = match reply {
            Ok((3, Value::Null)) => return,
            Ok((4, Value::Null)) if record.cancelled => {
                record.native_quiescent = true;
                drop(state);
                self.refresh_quiescence();
                return;
            }
            Ok((status @ (0 | 1 | 5), value)) => {
                record.native_quiescent = true;
                let contract = &self.prepared.operation(&record.selection).unwrap().schema;
                match status {
                    0 if contract.response.accepts(&value) => {
                        Ok(super::runtime::Outcome::Success(value))
                    }
                    1 if contract.error.accepts(&value) => {
                        Ok(super::runtime::Outcome::Domain(value))
                    }
                    5 => Err(Failure::from_value(value).unwrap_or_else(|e| e)),
                    _ => Err(Failure::new(
                        Code::SchemaRejected,
                        "undeclared pending result",
                    )),
                }
            }
            Ok((2, value)) => Err(Failure::from_value(value).unwrap_or_else(|e| e)),
            Ok(_) => Err(Failure::new(Code::AbiIncompatible, "invalid pending reply")),
            Err(failure) => Err(failure),
        };
        if record.native_quiescent && live_children {
            result = Err(Failure::new(
                Code::PluginFailed,
                "native completion with live imported work",
            ));
        }
        let failed = result.is_err();
        if !terminal {
            let failure = result.as_ref().err().cloned();
            let selection = record.selection.clone();
            record.logical = if result.is_ok() {
                Logical::Ready
            } else {
                Logical::Failed
            };
            record.result = Some(result);
            if let Some(failure) = failure {
                drop(state);
                self.observe_propagation(
                    &failure,
                    Kernel::context(super::runtime::Caller::Root, &selection),
                );
            } else {
                drop(state);
            }
        } else {
            drop(state);
        }
        if failed {
            self.cells.borrow_mut().release_ticket(id);
            if let Err(failure) = self.cancel_children(id) {
                let selection = self.state.borrow().tickets[&id].selection.clone();
                self.observe_propagation(
                    &failure,
                    Kernel::context(super::runtime::Caller::Root, &selection),
                );
            }
        }
        self.refresh_quiescence();
    }
    fn children(&self, id: u64) -> Vec<u64> {
        self.state
            .borrow()
            .tickets
            .iter()
            .filter(|(_, r)| r.parent == Some(id))
            .map(|(id, _)| *id)
            .collect()
    }
    fn refresh_quiescence(&self) {
        let ids: Vec<_> = self.state.borrow().tickets.keys().rev().copied().collect();
        for id in ids {
            let quiet = self.state.borrow().tickets[&id].native_quiescent
                && self
                    .children(id)
                    .iter()
                    .all(|c| self.state.borrow().tickets[c].quiescent);
            self.state
                .borrow_mut()
                .tickets
                .get_mut(&id)
                .unwrap()
                .quiescent = quiet;
        }
    }
    fn cancel_children(&self, id: u64) -> Result<(), Failure> {
        // Attempt every child even when one native cancellation fails.
        // The first failure propagates through an explicit caller's cancel;
        // terminal/root-drop paths observe it instead of silently discarding it.
        let mut first_failure = None;
        for child in self.children(id) {
            let state = self.state.borrow();
            let should_cancel =
                !state.tickets[&child].cancelled && !state.tickets[&child].quiescent;
            drop(state);
            if should_cancel {
                if let Err(failure) = self.cancel_ticket(child) {
                    if first_failure.is_none() {
                        first_failure = Some(failure);
                    }
                }
            }
        }
        match first_failure {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
    fn authenticate_ticket(&self, root: u64, ticket: &Ticket) -> Result<(), Failure> {
        self.check_thread()?;
        let state = self.state.borrow();
        if ticket.root != root
            || state.active_root != Some(root)
            || state
                .tickets
                .get(&ticket.id)
                .is_none_or(|r| r.root != root || r.parent.is_some())
        {
            return Err(Failure::new(
                Code::ScopeRejected,
                "wrong ticket owner or released ticket",
            ));
        }
        Ok(())
    }
    fn enter_ticket(&self, id: u64, cancel: bool) -> Result<(), Failure> {
        if self.state.borrow().tickets[&id].native_quiescent {
            self.drain_children(id)?;
            self.refresh_quiescence();
            return Ok(());
        }
        let (root, selection, authority, resource, revoked) = {
            let state = self.state.borrow();
            let r = &state.tickets[&id];
            (
                r.root,
                r.selection.clone(),
                r.authority.clone(),
                r.resource.clone(),
                r.cancelled || r.logical == Logical::Failed,
            )
        };
        let guard = CallGuard::new(
            self,
            root,
            &selection.provider,
            authority,
            resource,
            revoked,
        )?;
        guard.attach_ticket(id);
        let api = self.pending_api(id, guard.scope);
        let reply =
            self.images.borrow()[&selection.provider].pending(&api, &selection.key(), None, cancel);
        self.accept_reply(id, reply);
        let revoked = {
            let state = self.state.borrow();
            let r = &state.tickets[&id];
            r.cancelled || r.logical == Logical::Failed
        };
        if revoked {
            self.drain_children(id)?;
        }
        self.refresh_quiescence();
        Ok(())
    }
    fn cancel_ticket(&self, id: u64) -> Result<bool, Failure> {
        {
            let mut state = self.state.borrow_mut();
            let record = state.tickets.get_mut(&id).unwrap();
            if record.cancelled || record.quiescent {
                return Err(Failure::new(
                    Code::ScopeRejected,
                    "ticket is already cancelled or quiescent",
                ));
            }
            record.cancelled = true;
            if record.logical == Logical::Pending {
                record.logical = Logical::Cancelled;
            }
        }
        // Do not skip the parent's native cancel if a child fails: report
        // the failure after attempting both physical cancellation paths.
        let descendant_failure = self.cancel_children(id).err();
        let own_result = self.enter_ticket(id, true);
        if let Some(failure) = descendant_failure {
            return Err(failure);
        }
        own_result?;
        Ok(self.state.borrow().tickets[&id].quiescent)
    }
    fn drain_children(&self, id: u64) -> Result<(), Failure> {
        for child in self.children(id) {
            if !self.state.borrow().tickets[&child].quiescent {
                self.enter_ticket(child, false)?;
            }
        }
        Ok(())
    }
    fn release_tree(&self, id: u64) {
        self.cells.borrow_mut().release_ticket(id);
        for child in self.children(id) {
            self.release_tree(child);
        }
        self.state.borrow_mut().tickets.remove(&id);
    }
    pub(super) fn abandon_root(&self, root: u64) {
        self.cells.borrow_mut().release_root(root);
        let ids: Vec<_> = self
            .state
            .borrow()
            .tickets
            .iter()
            .filter(|(_, r)| r.root == root)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.state
                .borrow_mut()
                .tickets
                .get_mut(&id)
                .unwrap()
                .abandoned = true;
            if !self.state.borrow().tickets[&id].quiescent
                && !self.state.borrow().tickets[&id].cancelled
            {
                if let Err(failure) = self.cancel_ticket(id) {
                    // Root::drop cannot return an error. Surface the failure
                    // through the existing propagation channel, and leave
                    // unfinished physical work for drain_cancelled().
                    let selection = self.state.borrow().tickets[&id].selection.clone();
                    self.observe_propagation(
                        &failure,
                        Kernel::context(super::runtime::Caller::Root, &selection),
                    );
                }
            }
        }
        self.release_abandoned();
    }
    fn release_abandoned(&self) {
        let mut state = self.state.borrow_mut();
        state.tickets.retain(|_, r| !(r.abandoned && r.quiescent));
        if !state.root_live && state.tickets.is_empty() && state.active_root.take().is_some() {
            state.completed_roots += 1;
        }
    }
    /// Trusted draining after caller drop. Polls serially, without restoring effects.
    pub fn drain_cancelled(&self) -> Result<usize, Failure> {
        self.check_thread()?;
        let ids: Vec<_> = self
            .state
            .borrow()
            .tickets
            .iter()
            .filter(|(_, r)| r.abandoned && !r.quiescent && r.parent.is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.enter_ticket(id, false)?;
        }
        self.release_abandoned();
        Ok(self
            .state
            .borrow()
            .tickets
            .values()
            .filter(|r| !r.quiescent)
            .count())
    }
    pub(super) fn pending_inspection(&self) -> serde_json::Value {
        let state = self.state.borrow();
        let tickets: Vec<_> = state.tickets.iter().map(|(id,r)| serde_json::json!({"ticket":id,"root":r.root,"parent":r.parent,"native_quiescent":r.native_quiescent,"provider":r.selection.provider,"operation":r.selection.key(),"logical":r.logical,"quiescent":r.quiescent,"delivered":r.delivered,"cancellation_requested":r.cancelled,"caller_released":r.abandoned,"wake_requests":r.notification.requests.load(Ordering::Relaxed)})).collect();
        serde_json::json!({"format":"phenix.pending.v1","tickets":tickets,"physical_leases":state.tickets.values().filter(|r| !r.quiescent).count()})
    }
    pub(super) fn invalidate_permits(&self) {
        if let Ok(mut registry) = permits().lock() {
            for r in self.state.borrow().tickets.values() {
                registry.remove(&r.permit);
            }
        }
    }
}
impl Root<'_> {
    pub fn begin(&self, request: &Value) -> Result<Call, Failure> {
        let result = self.kernel.dispatch_call(
            self.id,
            super::runtime::Caller::Root,
            &self.entry,
            &request.encode()?,
            Some(&self.scope),
            true,
        );
        if let Err(failure) = &result {
            self.kernel.observe_propagation(
                failure,
                Kernel::context(super::runtime::Caller::Root, &self.entry),
            );
        }
        result
    }
    pub fn poll(&self, ticket: &Ticket) -> Result<Poll, Failure> {
        self.kernel.authenticate_ticket(self.id, ticket)?;
        let (quiescent, delivered) = {
            let state = self.kernel.state.borrow();
            let r = &state.tickets[&ticket.id];
            (r.quiescent, r.delivered)
        };
        if quiescent && delivered {
            return Err(Failure::new(
                Code::ScopeRejected,
                "terminal ticket already delivered",
            ));
        }
        if !quiescent {
            self.kernel.enter_ticket(ticket.id, false)?;
        }
        let mut state = self.kernel.state.borrow_mut();
        let r = state.tickets.get_mut(&ticket.id).unwrap();
        if let Some(result) = r.result.take() {
            r.delivered = true;
            return Ok(Poll::Ready(result));
        }
        if r.logical == Logical::Cancelled {
            return Ok(Poll::Cancelled {
                quiescent: r.quiescent,
            });
        }
        if r.delivered {
            Ok(Poll::Draining)
        } else {
            Ok(Poll::Pending)
        }
    }
    pub fn cancel(&self, ticket: &Ticket) -> Result<bool, Failure> {
        self.kernel.authenticate_ticket(self.id, ticket)?;
        self.kernel.cancel_ticket(ticket.id)
    }
    pub fn release(&self, ticket: &Ticket) -> Result<(), Failure> {
        self.kernel.authenticate_ticket(self.id, ticket)?;
        if !self.kernel.state.borrow().tickets[&ticket.id].quiescent {
            return Err(Failure::new(
                Code::ScopeRejected,
                "physical lease is still live",
            ));
        }
        self.kernel.release_tree(ticket.id);
        Ok(())
    }
}

impl Kernel {
    pub(super) fn import_pending(
        &self,
        root: u64,
        caller: &str,
        slot: &str,
        parent: u64,
        command: &str,
        bytes: &[u8],
    ) -> Result<phenix_contract::abi::Reply, Failure> {
        use phenix_contract::abi::Reply;
        let malformed = || Failure::new(Code::SchemaRejected, "invalid pending host command");
        match command {
            "phenix.host.pending.begin.v2" | "phenix.host.pending.scoped.v2" => {
                let (key, input, scope) = if command == "phenix.host.pending.scoped.v2" {
                    let (key, input, scope) = phenix_contract::ScopedRequest::decode(bytes)?;
                    (key, input, Some(scope))
                } else {
                    let Value::Map(mut fields) = Value::decode(bytes)? else {
                        return Err(malformed());
                    };
                    if fields.len() != 2 {
                        return Err(malformed());
                    }
                    let Some(Value::String(key)) = fields.remove("operation") else {
                        return Err(malformed());
                    };
                    let Some(Value::Bytes(input)) = fields.remove("request") else {
                        return Err(malformed());
                    };
                    (key, input, None)
                };
                let Some(provider) = self
                    .prepared
                    .bindings
                    .get(&(caller.into(), phenix_contract::import_identity(slot, &key)))
                else {
                    return self.fail(Code::MissingBinding, "pending import is not selected");
                };
                let (interface, operation) = key.split_once('/').ok_or_else(malformed)?;
                let selection = Selection {
                    interface: interface.into(),
                    operation: operation.into(),
                    provider: provider.clone(),
                };
                Ok(
                    match self.dispatch_call(
                        root,
                        super::runtime::Caller::Plugin(caller, slot),
                        &selection,
                        &input,
                        scope.as_ref(),
                        true,
                    )? {
                        Call::Ready(super::runtime::Outcome::Success(value)) => {
                            Reply::value(0, value)
                        }
                        Call::Ready(super::runtime::Outcome::Domain(value)) => {
                            Reply::value(1, value)
                        }
                        Call::Pending(ticket) => Reply::value(3, Value::U64(ticket.id)),
                    },
                )
            }
            "phenix.host.pending.poll.v2" | "phenix.host.pending.cancel.v2" => {
                let Value::Map(mut fields) = Value::decode(bytes)? else {
                    return Err(malformed());
                };
                if fields.len() != 2 {
                    return Err(malformed());
                }
                let Some(Value::String(operation)) = fields.remove("operation") else {
                    return Err(malformed());
                };
                let Some(Value::U64(id)) = fields.remove("ticket") else {
                    return Err(malformed());
                };
                let authenticated = {
                    let state = self.state.borrow();
                    state.tickets.get(&id).is_some_and(|r| {
                        r.root == root
                            && r.parent == Some(parent)
                            && r.selection.key() == operation
                            && r.import_slot == slot
                            && self.prepared.bindings.get(&(
                                caller.into(),
                                phenix_contract::import_identity(slot, &operation),
                            )) == Some(&r.selection.provider)
                    }) && state
                        .tickets
                        .get(&parent)
                        .is_some_and(|r| r.root == root && r.selection.provider == caller)
                };
                if !authenticated {
                    return Err(Failure::new(
                        Code::ScopeRejected,
                        "wrong imported ticket owner or operation",
                    ));
                }
                if command == "phenix.host.pending.cancel.v2" {
                    return Ok(Reply::value(0, Value::Bool(self.cancel_ticket(id)?)));
                }
                if !self.state.borrow().tickets[&id].quiescent {
                    self.enter_ticket(id, false)?;
                }
                let mut state = self.state.borrow_mut();
                let r = state.tickets.get_mut(&id).unwrap();
                if !r.quiescent {
                    return Ok(Reply::value(
                        if r.logical == Logical::Pending { 3 } else { 5 },
                        Value::Null,
                    ));
                }
                let reply = match r.result.take() {
                    Some(Ok(super::runtime::Outcome::Success(value))) => Reply::value(0, value),
                    Some(Ok(super::runtime::Outcome::Domain(value))) => Reply::value(1, value),
                    Some(Err(failure)) => Reply::value(2, failure.to_value()),
                    None if r.logical == Logical::Cancelled => Reply::value(4, Value::Null),
                    None => {
                        return Err(Failure::new(
                            Code::ScopeRejected,
                            "imported result already delivered",
                        ))
                    }
                };
                drop(state);
                self.release_tree(id);
                self.refresh_quiescence();
                Ok(reply)
            }
            _ => Err(malformed()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wake_authenticates_owner_and_rejects_release_without_host_pointer() {
        let permit = identity(&NEXT_PERMIT).unwrap();
        let ticket = identity(&NEXT_TICKET).unwrap();
        let notification = Arc::new(Notification::default());
        permits().lock().unwrap().insert(
            permit,
            Permit {
                ticket,
                notification: notification.clone(),
                ancestors: vec![],
            },
        );
        assert_eq!(unsafe { wake(permit, ticket + 1) }, 1);
        assert_eq!(
            std::thread::spawn(move || unsafe { wake(permit, ticket) })
                .join()
                .unwrap(),
            0
        );
        assert_eq!(notification.requests.load(Ordering::Relaxed), 1);
        permits().lock().unwrap().remove(&permit);
        assert_eq!(unsafe { wake(permit, ticket) }, 1);
        assert_eq!(unsafe { wake(u64::MAX, u64::MAX) }, 1);
    }
}
