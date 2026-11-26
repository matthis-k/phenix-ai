use phenix_agent::{AgentEvent, EVENTS, PROMPT, mock};
use phenix_graph::{InputKey, OutputKey, runtime::RequestStatus};

fn main() -> Result<(), phenix_kernel::CallbackError> {
    let host = mock::host(Default::default(), Default::default())?;
    let generation = host.active().expect("published mock generation");
    let prompt = std::env::args().nth(1).unwrap_or_else(|| "hello".into());
    let ingress = generation
        .compiled()
        .bind_ingress(PROMPT, InputKey::<String>::new(PROMPT))?;
    let capture = generation
        .compiled()
        .bind_capture(EVENTS, OutputKey::<AgentEvent>::new(EVENTS))?;
    generation.with_session(|session| -> Result<(), phenix_kernel::CallbackError> {
        let request = session.begin_bound_request(None)?;
        session.inject_bound_request(request, &ingress, prompt)?;
        session.close_request(request)?;
        for _ in 0..64 {
            session.run_until_idle(1);
            if let Some(failure) = session.failure() {
                return Err(std::io::Error::other(failure.summary()).into());
            }
            while let Some(packet) = session.take_bound_request_output(request, &capture)? {
                session.validate_bound_capture(request, &capture, &packet)?;
                let event = packet.value().borrow::<AgentEvent>().expect("typed event");
                println!("{event:?}");
            }
            if matches!(
                session.request_status(request),
                Ok(RequestStatus::Settled { .. })
            ) {
                session.release_request(request)?;
                return Ok(());
            }
        }
        Err(std::io::Error::other("mock turn did not settle").into())
    })??;
    Ok(())
}
