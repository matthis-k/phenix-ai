use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Kernel, Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn binding(key: &str, provider: &str) -> SlotBinding {
    phenix_conformance::binding("example.agent", key, provider)
}
fn grant(caller: &str, key: &str) -> ScopedGrant {
    let s = selection(key, "");
    ScopedGrant {
        caller: caller.into(),
        interface: s.interface,
        operation: s.operation,
        resources: BTreeSet::new(),
    }
}
fn request(prompt: &str, max_turns: u64) -> Value {
    Value::map([
        ("prompt", Value::String(prompt.into())),
        ("max_turns", Value::U64(max_turns)),
    ])
}
fn response(answer: &str, model_turns: u64, tool_calls: u64) -> Outcome {
    Outcome::Success(Value::map([
        ("answer", Value::String(answer.into())),
        ("model_turns", Value::U64(model_turns)),
        ("tool_calls", Value::U64(tool_calls)),
    ]))
}
fn invoke(kernel: &Kernel, prompt: &str, limit: u64) -> Result<Outcome, phenix_contract::Failure> {
    let root = kernel.admit_default_root().unwrap();
    root.invoke(&request(prompt, limit))
}
fn count(kernel: &Kernel) -> u64 {
    let root = kernel
        .admit_root(selection(
            "example.tool.count@1/count",
            "example.agent-tool",
        ))
        .unwrap();
    let Outcome::Success(Value::U64(count)) = root.invoke(&Value::String(String::new())).unwrap()
    else {
        panic!("invalid tool count");
    };
    count
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 6, "AGENT MODEL ALTERNATE TOOL OUTPUT");
    let output = Path::new(&args[5]);
    fs::create_dir_all(output).unwrap();
    let run = "example.agent@1/run";
    let model = "example.model@1/complete";
    let tool = "example.tool.execute@1/execute";
    let audit = "example.tool.count@1/count";
    let composition = Composition {
        format: "phenix.composition.v2".into(),
        definitions: vec![],
        artifacts: args[1..5].to_vec(),
        entry: selection(run, "example.agent"),
        slots: vec![
            binding(model, "example.agent-model"),
            binding(tool, "example.agent-tool"),
        ],
        grants: vec![
            grant("root", run),
            grant("root", audit),
            grant("example.agent", model),
            grant("example.agent", tool),
        ],
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [run, model, tool, audit].map(str::to_owned).into(),
            resources: BTreeSet::new(),
        },
        resource: None,
    };
    fs::write(
        output.join("composition.json"),
        serde_json::to_vec_pretty(&composition).unwrap(),
    )
    .unwrap();
    let prepared = Prepared::prepare(composition.clone()).unwrap();
    let inspection = prepared.inspect();
    let mut reversed = composition.clone();
    reversed.artifacts.reverse();
    reversed.slots.reverse();
    reversed.grants.reverse();
    assert_eq!(Prepared::prepare(reversed).unwrap().inspect(), inspection);
    let kernel = prepared.load().unwrap();
    let mut cases = Vec::new();
    for (prompt, answer, turns, calls) in [
        ("phenix", "mock:PHENIX", 2, 1),
        ("repeat", "mock:REPEAT", 21, 20),
        ("tool-error", "recovered", 2, 1),
        ("final", "direct", 1, 0),
        ("empty-final", "", 1, 0),
        ("second session", "mock:SECOND SESSION", 2, 1),
    ] {
        let before = count(&kernel);
        assert_eq!(
            invoke(&kernel, prompt, 0).unwrap(),
            response(answer, turns, calls)
        );
        assert_eq!(count(&kernel) - before, calls);
        cases.push(serde_json::json!({"case":prompt,"turns":turns,"tool_calls":calls}));
    }
    for (prompt, error, calls, limit) in [
        ("model-error", "ModelFailed", 0, 0),
        ("unknown-tool", "UnknownTool", 0, 0),
        ("invalid-final", "InvalidReply", 0, 0),
        ("duplicate", "InvalidReply", 1, 0),
        ("phenix", "TurnLimit", 1, 1),
    ] {
        let before = count(&kernel);
        assert_eq!(
            invoke(&kernel, prompt, limit).unwrap(),
            Outcome::Domain(Value::String(error.into()))
        );
        assert_eq!(count(&kernel) - before, calls);
        cases.push(serde_json::json!({"case":prompt,"error":error,"tool_calls":calls}));
    }
    // A failed run leaves no request-local feedback in the next admitted root.
    assert_eq!(
        invoke(&kernel, "after failure", 0).unwrap(),
        response("mock:AFTER FAILURE", 2, 1)
    );
    assert_eq!(
        invoke(&kernel, "phenix", 2).unwrap(),
        response("mock:PHENIX", 2, 1)
    );
    drop(kernel);
    for denied_key in [model, tool] {
        let mut denied = composition.clone();
        denied
            .grants
            .retain(|g| !(g.caller == "example.agent" && g.key() == denied_key));
        let kernel = Prepared::prepare(denied).unwrap().load().unwrap();
        assert_eq!(invoke(&kernel, "phenix", 0).unwrap_err().code, Code::Denied);
        assert_eq!(count(&kernel), 0);
        assert_eq!(kernel.inspect()["denied_operations"], 1);
        cases.push(serde_json::json!({"case":"denied","operation":denied_key,"tool_calls":0}));
    }
    let mut alternate = composition.clone();
    alternate.slots[0].provider = "example.agent-alternate".into();
    let kernel = Prepared::prepare(alternate).unwrap().load().unwrap();
    assert_eq!(
        invoke(&kernel, "phenix", 0).unwrap(),
        response("alternate:phenix", 1, 0)
    );
    assert_eq!(count(&kernel), 0);
    cases.push(serde_json::json!({"case":"alternate-provider","tool_calls":0}));
    let report = serde_json::json!({
        "format":"phenix.a0.agent.v1",
        "cases":cases,
        "isolation_after_failure":true,
        "exact_limit_completion":true,
        "order_independent_inspection":true,
        "inspection":inspection,
    });
    fs::write(
        output.join("agent.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("A0 native agent preflight passed");
}
