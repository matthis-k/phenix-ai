//! Native acceptance fixture only. No public Model request options are added.
#[phenix_sdk::plugin(
    id = "phenix.model.scripted",
    version = "1.0.0",
    provide(phenix_basic_contracts::Model, slot = "Model")
)]
mod plugin {
    use phenix_basic_contracts::{BasicError, Model, ModelReply, ModelRequest};
    use phenix_sdk::{CallError, Code, Failure, Host, PendingResult};
    use std::sync::mpsc::{channel, sync_channel, Sender};
    use std::thread::JoinHandle;

    #[derive(Default)]
    pub struct State {
        worker: Option<(Sender<()>, JoinHandle<()>)>,
        cancelled: bool,
        request: Option<ModelRequest>,
        pending: Option<std::path::PathBuf>,
    }

    fn drain(state: &mut State) {
        let (stop, worker) = state.worker.take().unwrap();
        stop.send(()).unwrap();
        worker.join().unwrap();
        if let Some(path) = state.pending.take() {
            std::fs::remove_file(path).expect("scripted Model pending witness");
        }
    }

    fn control() -> Option<String> {
        match std::env::var_os("PHENIX_SCRIPTED_MODEL_CONTROL_FILE") {
            Some(path) => Some(
                std::fs::read_to_string(path)
                    .expect("scripted Model control file")
                    .trim()
                    .to_owned(),
            ),
            None => std::env::var("PHENIX_SCRIPTED_MODEL_CONTROL").ok(),
        }
    }

    #[pending(Model::complete)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: ModelRequest,
    ) -> PendingResult<ModelReply, BasicError> {
        if let Err(error) = phenix_basic_contracts::check_session(host, &request.session) {
            return PendingResult::Ready(Err(error));
        }
        if control().is_none() {
            return PendingResult::Ready(
                super::super::complete(&request).map_err(CallError::Domain),
            );
        }
        assert!(state.worker.is_none());
        let (stop, wait) = channel();
        let (ready, started) = sync_channel(0);
        let worker = std::thread::spawn(move || {
            let file = std::fs::File::open("/dev/null").unwrap();
            ready.send(()).unwrap();
            let _ = wait.recv();
            drop(file);
        });
        started.recv().unwrap();
        state.worker = Some((stop, worker));
        state.cancelled = false;
        if let Some(path) = std::env::var_os("PHENIX_SCRIPTED_MODEL_CONTROL_FILE") {
            let path = std::path::PathBuf::from(path).with_extension("pending");
            let temporary = path.with_extension("pending.tmp");
            let initial = request
                .history
                .last()
                .is_some_and(|message| message.role() == phenix_basic_contracts::Role::User);
            // ModelStarted precedes deferred admission; witness the real worker
            // only after its startup handshake, before returning to the host.
            std::fs::write(&temporary, if initial { "initial" } else { "continuation" })
                .expect("scripted Model pending witness");
            std::fs::rename(&temporary, &path).expect("scripted Model pending witness");
            state.pending = Some(path);
        }
        state.request = Some(request);
        PendingResult::Pending
    }

    #[poll(Model::complete)]
    fn poll(state: &mut State) -> PendingResult<ModelReply, BasicError> {
        assert!(state.worker.is_some());
        let control = control();
        if state.cancelled {
            // Keep the physical worker until the harness explicitly permits
            // drain, regardless of how many cleanup polls the runtime performs.
            if control.as_deref() != Some("drain") {
                return PendingResult::Pending;
            }
            drain(state);
            state.request = None;
            // A late success must never resume Basic or become an answer.
            return PendingResult::Ready(Ok(ModelReply {
                text: "forbidden late answer".into(),
                calls: vec![],
                usage: Default::default(),
            }));
        }
        if control.as_deref() == Some("fail") {
            drain(state);
            state.request = None;
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "controlled scripted Model failure",
            ));
        }
        // Release only the first Model so an external host test can reliably
        // hold the continuation without racing the host's next poll.
        let initial = state
            .request
            .as_ref()
            .unwrap()
            .history
            .last()
            .is_some_and(|message| message.role() == phenix_basic_contracts::Role::User);
        if control.as_deref() == Some("release")
            || (control.as_deref() == Some("release-initial") && initial)
        {
            drain(state);
            let request = state.request.take().unwrap();
            return PendingResult::Ready(
                super::super::complete(&request).map_err(CallError::Domain),
            );
        }
        PendingResult::Pending
    }

    #[cancel(Model::complete)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        state.cancelled = true;
        Ok(false)
    }
}
