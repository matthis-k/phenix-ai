use example_contracts::{PendingConsumer, PendingRun, Text, Transform, TransformError};
use phenix_sdk::{
    Authority, Code, Failure, ImportCall, ImportPoll, PendingHost, PendingImport, PendingResult,
    ScopedRequest,
};
#[phenix_sdk::plugin(id = "example.pending-consumer", version = "1.0.0")]
mod plugin {
    use super::*;
    #[import]
    type Worker = PendingRun;
    #[import]
    type TransitiveEffect = Transform;
    #[derive(Default)]
    pub struct State {
        child: Option<PendingImport<Worker>>,
        mode: String,
    }
    #[pending(PendingConsumer)]
    fn begin(
        state: &mut State,
        host: &PendingHost<'_>,
        request: Text,
    ) -> PendingResult<Text, TransformError> {
        if request.text == "wrong-owner" {
            let old = state.child.as_mut().unwrap();
            for _ in 0..2 {
                assert_eq!(old.poll(host).err().unwrap().code, Code::ScopeRejected);
            }
            state.child = None;
            return PendingResult::Ready(Ok(Text {
                text: "owner-rejected".into(),
            }));
        }
        state.mode = request.text.clone();
        let child_request = Text {
            text: if request.text == "early" || request.text == "cancel-tree" {
                "stubborn".into()
            } else {
                request.text
            },
        };
        let call = if state.mode == "attenuated" {
            host.begin_scoped::<Worker>(
                &ScopedRequest {
                    authority: Authority {
                        operations: ["example.pending@1/run".into()].into(),
                        resources: Default::default(),
                    },
                    resource: None,
                },
                &child_request,
            )
        } else {
            host.begin::<Worker>(&child_request)
        };
        match call {
            Err(f) => PendingResult::Ready(Err(f.into())),
            Ok(ImportCall::Ready(result)) => PendingResult::Ready(result),
            Ok(ImportCall::Pending(child)) => {
                state.child = Some(child);
                if state.mode == "early" {
                    return PendingResult::Ready(Ok(Text {
                        text: "invalid".into(),
                    }));
                }
                if state.mode == "store" {
                    assert!(state.child.as_ref().unwrap().cancel(host).unwrap());
                    return PendingResult::Ready(Ok(Text {
                        text: "stored".into(),
                    }));
                }
                PendingResult::Pending
            }
        }
    }
    #[poll(PendingConsumer)]
    fn poll(state: &mut State, host: &PendingHost<'_>) -> PendingResult<Text, TransformError> {
        let child = state.child.as_mut().unwrap();
        match child.poll(host) {
            Ok(ImportPoll::Ready(result)) => {
                assert_eq!(child.poll(host).err().unwrap().code, Code::ScopeRejected);
                PendingResult::Ready(result)
            }
            Ok(ImportPoll::Cancelled) => {
                PendingResult::Ready(Err(
                    Failure::new(Code::PluginFailed, "child cancelled").into()
                ))
            }
            Ok(ImportPoll::Pending | ImportPoll::Draining) => PendingResult::Pending,
            Err(f) => PendingResult::Ready(Err(f.into())),
        }
    }
    #[cancel(PendingConsumer)]
    fn cancel(_: &mut State, _: &PendingHost<'_>) -> Result<bool, Failure> {
        Ok(true)
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), Failure> {
        state.child = None;
        Ok(())
    }
}
