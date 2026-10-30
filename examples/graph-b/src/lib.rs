use example_graph_contracts::{
    Consume, JoinConsumer, JoinRequest, ManyConsumer, NestedOptionalConsumer, OptionalConsumer,
    ReferenceConsumer, SlowTask, Start, Task,
};
use phenix_sdk::{live, *};

#[derive(Debug, Clone)]
pub enum EachOutcome {
    Ok(String),
    Err(String),
}

impl Portable for EachOutcome {
    const SCHEMA: StaticSchema = StaticSchema::Tagged(&[
        StaticVariant {
            name: "Err",
            schema: Some(StaticSchema::String),
        },
        StaticVariant {
            name: "Ok",
            schema: Some(StaticSchema::String),
        },
    ]);

    fn to_value(&self) -> Value {
        match self {
            Self::Ok(value) => Value::map([("Ok", Value::String(value.clone()))]),
            Self::Err(value) => Value::map([("Err", Value::String(value.clone()))]),
        }
    }

    fn from_value(value: Value) -> Result<Self, Failure> {
        if let Value::Map(mut fields) = value {
            if fields.len() == 1 {
                let (tag, value) = fields.pop_first().unwrap();
                if let Value::String(value) = value {
                    return match tag.as_str() {
                        "Ok" => Ok(Self::Ok(value)),
                        "Err" => Ok(Self::Err(value)),
                        _ => Err(Failure::new(Code::SchemaRejected, "unknown each outcome")),
                    };
                }
            }
        }
        Err(Failure::new(Code::SchemaRejected, "invalid each outcome"))
    }
}

#[phenix_sdk::contract(id = "example.graph.each-collect@1")]
pub trait EachCollect {
    fn run(&self, input: Vec<EachOutcome>) -> Result<Vec<EachOutcome>, String>;
}

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
    define(SlowTask),
    provide(SlowTask, slot = "slow"),
    require(SlowTask, slot = "Workers", many(min = 0)),
    define(EachCollect),
    provide(EachCollect, slot = "each_collect"),
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
    #[provide(EachCollect::run)]
    fn each_collect(
        _state: &mut State,
        input: Vec<EachOutcome>,
    ) -> CallResult<Vec<EachOutcome>, String> {
        Ok(input)
    }
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
        each_input: Option<Start>,
        each_file: Option<std::fs::File>,
        each_cancelled: bool,
    }

    #[pending(SlowTask::run)]
    fn each_begin(state: &mut State, input: Start) -> live::PendingResult<String, String> {
        assert!(
            state.each_file.is_none(),
            "same provider admitted concurrently"
        );
        let path = std::env::var_os("PHENIX_GRAPH_B_EACH_CONTROL").unwrap();
        state.each_file = Some(std::fs::File::open("/dev/null").unwrap());
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::path::Path::new(&path).with_extension("calls"))
                .unwrap(),
            "{}",
            input.text
        )
        .unwrap();
        state.each_input = Some(input);
        state.each_cancelled = false;
        live::PendingResult::Pending
    }

    #[poll(SlowTask::run)]
    fn each_poll(state: &mut State) -> live::PendingResult<String, String> {
        let control =
            std::fs::read_to_string(std::env::var_os("PHENIX_GRAPH_B_EACH_CONTROL").unwrap())
                .unwrap();
        if !state.each_cancelled && control.trim() == "fail" {
            // A logical failure deliberately retains the physical resource.
            state.each_cancelled = true;
            return live::PendingResult::Ready(Err(Failure::new(
                Code::PluginFailed,
                "each held failure",
            )
            .into()));
        }
        if control.trim()
            != if state.each_cancelled {
                "drain"
            } else {
                "release"
            }
        {
            return live::PendingResult::Pending;
        }
        state.each_file.take();
        let input = state.each_input.take().unwrap();
        live::PendingResult::Ready(if state.each_cancelled {
            Ok("forbidden late output".into())
        } else if input.text == "domain" {
            Err(CallError::Domain("domain fixture b".into()))
        } else {
            Ok(input.text)
        })
    }

    #[cancel(SlowTask::run)]
    fn each_cancel(state: &mut State) -> Result<bool, Failure> {
        state.each_cancelled = true;
        Ok(false)
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
