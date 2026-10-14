use example_graph_contracts::{
    ChangeNotice, PendingSignalA, PendingStateHook, ResultA, ResultB, SignalA, Start, StateHook,
    Task,
};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-c",
    events: &[],
    actions: &[],
    nodes: &[
        StaticGraphNode {
            id: "c",
            triggers: &[
                "example.graph.partial",
                "example.graph.slow",
                "example.graph.start",
            ],
            after: &[
                "example.graph-a/a",
                "example.graph-a/slow",
                "example.graph-b/b",
            ],
            selector: "",
            operation: "example.graph.task@1/run",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "d",
            triggers: &["example.graph.slow", "example.graph.start"],
            after: &["example.graph-c/c"],
            selector: "",
            operation: "example.graph.task@1/run",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "pending",
            triggers: &["state:example.graph-a/pending_signal"],
            after: &[],
            selector: "",
            operation: "example.graph.pending-change@1/run",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "receipt",
            triggers: &["state:example.graph-c/receipt"],
            after: &["example.graph-c/watch"],
            selector: "",
            operation: "example.graph.changed@1/run",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "watch",
            triggers: &["state:example.graph-a/signal"],
            after: &[],
            selector: "",
            operation: "example.graph.changed@1/run",
            writes: &["example.graph-c/receipt"],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
    ],
};

#[plugin(
    id = "example.graph-c",
    define(Task),
    define(StateHook),
    define(PendingStateHook),
    provide(Task, slot = "task"),
    provide(StateHook, slot = "hook"),
    provide(PendingStateHook, slot = "pending-hook"),
    require(ResultA, slot = "a"),
    require(ResultB, slot = "b"),
    require(SignalA, slot = "signal"),
    require(PendingSignalA, slot = "pending_signal"),
    graph(GRAPH)
)]
mod provider {
    use super::*;

    #[derive(Default)]
    struct State {
        #[get]
        #[set]
        #[watch]
        receipt: String,
        pending_notice: Option<ChangeNotice>,
        pending_polls: u64,
        pending_cancelled: bool,
        pending_worker: Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)>,
    }

    #[provide(Task::run)]
    fn run(host: &live::Host<'_>, input: Start) -> CallResult<String, String> {
        example_graph_contracts::check_resource(host, &input)?;
        let first = ResultA::get::locate(host, "a")?;
        let second = ResultB::get::locate(host, "b")?;
        let a = first
            .read(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        let b = second
            .read(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        let observed = format!("{}|{}", a.value.as_ref(), b.value.as_ref());
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_C_WITNESS") {
            std::fs::write(path, observed.as_bytes()).expect("graph-c invocation witness");
        }
        Ok(observed)
    }

    #[provide(StateHook::run)]
    fn on_change(
        state: &mut State,
        host: &live::Host<'_>,
        change: ChangeNotice,
    ) -> CallResult<String, String> {
        assert!(change.epoch > 0);
        let _ = host.resource()?;
        let result = format!("{}:{}", change.epoch, change.properties.join(","));
        if change.properties == ["example.graph-a/signal"] {
            // Committing this watched field starts a second reactive epoch.
            let mut receipt = state
                .receipt
                .edit(host)
                .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
            *receipt = result.clone();
            receipt
                .commit(host)
                .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
            if let Some(path) = std::env::var_os("PHENIX_GRAPH_HOOK_WITNESS") {
                std::fs::write(path, result.as_bytes()).expect("graph state-hook witness");
            }
        } else {
            assert_eq!(change.properties, ["example.graph-c/receipt"]);
            if std::env::var_os("PHENIX_GRAPH_SELF_TRIGGER").is_some() {
                // A deliberately nonterminating state hook must not trap the
                // synchronous host entrypoint. Each edit opens a new epoch.
                let mut receipt = state
                    .receipt
                    .edit(host)
                    .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
                *receipt = result.clone();
                receipt
                    .commit(host)
                    .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
            }
            if let Some(path) = std::env::var_os("PHENIX_GRAPH_RECEIPT_WITNESS") {
                std::fs::write(path, result.as_bytes()).expect("graph receipt-hook witness");
            }
        }
        Ok(result)
    }

    #[pending(PendingStateHook::run)]
    fn wait(state: &mut State, change: ChangeNotice) -> live::PendingResult<String, String> {
        assert_eq!(change.properties, ["example.graph-a/pending_signal"]);
        let (stop, receive) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            let file = std::fs::File::open("/dev/null").unwrap();
            ready.send(()).unwrap();
            receive.recv().unwrap();
            drop(file);
        });
        started.recv().unwrap();
        state.pending_worker = Some((stop, worker));
        state.pending_notice = Some(change);
        state.pending_polls = 0;
        state.pending_cancelled = false;
        live::PendingResult::Pending
    }

    fn drain_pending(state: &mut State) {
        if let Some((stop, worker)) = state.pending_worker.take() {
            stop.send(()).unwrap();
            worker.join().unwrap();
        }
    }

    #[poll(PendingStateHook::run)]
    fn poll_pending(
        state: &mut State,
        host: &live::Host<'_>,
    ) -> live::PendingResult<String, String> {
        state.pending_polls += 1;
        if state.pending_cancelled {
            assert!(
                state.receipt.edit(host).is_err(),
                "cancelled pending hook recovered write authority"
            );
        }
        if state.pending_polls < 3 {
            return live::PendingResult::Pending;
        }
        drain_pending(state);
        state.pending_notice.take();
        live::PendingResult::Ready(Ok("drained".into()))
    }

    #[cancel(PendingStateHook::run)]
    fn cancel_pending(state: &mut State) -> Result<bool, Failure> {
        state.pending_cancelled = true;
        if std::env::var_os("PHENIX_GRAPH_PENDING_DRAIN").is_some() {
            Ok(false)
        } else {
            drain_pending(state);
            state.pending_notice.take();
            Ok(true)
        }
    }
}
