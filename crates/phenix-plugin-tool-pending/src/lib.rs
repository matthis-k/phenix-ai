//! Controlled native fixture. Each feature variant is a distinct provider instance.
#[cfg(not(feature = "alternate"))]
const TOOL: &str = "pending-a";
#[cfg(feature = "alternate")]
const TOOL: &str = "pending-b";
#[cfg(not(feature = "alternate"))]
const CONTROL: &str = "PHENIX_TOOL_A_CONTROL";
#[cfg(feature = "alternate")]
const CONTROL: &str = "PHENIX_TOOL_B_CONTROL";

#[cfg_attr(
    not(feature = "alternate"),
    phenix_sdk::plugin(
        id = "phenix.tool.pending-a",
        version = "1.0.0",
        provide(phenix_basic_contracts::ToolGraphExecutor, slot = "GraphExecutor"),
        require(phenix_basic_contracts::ToolReceipts, slot = "Receipts"),
        require(phenix_basic_contracts::Storage, slot = "StorageAuthority", optional)
    )
)]
#[cfg_attr(
    feature = "alternate",
    phenix_sdk::plugin(
        id = "phenix.tool.pending-b",
        version = "1.0.0",
        provide(phenix_basic_contracts::ToolGraphExecutor, slot = "GraphExecutor"),
        require(phenix_basic_contracts::ToolReceipts, slot = "Receipts"),
        require(phenix_basic_contracts::Storage, slot = "StorageAuthority", optional)
    )
)]
mod plugin {
    use phenix_basic_contracts::{
        error, BasicError, ErrorCode, GraphToolFailure, GraphToolInvocation, GraphToolSuccess,
        ToolGraphExecutor, ToolInfo,
    };
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host, PendingResult};
    use std::sync::mpsc::{channel, sync_channel, Sender};
    use std::thread::JoinHandle;

    #[derive(Default)]
    pub struct State {
        worker: Option<(Sender<()>, JoinHandle<()>)>,
        invocation: Option<GraphToolInvocation>,
        cancelled: bool,
        witness: Option<std::path::PathBuf>,
    }

    fn control() -> String {
        std::env::var_os(format!("{}_FILE", super::CONTROL))
            .map(|path| {
                std::fs::read_to_string(path)
                    .expect("tool control file")
                    .trim()
                    .to_owned()
            })
            .unwrap_or_else(|| std::env::var(super::CONTROL).unwrap_or_else(|_| "release".into()))
    }

    fn witness(state: &State, extension: &str) {
        if let Some(path) = &state.witness {
            let temporary = path.with_extension(format!("{extension}.tmp"));
            std::fs::write(
                &temporary,
                serde_json::to_vec(state.invocation.as_ref().unwrap()).unwrap(),
            )
            .unwrap();
            std::fs::rename(temporary, path.with_extension(extension)).unwrap();
        }
    }

    fn drain(state: &mut State) {
        let (stop, worker) = state.worker.take().expect("physical tool worker");
        stop.send(()).unwrap();
        worker.join().unwrap();
        if let Some(path) = &state.witness {
            std::fs::remove_file(path.with_extension("pending")).unwrap();
        }
    }

    #[provide(ToolGraphExecutor::list)]
    fn list() -> CallResult<Vec<ToolInfo>, BasicError> {
        Ok(vec![ToolInfo {
            id: super::TOOL.into(),
            description: "Controlled independent native tool".into(),
            arguments_schema: r#"{"type":"string"}"#.into(),
            idempotent: false,
        }])
    }

    #[pending(ToolGraphExecutor::invoke)]
    fn begin(
        state: &mut State,
        host: &Host<'_>,
        invocation: GraphToolInvocation,
    ) -> PendingResult<GraphToolSuccess, GraphToolFailure> {
        if let Err(failure) = phenix_basic_contracts::check_session(host, &invocation.session) {
            return match failure {
                CallError::Kernel(failure) => PendingResult::QuiescentFailure(failure),
                CallError::Domain(error) => {
                    PendingResult::Ready(Err(CallError::Domain(GraphToolFailure {
                        invocation: Box::new(invocation),
                        error,
                    })))
                }
            };
        }
        if invocation.run.is_empty()
            || invocation.batch == 0
            || invocation.call.id.is_empty()
            || invocation.call.tool != super::TOOL
            || serde_json::from_str::<String>(&invocation.call.arguments).is_err()
        {
            return PendingResult::Ready(Err(CallError::Domain(GraphToolFailure {
                invocation: Box::new(invocation),
                error: error(ErrorCode::InvalidInput, "invalid controlled invocation"),
            })));
        }
        assert!(
            state.worker.is_none(),
            "overlapping physical calls to the same tool provider"
        );
        let (stop, wait) = channel();
        let (ready, started) = sync_channel(0);
        let worker = std::thread::spawn(move || {
            let physical = std::fs::File::open("/dev/null").unwrap();
            ready.send(()).unwrap();
            let _ = wait.recv();
            drop(physical);
        });
        started.recv().unwrap();
        state.worker = Some((stop, worker));
        state.invocation = Some(invocation);
        state.cancelled = false;
        state.witness =
            std::env::var_os(format!("{}_FILE", super::CONTROL)).map(std::path::PathBuf::from);
        witness(state, "pending");
        PendingResult::Pending
    }

    #[provide(ToolGraphExecutor::validate)]
    fn validate(host: &Host<'_>, invocation: GraphToolInvocation) -> CallResult<(), BasicError> {
        phenix_basic_contracts::check_session(host, &invocation.session)?;
        if invocation.run.is_empty()
            || invocation.batch == 0
            || invocation.call.id.is_empty()
            || invocation.call.tool != super::TOOL
            || serde_json::from_str::<String>(&invocation.call.arguments).is_err()
        {
            return Err(CallError::Domain(error(
                ErrorCode::InvalidInput,
                "invalid controlled invocation",
            )));
        }
        Ok(())
    }

    #[poll(ToolGraphExecutor::invoke)]
    fn poll(
        state: &mut State,
        host: &Host<'_>,
    ) -> PendingResult<GraphToolSuccess, GraphToolFailure> {
        let control = control();
        if state.cancelled {
            if control != "drain" {
                return PendingResult::Pending;
            }
            witness(state, "drained");
            drain(state);
            return PendingResult::Ready(Ok(GraphToolSuccess {
                invocation: state.invocation.take().unwrap(),
                text: "forbidden late success".into(),
            }));
        }
        if control == "hold" {
            return PendingResult::Pending;
        }
        if control == "fail" {
            witness(state, "failed");
            drain(state);
            state.invocation = None;
            return PendingResult::QuiescentFailure(Failure::new(
                Code::PluginFailed,
                "controlled tool infrastructure failure",
            ));
        }
        witness(state, "completed");
        drain(state);
        let invocation = state.invocation.as_ref().unwrap().clone();
        let input: String = serde_json::from_str(&invocation.call.arguments).unwrap();
        let outcome = if input == "fail" {
            phenix_basic_contracts::GraphToolOutcome::Err {
                invocation,
                error: error(ErrorCode::ToolFailed, "controlled domain failure"),
            }
        } else {
            phenix_basic_contracts::GraphToolOutcome::Ok {
                invocation,
                text: format!("{}:{input}", super::TOOL),
            }
        };
        let result = phenix_basic_contracts::complete_graph_tool(host, outcome);
        if matches!(&result, Ok(_) | Err(CallError::Domain(_))) {
            witness(state, "receipted");
        }
        state.invocation = None;
        match result {
            Err(CallError::Kernel(failure)) => PendingResult::QuiescentFailure(failure),
            result => PendingResult::Ready(result),
        }
    }

    #[cancel(ToolGraphExecutor::invoke)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        state.cancelled = true;
        witness(state, "cancelled");
        Ok(false)
    }
}
