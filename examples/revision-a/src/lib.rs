use example_contracts::{Revision, Text, TransformError};
use phenix_sdk::{Code, Failure, Host, PendingResult, WakePermit};
use std::{
    sync::{mpsc, Mutex},
    thread::JoinHandle,
};
const REVISION: &str = "A";
static LATE: Mutex<Option<WakePermit>> = Mutex::new(None);
#[phenix_sdk::plugin(
    id = "example.revision",
    version = "1.0.0",
    define(example_contracts::RevisionContract),
    provide(example_contracts::RevisionContract, slot = "Revision")
)]
mod plugin {
    use super::*;
    #[derive(Default)]
    pub struct State {
        worker: Option<JoinHandle<String>>,
        gate: Option<mpsc::Sender<()>>,
        stubborn: bool,
        polls: usize,
    }
    #[start]
    fn start(_: &mut State) -> Result<(), Failure> {
        if std::env::var("PHENIX_REVISION_START_FAIL").as_deref() == Ok(REVISION) {
            return Err(Failure::new(Code::PluginFailed, "revision startup failure"));
        }
        Ok(())
    }
    fn finish(state: &mut State) -> Text {
        state.gate.take().unwrap().send(()).unwrap();
        Text {
            text: state.worker.take().unwrap().join().unwrap(),
        }
    }
    #[pending(Revision)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        request: Text,
    ) -> PendingResult<Text, TransformError> {
        if request.text == "late" {
            assert!(!LATE.lock().unwrap().take().unwrap().wake());
            return PendingResult::Ready(Ok(Text {
                text: "late-rejected".into(),
            }));
        }
        state.stubborn = request.text == "stubborn";
        state.polls = 0;
        let permit = host.wake_permit();
        *LATE.lock().unwrap() = Some(permit);
        let (gate, rx) = mpsc::channel();
        let (started, ready) = mpsc::channel();
        state.gate = Some(gate);
        state.worker = Some(std::thread::spawn(move || {
            let _resource = std::fs::File::open("/dev/null").unwrap();
            assert!(permit.wake());
            started.send(()).unwrap();
            rx.recv().unwrap();
            format!("{}:{}", REVISION, request.text)
        }));
        ready.recv().unwrap();
        PendingResult::Pending
    }
    #[poll(Revision)]
    fn poll(state: &mut State, _: &Host<'_>) -> PendingResult<Text, TransformError> {
        if state.stubborn && state.polls == 0 {
            state.polls += 1;
            return PendingResult::Pending;
        }
        PendingResult::Ready(Ok(finish(state)))
    }
    #[cancel(Revision)]
    fn cancel(state: &mut State, _: &Host<'_>) -> Result<bool, Failure> {
        if state.stubborn {
            return Ok(false);
        }
        finish(state);
        Ok(true)
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), Failure> {
        assert!(state.worker.is_none(), "stop with live revision worker");
        Ok(())
    }
}
extern "C" fn witness() {
    if let Ok(path) = std::env::var("PHENIX_REVISION_LOAD_TRACE") {
        let _ = std::fs::write(path, REVISION);
    }
}
#[used]
#[link_section = ".init_array"]
static WITNESS: extern "C" fn() = witness;
