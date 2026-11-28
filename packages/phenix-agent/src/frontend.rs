//! Native single-capture frontend driver. Protocol adapters with additional
//! captures must drain those as well. Frontend callbacks run outside the lease.

use crate::{AgentEvent, EVENTS, PROMPT};
use phenix_graph::{
    InputKey, OutputKey,
    runtime::{RequestId, RequestStatus},
};
use phenix_kernel::{CallbackError, host::Generation};

fn cleanup(generation: &Generation, request: RequestId) -> Result<(), CallbackError> {
    generation.with_session(|session| -> Result<(), CallbackError> {
        if !matches!(
            session.request_status(request)?,
            RequestStatus::Cancelled | RequestStatus::Failed | RequestStatus::Settled { .. }
        ) {
            session.cancel_request(request)?;
        }
        session.release_request(request)?;
        Ok(())
    })?
}

/// Drive one native request with a finite step budget. On returned errors,
/// cancel/release the request and report cleanup failure as well. This
/// synchronous driver is for immediate processes; a wake-driven frontend
/// should suspend instead of spending its budget polling deferred work.
/// A panicking callback is outside this helper's returned-error guarantee.
pub fn run_turn(
    generation: &Generation,
    prompt: String,
    budget: usize,
    mut emit: impl FnMut(&AgentEvent),
) -> Result<(), CallbackError> {
    let ingress = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))?;
    let capture = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))?;
    let request = generation.with_session(|session| session.begin_bound_request(None))??;
    let mut released = false;
    let result = (|| -> Result<(), CallbackError> {
        generation.with_session(|session| -> Result<(), CallbackError> {
            session.inject_bound_request(request, &ingress, prompt)?;
            session.close_request(request)?;
            Ok(())
        })??;
        let mut completed = false;
        for _ in 0..budget {
            let (events, settled) =
                generation.with_session(|session| -> Result<_, CallbackError> {
                    session.run_until_idle(1);
                    if let Some(failure) = session.failure() {
                        return Err(std::io::Error::other(failure.summary()).into());
                    }
                    if let Some(failure) = session.request_failure(request)? {
                        return Err(std::io::Error::other(failure.summary()).into());
                    }
                    let mut events = Vec::new();
                    while let Some(packet) = session.take_bound_request_output(request, &capture)? {
                        session.validate_bound_capture(request, &capture, &packet)?;
                        events.push(
                            packet
                                .value()
                                .borrow::<AgentEvent>()
                                .expect("typed capture")
                                .clone(),
                        );
                    }
                    let settled = matches!(
                        session.request_status(request)?,
                        RequestStatus::Settled { .. }
                    );
                    if settled {
                        session.release_request(request)?;
                    }
                    Ok((events, settled))
                })??;
            released = settled;
            for event in events {
                if completed {
                    return Err(std::io::Error::other("event after completion").into());
                }
                completed = event == AgentEvent::Completed;
                emit(&event);
            }
            if settled {
                return if completed {
                    Ok(())
                } else {
                    Err(std::io::Error::other("request settled without completion").into())
                };
            }
        }
        Err(std::io::Error::other("turn step budget exhausted").into())
    })();
    match result {
        Err(error) if !released => match cleanup(generation, request) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(std::io::Error::other(format!(
                "{error}; request cleanup failed: {cleanup_error}"
            ))
            .into()),
        },
        result => result,
    }
}
