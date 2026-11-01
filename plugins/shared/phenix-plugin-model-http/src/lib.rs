//! Pending HTTP model. Worker I/O is separate from authenticated journal effects.
mod request;
use phenix_provider_protocols::stream;
#[cfg(test)]
mod control_tests;
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

#[derive(Default)]
pub(crate) enum ModelConfiguration {
    #[default]
    Preset,
    // The first Model call binds an install to one run, including its tool
    // continuations. A different run requires fresh controller authorization.
    Selected(
        std::collections::BTreeMap<String, (std::sync::Arc<request::Config>, Option<String>, u64)>,
    ),
}

impl ModelConfiguration {
    pub(crate) fn install(&mut self, session: String, config: request::Config, installation: u64) {
        let config = std::sync::Arc::new(config);
        match self {
            Self::Preset => {
                *self = Self::Selected(std::collections::BTreeMap::from([(
                    session,
                    (config, None, installation),
                )]));
            }
            Self::Selected(pending) => {
                pending.insert(session, (config, None, installation));
            }
        }
    }

    fn release(&mut self, session: &str, installation: u64) {
        if let Self::Selected(entries) = self {
            if entries
                .get(session)
                .is_some_and(|entry| entry.2 == installation)
            {
                entries.remove(session);
            }
        }
    }

    fn release_run(&mut self, session: &str, run: &str) {
        if let Self::Selected(entries) = self {
            if entries
                .get(session)
                .is_some_and(|entry| entry.1.as_deref() == Some(run))
            {
                entries.remove(session);
            }
        }
    }

    pub(crate) fn take_for_turn(
        &mut self,
        session: &str,
        run: &str,
    ) -> Result<std::sync::Arc<request::Config>, BasicError> {
        match self {
            Self::Preset => request::Config::load().map(std::sync::Arc::new),
            Self::Selected(pending) => {
                let (config, admitted_run, _) = pending
                    .get_mut(session)
                    .filter(|(_, admitted, _)| {
                        !run.is_empty() && admitted.as_deref().is_none_or(|value| value == run)
                    })
                    .ok_or_else(|| {
                        error(
                            ErrorCode::InvalidInput,
                            "selected HTTP model is not installed for session",
                        )
                    })?;
                *admitted_run = Some(run.into());
                Ok(config.clone())
            }
        }
    }
}

#[phenix_sdk::plugin(
    id = "phenix.model.http",
    version = "1.0.0",
    profile = "state",
    provide(phenix_basic_contracts::Model, slot = "Model"),
    provide(phenix_provider_contracts::HttpModelSession, slot = "ModelControl"),
    require(phenix_basic_contracts::EventJournal, slot = "Journal"),
    require(phenix_basic_contracts::Storage, slot = "Storage", optional)
)]
mod plugin {
    use phenix_basic_contracts::{
        BasicError, Event, EventJournal, EventKind, JournalAppend, Model, ModelReply, ModelRequest,
        Usage,
    };
    use phenix_provider_contracts::{
        HttpModelInstall, HttpModelRelease, HttpModelSession, ProviderError, ProviderErrorCode,
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
        configuration: super::ModelConfiguration,
        installation: u64,
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
        let config = state
            .configuration
            .take_for_turn(&request.session, &request.run)
            .map_err(CallError::Domain)?;
        let body = phenix_provider_protocols::request::encode_protocol_with_max_tokens(
            &request,
            &config.model,
            config.protocol,
            config.provider_max_tokens,
        )
        .map_err(|failure| {
            state
                .configuration
                .release_run(&request.session, &request.run);
            CallError::Domain(failure)
        })?;
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
            .map_err(|_| {
                state.configuration.release_run(&session, &run);
                CallError::Domain(super::failed("HTTP worker could not start"))
            })?;
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
    #[provide(HttpModelSession::install)]
    fn install(
        state: &mut State,
        host: &Host<'_>,
        input: HttpModelInstall,
    ) -> CallResult<u64, ProviderError> {
        let fail = |code, message: &str| {
            CallError::Domain(ProviderError {
                code,
                message: message.into(),
            })
        };
        let resource = phenix_basic_contracts::session_resource(input.session())
            .map_err(|_| fail(ProviderErrorCode::InvalidInput, "invalid selected session"))?;
        if host.resource()?.as_deref() != Some(resource.as_str()) {
            return Err(fail(
                ProviderErrorCode::Denied,
                "model session resource denied",
            ));
        }
        if state.worker.is_some() {
            return Err(fail(
                ProviderErrorCode::Conflict,
                "model turn is already active",
            ));
        }
        let session = input.session().to_owned();
        let config = super::request::Config::from_install(input).map_err(|_| {
            fail(
                ProviderErrorCode::InvalidInput,
                "invalid HTTP model install",
            )
        })?;
        let installation = state.installation.checked_add(1).ok_or_else(|| {
            fail(
                ProviderErrorCode::Conflict,
                "HTTP installation identity exhausted",
            )
        })?;
        state.installation = installation;
        state.configuration.install(session, config, installation);
        Ok(installation)
    }

    #[provide(HttpModelSession::release)]
    fn release(
        state: &mut State,
        host: &Host<'_>,
        input: HttpModelRelease,
    ) -> CallResult<(), ProviderError> {
        let resource = phenix_basic_contracts::session_resource(&input.session).map_err(|_| {
            CallError::Domain(ProviderError {
                code: ProviderErrorCode::InvalidInput,
                message: "invalid selected session".into(),
            })
        })?;
        if host.resource()?.as_deref() != Some(resource.as_str()) {
            return Err(CallError::Domain(ProviderError {
                code: ProviderErrorCode::Denied,
                message: "model session resource denied".into(),
            }));
        }
        state
            .configuration
            .release(&input.session, input.installation);
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
                state
                    .configuration
                    .release_run(&worker.session, &worker.run);
                worker.failure = Some(failure);
                worker.cancelled.store(true, Ordering::Release);
            }
        }
        if !finished {
            return PendingResult::Pending;
        }
        let worker = state.worker.take().unwrap();
        let session = worker.session.clone();
        let run = worker.run.clone();
        if cancelled && worker.failure.is_none() {
            state.configuration.release_run(&session, &run);
            let _ = worker.thread.join();
            return PendingResult::Ready(Ok(ModelReply {
                text: String::new(),
                calls: vec![],
                usage: Usage::default(),
            }));
        }
        let result = joined(worker);
        if result.is_err() {
            state.configuration.release_run(&session, &run);
        }
        match result {
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
            result => PendingResult::Ready(result),
        }
    }
    #[cancel(Model::complete)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        let Some(worker) = state.worker.as_ref() else {
            return Ok(true);
        };
        state
            .configuration
            .release_run(&worker.session, &worker.run);
        worker.cancelled.store(true, Ordering::Release);
        if !worker.thread.is_finished() {
            return Ok(false);
        }
        let _ = state.worker.take().unwrap().thread.join();
        Ok(true)
    }

    #[test]
    fn cancellation_revokes_configuration_before_physical_worker_drain() {
        let mut state = State::default();
        state.configuration.install(
            "s1".into(),
            super::request::Config {
                endpoint: "http://127.0.0.1:11434/v1/chat/completions".into(),
                model: "selected".into(),
                protocol: phenix_provider_protocols::Protocol::OpenaiChat,
                provider_max_tokens: None,
                credential: Some(
                    phenix_provider_contracts::CredentialSecret::new(b"test-secret".to_vec())
                        .unwrap(),
                ),
                presentation: phenix_provider_contracts::ApiKeyPresentation::Bearer,
                ca_file: None,
            },
            1,
        );
        let config = state.configuration.take_for_turn("s1", "r1").unwrap();
        let weak = Arc::downgrade(&config);
        let (drain, wait) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _config = config;
            wait.recv().unwrap();
            drop(sender);
        });
        state.worker = Some(Worker {
            session: "s1".into(),
            run: "r1".into(),
            progress: Arc::new(Mutex::new(String::new())),
            cancelled: Arc::new(AtomicBool::new(false)),
            receiver,
            thread,
            failure: None,
        });
        assert!(!cancel(&mut state).unwrap());
        assert!(state.configuration.take_for_turn("s1", "r1").is_err());
        assert!(state.configuration.take_for_turn("s1", "r2").is_err());
        assert!(state
            .worker
            .as_ref()
            .unwrap()
            .cancelled
            .load(Ordering::Acquire));
        assert!(weak.upgrade().is_some());
        drain.send(()).unwrap();
        state.worker.take().unwrap().thread.join().unwrap();
        assert!(weak.upgrade().is_none());
        assert!(cancel(&mut state).unwrap());
    }
}
