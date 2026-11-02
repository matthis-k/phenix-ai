#[allow(dead_code)]
mod values {
    include!("../../native_values.rs");
}

#[phenix_sdk::plugin(
    id = "experiment.fixture",
    version = "1.0.0",
    define(dataflow_native_contracts::Prepare),
    define(dataflow_native_contracts::Service),
    define(dataflow_native_contracts::Complete),
    provide(dataflow_native_contracts::Prepare, slot = "Prepare"),
    provide(dataflow_native_contracts::Service, slot = "Service"),
    provide(dataflow_native_contracts::Complete, slot = "Complete")
)]
mod plugin {
    use dataflow_native_contracts::{Branch, Complete, Error, Message, Prepare, ResultValue, Service};
    use phenix_sdk::{CallResult, Failure, Host, PendingResult};
    use std::sync::mpsc;
    #[derive(Default)]
    pub struct State {
        worker: Option<std::thread::JoinHandle<dataflow_fixture::Message>>,
        stop: Option<mpsc::Sender<()>>,
    }
    fn finish(m: dataflow_fixture::Message) -> ResultValue {
        crate::values::native_result(m)
    }
    #[provide(Prepare::run)]
    fn prepare(_: &mut State, _: &Host<'_>, request: Message) -> CallResult<Branch, Error> {
        let m = crate::values::common_message(request);
        let p = dataflow_fixture::prepare(m, &Default::default());
        Ok(crate::values::native_branch(p.0))
    }
    #[pending(Service::run)]
    fn service(
        state: &mut State,
        host: &Host<'_>,
        request: Message,
    ) -> PendingResult<ResultValue, Error> {
        let m = crate::values::common_message(request);
        if !m.pending {
            return PendingResult::Ready(Ok(finish(m)));
        }
        assert!(state.worker.is_none());
        let dir = std::path::PathBuf::from(
            std::env::var_os("PHENIX_EXPERIMENT_GATE").expect("controlled native pending gate"),
        );
        let gate = dir.join(format!("{}-{}", m.id.root, m.id.occurrence));
        let drained = gate.with_extension("drained");
        let permit = host.wake_permit();
        let (tx, rx) = mpsc::channel();
        state.stop = Some(tx);
        state.worker = Some(std::thread::spawn(move || {
            while !gate.exists() && matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            std::fs::write(drained, b"physical exit").unwrap();
            let _ = permit.wake();
            m
        }));
        PendingResult::Pending
    }
    #[poll(Service::run)]
    fn poll(state: &mut State, _: &Host<'_>) -> PendingResult<ResultValue, Error> {
        if !state.worker.as_ref().unwrap().is_finished() {
            return PendingResult::Pending;
        }
        let m = state.worker.take().unwrap().join().unwrap();
        state.stop.take();
        PendingResult::Ready(Ok(finish(m)))
    }
    #[cancel(Service::run)]
    fn cancel(state: &mut State, _: &Host<'_>) -> Result<bool, Failure> {
        if let Some(stop) = state.stop.take() {
            let _ = stop.send(());
        }
        Ok(false)
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), Failure> {
        assert!(
            state.worker.is_none(),
            "native work must physically drain before unload"
        );
        Ok(())
    }
    #[provide(Complete::run)]
    fn complete(
        _: &mut State,
        _: &Host<'_>,
        request: ResultValue,
    ) -> CallResult<ResultValue, Error> {
        Ok(request)
    }
}
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH_V2: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph.bin"));
