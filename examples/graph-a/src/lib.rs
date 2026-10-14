use example_graph_contracts::{SlowTask, Start, Task};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-a",
    events: &[],
    actions: &[],
    nodes: &[
        StaticGraphNode {
            id: "a",
            triggers: &["example.graph.partial", "example.graph.start"],
            after: &[],
            selector: "",
            operation: "example.graph.task@1/run",
            writes: &["example.graph-a/result", "example.graph-a/signal"],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::Skip,
        },
        StaticGraphNode {
            id: "slow",
            triggers: &["example.graph.slow"],
            after: &["example.graph-a/a"],
            selector: "",
            operation: "example.graph.slow@1/run",
            writes: &["example.graph-a/result"],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
    ],
};

#[plugin(
    id = "example.graph-a",
    define(Task),
    define(SlowTask),
    provide(Task, slot = "task"),
    provide(SlowTask, slot = "slow"),
    profile = "state",
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[derive(Default)]
    struct State {
        #[get(id = "result")]
        #[set]
        #[watch]
        result: String,
        #[get]
        #[set]
        #[watch]
        signal: String,
        input: Option<Start>,
        polls: u64,
        cancelled: bool,
        worker: Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)>,
    }

    #[provide(Task::run)]
    fn run(state: &mut State, host: &live::Host<'_>, input: Start) -> CallResult<String, String> {
        example_graph_contracts::check_resource(host, &input)?;
        if input.text == "domain" {
            return Err(CallError::Domain("domain fixture".into()));
        }
        let mut edit = state
            .result
            .edit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        *edit = format!("A:{}", input.text);
        edit.commit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        if input.text == "g4-hook" {
            let mut signal = state
                .signal
                .edit(host)
                .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
            *signal = "committed".into();
            signal
                .commit(host)
                .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        }
        Ok(input.text)
    }

    #[pending(SlowTask::run)]
    fn slow(state: &mut State, input: Start) -> live::PendingResult<String, String> {
        let (stop, wait) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            let file = std::fs::File::open("/dev/null").unwrap();
            ready.send(()).unwrap();
            wait.recv().unwrap();
            drop(file);
        });
        started.recv().unwrap();
        state.worker = Some((stop, worker));
        state.input = Some(input);
        state.polls = 0;
        state.cancelled = false;
        live::PendingResult::Pending
    }

    fn drain(state: &mut State) {
        if let Some((stop, worker)) = state.worker.take() {
            stop.send(()).unwrap();
            worker.join().unwrap();
        }
    }

    #[poll(SlowTask::run)]
    fn poll(state: &mut State, host: &live::Host<'_>) -> live::PendingResult<String, String> {
        state.polls += 1;
        if state.cancelled {
            assert!(
                state.result.edit(host).is_err(),
                "cancelled cleanup regained write authority"
            );
        }
        if state.polls < 3 {
            return live::PendingResult::Pending;
        }
        drain(state);
        let input = state.input.take().unwrap();
        live::PendingResult::Ready(if state.cancelled {
            Ok("drained".into())
        } else {
            run(state, host, input)
        })
    }

    #[cancel(SlowTask::run)]
    fn cancel(state: &mut State) -> Result<bool, Failure> {
        state.cancelled = true;
        if state
            .input
            .as_ref()
            .is_some_and(|input| input.text == "slow-drain")
        {
            Ok(false)
        } else {
            drain(state);
            state.input = None;
            Ok(true)
        }
    }
}

#[cfg(target_os = "linux")]
#[used]
#[link_section = ".init_array"]
static GRAPH_CONSTRUCTOR_WITNESS: extern "C" fn() = graph_constructor;

#[cfg(target_os = "linux")]
extern "C" fn graph_constructor() {
    if let Some(path) = std::env::var_os("PHENIX_GRAPH_CONSTRUCTOR_WITNESS") {
        let _ = std::fs::write(path, b"native code loaded");
    }
}
