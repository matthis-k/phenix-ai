use super::*;
use std::sync::atomic::AtomicU64;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
pub(super) fn session_id() -> u64 {
    NEXT_SESSION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("session identity exhausted")
}

/// Opaque session/generation-bound authority. Released sequence numbers are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId {
    pub(super) session: u64,
    pub(super) generation: u64,
    pub(super) sequence: u64,
}
impl RequestId {
    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

/// Authenticated by its private construction path, not by application payload IDs.
#[derive(Clone, Debug)]
pub struct CaptureMetadata {
    pub(super) session: u64,
    pub(super) generation: u64,
    pub(super) process: Option<usize>,
    pub(super) output: String,
    pub(super) activation: u64,
    pub(super) request: Option<RequestId>,
    pub(super) capture: Option<String>,
}
impl CaptureMetadata {
    pub fn session(&self) -> u64 {
        self.session
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn process(&self) -> Option<usize> {
        self.process
    }
    pub fn output(&self) -> &str {
        &self.output
    }
    pub fn activation(&self) -> u64 {
        self.activation
    }
    pub fn request(&self) -> Option<RequestId> {
        self.request
    }
    pub fn capture(&self) -> Option<&str> {
        self.capture.as_deref()
    }
    pub(crate) fn for_capture(&self, name: &str) -> Self {
        let mut metadata = self.clone();
        metadata.capture = Some(name.to_owned());
        metadata
    }
}

pub(super) struct RequestScope {
    pub(super) sealed: bool,
    captures: usize,
    expected: Option<String>,
    pub(super) invalid: bool,
    pub(super) reachable: BTreeSet<usize>,
    pub(super) observed_inputs: BTreeSet<(usize, usize)>,
    pub(super) terminal: Option<RequestStatus>,
    pub(super) failure: Option<String>,
    pub(super) disposal: RequestDisposal,
    pub(super) cancelled_jobs: Vec<StepTicket>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    Full,
    Foreign,
    Stale,
    Sealed,
    Exhausted,
    UnknownOutput,
    InvalidSource,
    Busy,
    InvalidCapture,
    CaptureCount(usize),
    NotSettled,
    RecoveryFailed,
    Transport(ConnectionError),
}
#[derive(Debug)]
pub struct RequestInjectionError {
    pub reason: RequestError,
    pub value: Value,
}
impl std::fmt::Display for RequestInjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.reason.fmt(f)
    }
}
impl std::error::Error for RequestInjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.reason)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestStatus {
    Open,
    Pending,
    Settled { captures: usize },
    Cancelled,
    Failed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestDisposal {
    /// Previously committed queued packets, including queued captures.
    pub queued_packets: usize,
    /// Uncommitted fanout copies disposed without publication.
    pub prepared_packets: usize,
    pub activations: usize,
    /// Invalidated runner-owned jobs, not a claim that external work stopped.
    pub handed_jobs: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestWork {
    pub queued: usize,
    /// Both newly reserved activations and ready continuations.
    pub ready: usize,
    pub dispatched: usize,
    pub waiting_input: usize,
    pub waiting_wake: usize,
    pub publication_pending: usize,
    /// Number of distinct full capture FIFOs targeted by this request's
    /// retained, uncommitted publications. Passive snapshot only: the host
    /// must consume or explicitly dispose buffered captures to free capacity.
    pub capture_backpressure: usize,
    /// Bounded all-optional closure obligations registered by admitted roots.
    pub closure_pending: usize,
}
impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "request: {self:?}")
    }
}
impl std::error::Error for RequestError {}

impl NetworkSession {
    /// Reserve one bounded scope. `Some(output)` requires exactly one capture
    /// in the scope, from that compiled projection, before successful settlement.
    pub fn begin_request(&mut self, exact_output: Option<&str>) -> Result<RequestId, RequestError> {
        if self.terminal.is_some() || self.state() == SessionState::Completed {
            return Err(RequestError::Stale);
        }
        if exact_output.is_some_and(|name| !self.captures.contains_key(name)) {
            return Err(RequestError::UnknownOutput);
        }
        if self.requests.len() == self.max_requests {
            return Err(RequestError::Full);
        }
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(RequestError::Exhausted)?;
        let id = RequestId {
            session: self.session_id,
            generation: self.generation,
            sequence: self.next_request,
        };
        self.requests.insert(
            id,
            RequestScope {
                sealed: false,
                captures: 0,
                expected: exact_output.map(str::to_owned),
                invalid: false,
                reachable: BTreeSet::new(),
                observed_inputs: BTreeSet::new(),
                terminal: None,
                failure: None,
                disposal: RequestDisposal::default(),
                cancelled_jobs: vec![],
            },
        );
        Ok(id)
    }
    pub(super) fn check_request(&self, id: RequestId) -> Result<&RequestScope, RequestError> {
        if id.session != self.session_id {
            return Err(RequestError::Foreign);
        }
        if id.generation != self.generation {
            return Err(RequestError::Stale);
        }
        self.requests.get(&id).ok_or(RequestError::Stale)
    }
    pub fn inject_request(
        &mut self,
        id: RequestId,
        name: &str,
        value: Value,
    ) -> Result<(), RequestInjectionError> {
        let validation = self.check_request(id).and_then(|r| {
            if self.terminal.is_some() || r.terminal.is_some() {
                Err(RequestError::Stale)
            } else if r.sealed {
                Err(RequestError::Sealed)
            } else {
                Ok(())
            }
        });
        if let Err(reason) = validation {
            return Err(RequestInjectionError { reason, value });
        }
        self.inject_scoped(name, value, Some(id))
            .map_err(|e| RequestInjectionError {
                reason: RequestError::Transport(e.reason),
                value: e.value,
            })
    }
    pub fn inject_request_local<T: Send + Sync + 'static>(
        &mut self,
        id: RequestId,
        name: &str,
        value: T,
    ) -> Result<(), RequestInjectionError> {
        self.inject_request(id, name, Value::local(value))
    }
    /// Start only an explicitly compiled zero-input source, using the same
    /// persistent process and handoff. No root guessing or process respawning.
    pub fn start_request_source(&mut self, id: RequestId, path: &str) -> Result<(), RequestError> {
        let scope = self.check_request(id)?;
        if self.terminal.is_some() || scope.terminal.is_some() {
            return Err(RequestError::Stale);
        }
        if scope.sealed {
            return Err(RequestError::Sealed);
        }
        let index = self
            .compiled
            .processes()
            .iter()
            .position(|p| p.path() == path)
            .ok_or(RequestError::InvalidSource)?;
        let p = &mut self.processes[index];
        if !p.source {
            return Err(RequestError::InvalidSource);
        }
        if p.done || p.handoff.state() != HandoffState::Idle {
            return Err(RequestError::Busy);
        }
        p.handoff
            .admit(ProcessContext::default().with_resources(p.resources.clone()))
            .map_err(|_| RequestError::Exhausted)?;
        p.request = Some(id);
        self.note_request_root(id, index);
        self.refresh();
        Ok(())
    }
    /// Seals request ingress, not the persistent network's public input writers.
    pub fn close_request(&mut self, id: RequestId) -> Result<(), RequestError> {
        let scope = self.check_request(id)?;
        if self.terminal.is_some() || scope.terminal.is_some() {
            return Err(RequestError::Stale);
        }
        self.requests.get_mut(&id).unwrap().sealed = true;
        self.refresh();
        Ok(())
    }
    pub fn request_status(&self, id: RequestId) -> Result<RequestStatus, RequestError> {
        let request = self.check_request(id)?;
        if let Some(status) = request.terminal {
            return Ok(status);
        }
        if self.terminal.is_some() {
            return Ok(if self.terminal == Some(SessionState::Failed) {
                RequestStatus::Failed
            } else {
                RequestStatus::Cancelled
            });
        }
        if request.invalid {
            return Err(RequestError::InvalidCapture);
        }
        if !request.sealed {
            return Ok(RequestStatus::Open);
        }
        // Ownership is represented in exactly the same queues and handoffs as
        // ordinary execution. No zero-count callback manufactures new work.
        if self.request_live(id) {
            return Ok(RequestStatus::Pending);
        }
        if request.expected.is_some() && request.captures != 1 {
            return Err(RequestError::CaptureCount(request.captures));
        }
        Ok(RequestStatus::Settled {
            captures: request.captures,
        })
    }
    pub fn release_request(&mut self, id: RequestId) -> Result<(), RequestError> {
        self.check_request(id)?;
        if self.terminal.is_none() && (self.request_live(id) || !self.requests[&id].sealed) {
            return Err(RequestError::NotSettled);
        }
        self.requests.remove(&id);
        Ok(())
    }
    fn request_live(&self, id: RequestId) -> bool {
        self.processes
            .iter()
            .enumerate()
            .any(|(index, p)| p.request == Some(id) || self.closure_obligation(index, id))
            || self.transport.queues().iter().any(|q| {
                q.packets()
                    .any(|p| p.metadata().and_then(|m| m.request()) == Some(id))
            })
    }
    pub fn request_work(&self, id: RequestId) -> Result<RequestWork, RequestError> {
        self.check_request(id)?;
        let mut work = RequestWork {
            closure_pending: (0..self.processes.len())
                .filter(|index| self.closure_obligation(*index, id))
                .count(),
            queued: self
                .transport
                .queues()
                .iter()
                .map(|q| {
                    q.packets()
                        .filter(|p| p.metadata().and_then(|m| m.request()) == Some(id))
                        .count()
                })
                .sum(),
            ..Default::default()
        };
        let blocked_captures: BTreeSet<_> = self
            .processes
            .iter()
            .filter(|p| p.request == Some(id) && p.pending.is_some())
            .flat_map(|p| p.pending_targets.iter().copied())
            .filter(|queue| {
                matches!(&self.endpoints[*queue].1, QueueEndpoint::Capture(_))
                    && self.transport.queues()[*queue].is_full()
            })
            .collect();
        work.capture_backpressure = blocked_captures.len();
        for p in self.processes.iter().filter(|p| p.request == Some(id)) {
            match p.handoff.state() {
                HandoffState::Ready => work.ready += 1,
                HandoffState::Dispatched => work.dispatched += 1,
                HandoffState::WaitingInput => work.waiting_input += 1,
                HandoffState::WaitingWake => work.waiting_wake += 1,
                HandoffState::PublicationPending => work.publication_pending += 1,
                _ => {}
            }
        }
        Ok(work)
    }
    /// Read this request's first buffered capture, even when an unrelated
    /// request precedes it in the shared output queue. Global take_output
    /// retains strict FIFO, while scoped reads preserve FIFO within each
    /// request. This cannot bypass a full queue that has not published the
    /// requested packet yet.
    pub fn take_request_output(
        &mut self,
        id: RequestId,
        name: &str,
    ) -> Result<Option<Packet>, RequestError> {
        let scope = self.check_request(id)?;
        if self.terminal.is_some() || scope.terminal.is_some() {
            return Err(RequestError::Stale);
        }
        let queue = *self.captures.get(name).ok_or(RequestError::UnknownOutput)?;
        let Some((offset, packet)) = self.transport.queues()[queue]
            .packets()
            .enumerate()
            .find(|(_, packet)| packet.metadata().and_then(|m| m.request()) == Some(id))
        else {
            return Ok(None);
        };
        // Check the exact original packet before any ownership transfer. A
        // malformed matching capture must not be removed or skipped.
        self.validate_capture(id, name, packet)?;
        let packet = self.transport.pop_at(queue, offset).unwrap();
        self.observe_capture(name, &packet);
        self.refresh();
        Ok(Some(packet))
    }
    pub fn validate_capture(
        &self,
        id: RequestId,
        name: &str,
        packet: &Packet,
    ) -> Result<(), RequestError> {
        let request = self.check_request(id)?;
        if self.terminal.is_some() || request.terminal.is_some() {
            return Err(RequestError::Stale);
        }
        let (process, output) = self
            .compiled
            .outputs()
            .get(name)
            .ok_or(RequestError::UnknownOutput)?;
        let metadata = packet.metadata().ok_or(RequestError::InvalidCapture)?;
        if metadata.session != self.session_id
            || metadata.generation != self.generation
            || metadata.request != Some(id)
            || metadata.process != Some(*process)
            || metadata.output != *output
            || metadata.capture.as_deref() != Some(name)
            || packet.origin.as_ref() != self.compiled.processes()[*process].origin()
            || request
                .expected
                .as_ref()
                .is_some_and(|expected| expected != name)
        {
            return Err(RequestError::InvalidCapture);
        }
        Ok(())
    }
    pub(super) fn observe_capture(&mut self, name: &str, packet: &Packet) {
        let Some(metadata) = packet.metadata() else {
            return;
        };
        let Some(id) = metadata.request else {
            return;
        };
        let Some(request) = self.requests.get_mut(&id) else {
            return;
        };
        let (process, output) = &self.compiled.outputs()[name];
        let valid = metadata.session == self.session_id
            && metadata.generation == self.generation
            && metadata.process == Some(*process)
            && metadata.output == *output
            && metadata.capture.as_deref() == Some(name)
            && request
                .expected
                .as_ref()
                .is_none_or(|expected| expected == name);
        request.invalid |= !valid;
        request.captures = request.captures.saturating_add(1);
    }
}
