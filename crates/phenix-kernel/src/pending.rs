use super::runtime::{CallGuard, Kernel, Root};
use crate::resolve::Selection;
use phenix_contract::{
    abi::{HostApi, PendingHostApi},
    Authority, Code, Failure, Value,
};
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
}
fn permits() -> &'static Mutex<BTreeMap<u64, Permit>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<u64, Permit>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}
unsafe extern "C" fn wake(permit: u64, ticket: u64) -> u32 {
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
        input: &[u8],
        guard: &CallGuard<'_>,
        authority: Authority,
        resource: Option<String>,
    ) -> Result<Call, Failure> {
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
                },
            );
        let record = Record {
            root,
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
        let api = self.pending_api(id, guard.scope);
        let reply = self.images.borrow()[&selection.provider].pending(
            &api,
            &selection.key(),
            Some(input),
            false,
        );
        self.accept_reply(id, reply);
        Ok(Call::Pending(Ticket { id, root }))
    }
    fn pending_api(&self, id: u64, scope: u64) -> PendingHostApi {
        let state = self.state.borrow();
        let record = &state.tickets[&id];
        PendingHostApi {
            host: HostApi {
                scope,
                invoke: super::runtime::invoke_import,
            },
            ticket: id,
            permit: record.permit,
            wake,
        }
    }
    fn accept_reply(&self, id: u64, reply: Result<(u32, Value), Failure>) {
        let mut state = self.state.borrow_mut();
        let record = state.tickets.get_mut(&id).unwrap();
        let terminal = record.logical != Logical::Pending;
        let result = match reply {
            Ok((3, Value::Null)) => return,
            Ok((4, Value::Null)) if record.cancelled => {
                record.quiescent = true;
                return;
            }
            Ok((status @ (0 | 1 | 5), value)) => {
                record.quiescent = true;
                let contract = self.prepared.packages[&record.selection.provider]
                    .descriptor()
                    .contract(&record.selection.key())
                    .unwrap();
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
        if !terminal {
            let failure = result.as_ref().err().cloned();
            let selection = record.selection.clone();
            record.logical = if result.is_ok() {
                Logical::Ready
            } else {
                Logical::Failed
            };
            record.result = Some(result);
            drop(state);
            if let Some(failure) = failure {
                self.observe_propagation(
                    &failure,
                    Kernel::context(super::runtime::Caller::Root, &selection),
                );
            }
        }
    }
    fn authenticate_ticket(&self, root: u64, ticket: &Ticket) -> Result<(), Failure> {
        self.check_thread()?;
        let state = self.state.borrow();
        if ticket.root != root
            || state.active_root != Some(root)
            || state.tickets.get(&ticket.id).is_none_or(|r| r.root != root)
        {
            return Err(Failure::new(
                Code::ScopeRejected,
                "wrong ticket owner or released ticket",
            ));
        }
        Ok(())
    }
    fn enter_ticket(&self, id: u64, cancel: bool) -> Result<(), Failure> {
        let (root, selection, authority, resource, revoked) = {
            let state = self.state.borrow();
            let r = &state.tickets[&id];
            (
                r.root,
                r.selection.clone(),
                r.authority.clone(),
                r.resource.clone(),
                r.cancelled,
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
        let api = self.pending_api(id, guard.scope);
        let reply =
            self.images.borrow()[&selection.provider].pending(&api, &selection.key(), None, cancel);
        self.accept_reply(id, reply);
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
        self.enter_ticket(id, true)?;
        Ok(self.state.borrow().tickets[&id].quiescent)
    }
    pub(super) fn abandon_root(&self, root: u64) {
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
                let _ = self.cancel_ticket(id);
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
            .filter(|(_, r)| r.abandoned && !r.quiescent)
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
        let tickets: Vec<_> = state.tickets.iter().map(|(id,r)| serde_json::json!({"ticket":id,"root":r.root,"provider":r.selection.provider,"operation":r.selection.key(),"logical":r.logical,"quiescent":r.quiescent,"delivered":r.delivered,"cancellation_requested":r.cancelled,"caller_released":r.abandoned,"wake_requests":r.notification.requests.load(Ordering::Relaxed)})).collect();
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
        self.kernel.state.borrow_mut().tickets.remove(&ticket.id);
        Ok(())
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
