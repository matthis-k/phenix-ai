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
    }

    fn drain(state: &mut State) {
        let (stop, worker) = state.worker.take().unwrap();
        stop.send(()).unwrap();
        worker.join().unwrap();
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
        if std::env::var_os("PHENIX_SCRIPTED_MODEL_CONTROL").is_none() {
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
        PendingResult::Pending
    }

    #[poll(Model::complete)]
    fn poll(state: &mut State) -> PendingResult<ModelReply, BasicError> {
        assert!(state.worker.is_some());
        if state.cancelled {
            // Keep the physical worker until the harness explicitly permits
            // drain, regardless of how many cleanup polls the runtime performs.
            if std::env::var("PHENIX_SCRIPTED_MODEL_CONTROL").as_deref() != Ok("drain") {
                return PendingResult::Pending;
            }
            drain(state);
            // A late success must never resume Basic or become an answer.
            return PendingResult::Ready(Ok(ModelReply {
                text: "forbidden late answer".into(),
                calls: vec![],
                usage: Default::default(),
            }));
        }
        if std::env::var("PHENIX_SCRIPTED_MODEL_CONTROL").as_deref() == Ok("fail") {
            drain(state);
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "controlled scripted Model failure",
            ));
        }
        PendingResult::Pending
    }

    #[cancel(Model::complete)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        state.cancelled = true;
        Ok(false)
    }
}
