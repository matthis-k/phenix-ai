//! Pending HTTP model. Worker I/O is separate from authenticated journal effects.
mod request;
use phenix_provider_protocols::stream;
#[cfg(test)]
mod tests;
mod transport;

use phenix_basic_contracts::{error, BasicError, ErrorCode};

fn invalid(message: &str) -> BasicError {
    error(ErrorCode::InvalidReply, message)
}
fn failed(message: &str) -> BasicError {
    error(ErrorCode::ModelFailed, message)
}

#[phenix_sdk::plugin(
    id = "phenix.model.http",
    version = "1.0.0",
    profile = "state",
    provide(phenix_basic_contracts::Model, slot = "Model"),
    require(phenix_basic_contracts::EventJournal, slot = "Journal"),
    require(phenix_basic_contracts::Storage, slot = "Storage", optional)
)]
mod plugin {
    use phenix_basic_contracts::{
        BasicError, Event, EventJournal, EventKind, JournalAppend, Model, ModelReply, ModelRequest,
        Usage,
    };
    use phenix_sdk::{Blob, CallError, CallResult, Code, Failure, Host, PendingResult};
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        },
        thread::JoinHandle,
    };

    struct Worker {
        session: String,
        run: String,
        progress: Arc<Mutex<String>>,
        cancelled: Arc<AtomicBool>,
        receiver: mpsc::Receiver<Result<ModelReply, BasicError>>,
        thread: JoinHandle<()>,
        failure: Option<CallError<BasicError>>,
    }
    #[derive(Default)]
    pub struct State {
        worker: Option<Worker>,
    }

    fn start(
        state: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &request.session)?;
        if state.worker.is_some() {
            return Err(CallError::Domain(phenix_basic_contracts::error(
                phenix_basic_contracts::ErrorCode::Conflict,
                "HTTP model work is already active",
            )));
        }
        let config = super::request::Config::load().map_err(CallError::Domain)?;
        let body = phenix_provider_protocols::request::encode_protocol(
            &request,
            &config.model,
            config.protocol,
        )
        .map_err(CallError::Domain)?;
        let session = request.session.clone();
        let run = request.run.clone();
        let progress = Arc::new(Mutex::new(String::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_progress = progress.clone();
        let worker_cancelled = cancelled.clone();
        let wake = host.wake_permit();
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("phenix-http-model".into())
            .spawn(move || {
                let result = super::transport::run(
                    config,
                    body,
                    request,
                    worker_progress,
                    worker_cancelled,
                    wake,
                );
                let _ = sender.send(result);
                wake.wake();
            })
            .map_err(|_| CallError::Domain(super::failed("HTTP worker could not start")))?;
        state.worker = Some(Worker {
            session,
            run,
            progress,
            cancelled,
            receiver,
            thread,
            failure: None,
        });
        Ok(())
    }
    #[pending(Model::complete)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> PendingResult<ModelReply, BasicError> {
        match start(state, host, request) {
            Ok(()) => PendingResult::Pending,
            Err(CallError::Domain(error)) => PendingResult::Ready(Err(CallError::Domain(error))),
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
        }
    }
    fn joined(worker: Worker) -> CallResult<ModelReply, BasicError> {
        let joined = worker.thread.join();
        if let Some(failure) = worker.failure {
            return Err(failure);
        }
        if joined.is_err() {
            return Err(CallError::Domain(super::failed("HTTP worker failed")));
        }
        worker
            .receiver
            .try_recv()
            .map_err(|_| CallError::Domain(super::failed("missing HTTP worker result")))?
            .map_err(CallError::Domain)
    }
    #[poll(Model::complete)]
    fn poll(state: &mut State, host: &Host<'_>) -> PendingResult<ModelReply, BasicError> {
        let Some(worker) = state.worker.as_mut() else {
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "missing HTTP worker",
            ));
        };
        // Finish only if the worker had stopped before taking progress. A worker
        // finishing during the journal call may still have queued its last delta.
        let finished = worker.thread.is_finished();
        let cancelled = worker.cancelled.load(Ordering::Acquire);
        if !cancelled && worker.failure.is_none() {
            let delta = match worker.progress.lock() {
                Ok(mut progress) => Ok(std::mem::take(&mut *progress)),
                Err(_) => Err(CallError::Domain(super::failed(
                    "HTTP progress queue failed",
                ))),
            };
            let result = delta.and_then(|text| {
                if text.is_empty() {
                    return Ok(());
                }
                host.invoke::<EventJournal::append>(
                    "Journal",
                    &JournalAppend {
                        session: worker.session.clone(),
                        events: vec![Event {
                            sequence: 0,
                            session: worker.session.clone(),
                            run: worker.run.clone(),
                            kind: EventKind::TextDelta,
                            call_id: String::new(),
                            text,
                            data: Blob(vec![]),
                        }],
                    },
                )
                .map(|_| ())
            });
            if let Err(failure) = result {
                worker.failure = Some(failure);
                worker.cancelled.store(true, Ordering::Release);
            }
        }
        if !finished {
            return PendingResult::Pending;
        }
        let worker = state.worker.take().unwrap();
        if cancelled && worker.failure.is_none() {
            let _ = worker.thread.join();
            return PendingResult::Ready(Ok(ModelReply {
                text: String::new(),
                calls: vec![],
                usage: Usage::default(),
            }));
        }
        match joined(worker) {
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
            result => PendingResult::Ready(result),
        }
    }
    #[cancel(Model::complete)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        let Some(worker) = state.worker.as_ref() else {
            return Ok(true);
        };
        worker.cancelled.store(true, Ordering::Release);
        if !worker.thread.is_finished() {
            return Ok(false);
        }
        let _ = state.worker.take().unwrap().thread.join();
        Ok(true)
    }
}
