//! Native frontend with explicitly selected auxiliary captures. The session owns scheduling, correlation,
//! publication and recovery. A frontend owns admitted request lifetime and
//! may dispatch owned jobs outside the session lease.

use crate::{AgentEvent, EVENTS, PROMPT};
use phenix_graph::{
    CaptureHandle, InputKey, OutputKey,
    connection::Packet,
    process::{ActivationJob, CompletedStep},
    runtime::{RequestId, RequestStatus},
};
use phenix_kernel::{CallbackError, host::Generation};

/// An immediate result of driving a turn. Waiting is not completion.
/// A reactor can subscribe to the session's wake listener before polling.
pub enum TurnPoll {
    Events(Vec<AgentEvent>),
    Observation { index: usize, packet: Packet },
    Job(ActivationJob),
    Waiting,
    Complete,
}

/// One admitted native request. Use one driver per generation; the graph's
/// own session rules still govern ownership of jobs and completions.
/// The driver cannot preempt running native code or undo external effects.
pub struct Turn<'a> {
    generation: &'a Generation,
    request: RequestId,
    capture: CaptureHandle,
    observations: Vec<CaptureHandle>,
    completed: bool,
    released: bool,
}

impl<'a> Turn<'a> {
    pub fn begin(generation: &'a Generation, prompt: String) -> Result<Self, CallbackError> {
        let ingress = generation
            .compiled()
            .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))?;
        let capture = generation
            .compiled()
            .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))?;
        let request = generation.with_session(|session| session.begin_bound_request(None))??;
        let mut turn = Self {
            generation,
            request,
            capture,
            observations: Vec::new(),
            completed: false,
            released: false,
        };
        let result = generation.with_session(|session| -> Result<(), CallbackError> {
            session.inject_bound_request(request, &ingress, prompt)?;
            session.close_request(request)?;
            Ok(())
        });
        let result = result
            .map_err(CallbackError::from)
            .and_then(|result| result);
        if let Err(error) = result {
            return Err(turn.cleanup_error(error));
        }
        Ok(turn)
    }

    /// Register a generation-bound auxiliary capture before driving work.
    /// The index is stable for this turn; capture validation remains in Graph.
    pub fn observe(&mut self, capture: CaptureHandle) {
        self.observations.push(capture);
    }

    /// Poll without running a process under the session lease. A returned
    /// job belongs to the caller until it is submitted, rejected or retired.
    /// Drain events before asking the scheduler for another owned step.
    pub fn poll(&mut self) -> Result<TurnPoll, CallbackError> {
        if self.released {
            return Err(std::io::Error::other("turn already released").into());
        }
        let request = self.request;
        let capture = &self.capture;
        let observations = &self.observations;
        let poll = self
            .generation
            .with_session(|session| -> Result<_, CallbackError> {
                if let Some(failure) = session.failure() {
                    return Err(failure.clone().into());
                }
                if let Some(failure) = session.request_failure(request)? {
                    return Err(failure.clone().into());
                }
                let mut events = Vec::new();
                while let Some(packet) = session.take_bound_request_output(request, capture)? {
                    session.validate_bound_capture(request, capture, &packet)?;
                    events.push(
                        packet
                            .value()
                            .borrow::<AgentEvent>()
                            .expect("typed capture")
                            .clone(),
                    );
                }
                if !events.is_empty() {
                    return Ok(TurnPoll::Events(events));
                }
                for (index, capture) in observations.iter().enumerate() {
                    if let Some(packet) = session.take_bound_request_output(request, capture)? {
                        session.validate_bound_capture(request, capture, &packet)?;
                        return Ok(TurnPoll::Observation { index, packet });
                    }
                }
                match session.request_status(request)? {
                    RequestStatus::Settled { .. } => {
                        session.release_request(request)?;
                        return Ok(TurnPoll::Complete);
                    }
                    RequestStatus::Cancelled => {
                        return Err(std::io::Error::other("turn cancelled").into());
                    }
                    RequestStatus::Failed => {
                        return Err(std::io::Error::other("turn failed").into());
                    }
                    _ => {}
                }
                if let Some((_, job)) = session.next_job() {
                    return Ok(TurnPoll::Job(job));
                }
                // next_job may commit pending publication or supervise a deadline
                // even if it has no executable step to return.
                if let Some(failure) = session.request_failure(request)? {
                    return Err(failure.clone().into());
                }
                if matches!(session.request_status(request)?, RequestStatus::Cancelled) {
                    return Err(std::io::Error::other("turn cancelled").into());
                }
                if let Some(packet) = session.take_bound_request_output(request, capture)? {
                    session.validate_bound_capture(request, capture, &packet)?;
                    return Ok(TurnPoll::Events(vec![
                        packet
                            .value()
                            .borrow::<AgentEvent>()
                            .expect("typed capture")
                            .clone(),
                    ]));
                }
                for (index, capture) in observations.iter().enumerate() {
                    if let Some(packet) = session.take_bound_request_output(request, capture)? {
                        session.validate_bound_capture(request, capture, &packet)?;
                        return Ok(TurnPoll::Observation { index, packet });
                    }
                }
                if matches!(
                    session.request_status(request)?,
                    RequestStatus::Settled { .. }
                ) {
                    session.release_request(request)?;
                    return Ok(TurnPoll::Complete);
                }
                Ok(TurnPoll::Waiting)
            })??;
        match &poll {
            TurnPoll::Events(events) => {
                for event in events {
                    if self.completed {
                        return Err(std::io::Error::other("event after completion").into());
                    }
                    self.completed = event == &AgentEvent::Completed;
                }
            }
            TurnPoll::Complete => {
                self.released = true;
                if !self.completed {
                    return Err(std::io::Error::other("request settled without completion").into());
                }
            }
            _ => {}
        }
        Ok(poll)
    }

    /// Return a worker completion through the canonical handoff. Preserve
    /// refused completions intact, including on session contention.
    pub fn complete_job(
        &self,
        completion: CompletedStep,
    ) -> Result<(), (CallbackError, Box<CompletedStep>)> {
        let mut completion = Some(completion);
        match self.generation.with_session(|session| {
            session.complete_job(completion.take().expect("owned completion"))
        }) {
            Ok(Ok(())) => Ok(()),
            Ok(Err((error, completion))) => Err((error.into(), completion)),
            Err(error) => Err((
                error.into(),
                Box::new(completion.expect("session refused before callback")),
            )),
        }
    }

    /// Explicitly invalidate outstanding tickets and release the request.
    /// The caller may still own a job; returning it later must be rejected.
    pub fn cancel(&mut self) -> Result<(), CallbackError> {
        if self.released {
            return Ok(());
        }
        self.generation
            .with_session(|session| -> Result<(), CallbackError> {
                if !matches!(
                    session.request_status(self.request)?,
                    RequestStatus::Cancelled
                        | RequestStatus::Failed
                        | RequestStatus::Settled { .. }
                ) {
                    session.cancel_request(self.request)?;
                }
                session.release_request(self.request)?;
                Ok(())
            })??;
        self.released = true;
        Ok(())
    }

    fn cleanup_error(&mut self, error: CallbackError) -> CallbackError {
        match self.cancel() {
            Ok(()) => error,
            Err(cleanup) => {
                std::io::Error::other(format!("{error}; request cleanup failed: {cleanup}")).into()
            }
        }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        // Explicit cancel reports contention/cleanup errors. Drop is only a
        // best effort fallback, including unwinding from frontend callbacks.
        let _ = self.cancel();
    }
}

/// Synchronous convenience for immediate workloads. Uses the same owned
/// handoff as an external worker or reactor, never a second execution path.
pub fn run_turn(
    generation: &Generation,
    prompt: String,
    budget: usize,
    emit: impl FnMut(&AgentEvent),
) -> Result<(), CallbackError> {
    run_turn_observed(generation, prompt, budget, Vec::new(), emit, |_, _| Ok(()))
}

/// Drive the same lifecycle while delivering additional authorized captures.
/// Both callbacks run outside the session lease.
pub fn run_turn_observed(
    generation: &Generation,
    prompt: String,
    budget: usize,
    observations: Vec<CaptureHandle>,
    mut emit: impl FnMut(&AgentEvent),
    mut observe: impl FnMut(usize, Packet) -> Result<(), CallbackError>,
) -> Result<(), CallbackError> {
    let mut turn = Turn::begin(generation, prompt)?;
    for capture in observations {
        turn.observe(capture);
    }
    let result = (|| -> Result<(), CallbackError> {
        let mut remaining = budget;
        loop {
            if remaining == 0 {
                return Err(std::io::Error::other("turn step budget exhausted").into());
            }
            match turn.poll()? {
                TurnPoll::Events(events) => {
                    for event in events {
                        emit(&event);
                    }
                }
                TurnPoll::Observation { index, packet } => observe(index, packet)?,
                TurnPoll::Job(job) => {
                    remaining -= 1;
                    turn.complete_job(job.run()).map_err(|(error, _)| error)?;
                }
                TurnPoll::Waiting => {
                    return Err(
                        std::io::Error::other("turn requires an external wake or worker").into(),
                    );
                }
                TurnPoll::Complete => return Ok(()),
            }
        }
    })();
    result.map_err(|error| turn.cleanup_error(error))
}
