use example_contracts::{PendingRun, Text, Transform, TransformError};
use phenix_sdk::{CallError, Code, Failure, PendingHost, PendingResult, WakePermit};
use std::{
    sync::{mpsc, Mutex},
    thread::JoinHandle,
};
static LATE: Mutex<Option<WakePermit>> = Mutex::new(None);
#[phenix_sdk::plugin(id = "example.pending", version = "1.0.0")]
mod plugin {
    use super::*;
    #[import]
    type Service = Transform;
    #[derive(Default)]
    pub struct State {
        worker: Option<JoinHandle<String>>,
        gate: Option<mpsc::Sender<()>>,
        mode: String,
        drain_polls: usize,
    }
    fn finish(state: &mut State) -> String {
        state.gate.take().unwrap().send(()).unwrap();
        state.worker.take().unwrap().join().unwrap()
    }
    #[start]
    fn start(_: &mut State) -> Result<(), Failure> {
        if let Ok(path) = std::env::var("PHENIX_PENDING_TRACE") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "start"
            )
            .unwrap();
        }
        Ok(())
    }
    #[pending(PendingRun)]
    fn begin(
        state: &mut State,
        host: &PendingHost<'_>,
        request: Text,
    ) -> PendingResult<Text, TransformError> {
        if request.text == "late" {
            assert!(!LATE.lock().unwrap().take().unwrap().wake());
            return PendingResult::Ready(Ok(Text {
                text: "late-rejected".into(),
            }));
        }
        state.mode = request.text.clone();
        state.drain_polls = 0;
        let permit = host.wake_permit();
        *LATE.lock().unwrap() = Some(permit);
        let (gate, receiver) = mpsc::channel();
        let (started, ready) = mpsc::channel();
        state.gate = Some(gate);
        state.worker = Some(std::thread::spawn(move || {
            let _resource = std::fs::File::open("/dev/null").unwrap();
            assert!(permit.wake());
            started.send(()).unwrap();
            receiver.recv().unwrap();
            request.text
        }));
        ready.recv().unwrap();
        if state.mode == "panic" {
            panic!("worker remains live after panic");
        }
        PendingResult::Pending
    }
    #[poll(PendingRun)]
    fn poll(state: &mut State, host: &PendingHost<'_>) -> PendingResult<Text, TransformError> {
        if state.mode == "stubborn" && state.drain_polls == 0 {
            state.drain_polls += 1;
            // Cancellation revoked all protected imports before this poll.
            let reply = host.host().invoke::<Service>(&Text {
                text: "denied".into(),
            });
            assert!(matches!(
                reply,
                Err(CallError::Kernel(Failure {
                    code: Code::ScopeRejected,
                    ..
                }))
            ));
            return PendingResult::Pending;
        }
        let text = finish(state);
        if state.mode == "error" {
            return PendingResult::Ready(Err(Failure::new(
                Code::PluginFailed,
                "explicit worker error",
            )
            .into()));
        }
        if state.mode == "stubborn" || state.mode == "panic" {
            return PendingResult::Ready(Ok(Text { text }));
        }
        PendingResult::Ready(host.host().invoke::<Service>(&Text { text }))
    }
    #[cancel(PendingRun)]
    fn cancel(state: &mut State, _: &PendingHost<'_>) -> Result<bool, Failure> {
        if state.mode == "stubborn" {
            return Ok(false);
        }
        finish(state);
        Ok(true)
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), Failure> {
        if let Ok(path) = std::env::var("PHENIX_PENDING_TRACE") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new().append(true).open(path).unwrap(),
                "stop"
            )
            .unwrap();
        }
        assert!(
            state.worker.is_none(),
            "kernel stopped an instance with live work"
        );
        Ok(())
    }
}
