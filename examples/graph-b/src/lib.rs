use example_graph_contracts::{
    Consume, JoinConsumer, JoinRequest, ManyConsumer, NestedOptionalConsumer, OptionalConsumer,
    ReferenceConsumer, Start, Task,
};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-b",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "b",
        triggers: &["example.graph.slow", "example.graph.start"],
        after: &[],
        selector: "",
        operation: "example.graph.task@1/run",
        writes: &["example.graph-b/result"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[plugin(
    id = "example.graph-b",
    define(Task),
    define(Consume),
    define(JoinConsumer),
    define(ReferenceConsumer),
    define(OptionalConsumer),
    define(ManyConsumer),
    define(NestedOptionalConsumer),
    provide(Task, slot = "task"),
    provide(Consume, slot = "consume"),
    provide(JoinConsumer, slot = "join_consumer"),
    provide(ReferenceConsumer, slot = "reference_consumer"),
    provide(OptionalConsumer, slot = "optional_consumer"),
    provide(ManyConsumer, slot = "many_consumer"),
    provide(NestedOptionalConsumer, slot = "nested_optional_consumer"),
    profile = "state",
    graph(GRAPH)
)]
mod provider {
    use super::*;
    fn readiness_witness() {
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_B_READINESS_WITNESS") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "called"
            )
            .unwrap();
        }
    }

    #[provide(OptionalConsumer::run)]
    fn optional_consumer(
        state: &mut State,
        host: &live::Host<'_>,
        input: Option<String>,
    ) -> CallResult<String, String> {
        // Delivery must not restore the broader root's B property authority.
        assert!(matches!(
            state.result.read(host),
            Err(live::AccessError::State(ValueAccessError::Denied))
        ));
        assert!(matches!(
            state.result.edit(host),
            Err(live::AccessError::State(ValueAccessError::Denied))
        ));
        readiness_witness();
        Ok(format!("optional:{input:?}"))
    }

    #[provide(ManyConsumer::run)]
    fn many_consumer(_state: &mut State, input: Vec<String>) -> CallResult<String, String> {
        readiness_witness();
        Ok(format!("many:{input:?}"))
    }

    #[provide(NestedOptionalConsumer::run)]
    fn nested_optional_consumer(
        _state: &mut State,
        input: Option<Option<String>>,
    ) -> CallResult<String, String> {
        readiness_witness();
        Ok(format!("nested:{input:?}"))
    }

    #[provide(JoinConsumer::run)]
    fn join_consumer(_state: &mut State, input: JoinRequest) -> CallResult<String, String> {
        assert_eq!(input.fast, "join-value");
        assert_eq!(input.slow, "join-value");
        let joined = format!("joined:fast={};slow={}", input.fast, input.slow);
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_B_JOIN_WITNESS") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "{joined}"
            )
            .unwrap();
        }
        Ok(joined)
    }

    #[provide(ReferenceConsumer::run)]
    fn reference_consumer(
        _state: &mut State,
        host: &live::Host<'_>,
        input: live::ValueRef<String>,
    ) -> CallResult<String, String> {
        assert!(
            matches!(
                input.read(host),
                Err(live::AccessError::State(ValueAccessError::Denied))
            ),
            "reference delivery must not grant read authority"
        );
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_B_CONSUME_WITNESS") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "denied-read"
            )
            .unwrap();
        }
        Ok("denied-read".into())
    }

    #[provide(Consume::run)]
    fn consume(_state: &mut State, input: String) -> CallResult<String, String> {
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_B_CONSUME_WITNESS") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "called"
            )
            .unwrap();
        }
        Ok(format!("consumed:{input}"))
    }

    #[derive(Default)]
    struct State {
        #[get(id = "result")]
        #[set]
        #[watch]
        result: String,
    }

    #[provide(Task::run)]
    fn run(state: &mut State, host: &live::Host<'_>, input: Start) -> CallResult<String, String> {
        example_graph_contracts::check_resource(host, &input)?;
        if input.text == "kernel-b" {
            return Err(Failure::new(Code::PluginFailed, "kernel failure fixture b").into());
        }
        if input.text == "domain-b" {
            return Err(CallError::Domain("domain fixture b".into()));
        }
        let mut edit = state
            .result
            .edit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        *edit = format!("B:{}", input.text);
        edit.commit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        Ok(input.text)
    }
}
