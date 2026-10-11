//! Independent process loads SDK-only typed fixtures, with no linked plugin code.
use phenix_contract::{Authority, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding, StateComposition},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};
const PROVIDER: &str = "test.state-provider";
const CLIENT: &str = "test.state-client";
const COUNTER: &str = "test.state-provider.property.counter@1";
const NOTES: &str = "test.state-provider.property.notes@1";
fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn composition(provider: &str, client: &str, alternative: &str) -> StateComposition {
    let resources: BTreeSet<_> = [COUNTER, NOTES]
        .map(|s| s.trim_end_matches("@1").to_owned())
        .into();
    let mut grants = vec![];
    let mut operations = BTreeSet::new();
    for (caller, keys) in [
        (
            "root",
            vec![
                "test.state@1/run".into(),
                "test.state@1/read".into(),
                "test.refs@1/get".into(),
                "test.provider@1/run".into(),
                format!("{COUNTER}/get"),
            ],
        ),
        (
            CLIENT,
            vec![
                "test.refs@1/get".into(),
                "test.refs@1/notes".into(),
                format!("{COUNTER}/get"),
                format!("{COUNTER}/set"),
                format!("{COUNTER}/watch"),
                format!("{NOTES}/get"),
                format!("{NOTES}/set"),
                format!("{NOTES}/watch"),
            ],
        ),
    ] {
        for key in keys {
            let s = selection(&key, "");
            operations.insert(key);
            grants.push(ScopedGrant {
                caller: caller.into(),
                interface: s.interface,
                operation: s.operation,
                resources: resources.clone(),
            });
        }
    }
    let slot = |slot: &str, export: &str| SlotBinding {
        caller: CLIENT.into(),
        slot: slot.into(),
        index: 0,
        provider: PROVIDER.into(),
        export: export.into(),
    };
    StateComposition {
        format: "phenix.state-composition.v2".into(),
        definitions: vec![],
        slots: vec![
            slot("refs", "refs"),
            slot("counter", "property.counter"),
            slot("notes", "property.notes"),
            slot("many", "refs"),
            SlotBinding {
                caller: CLIENT.into(),
                slot: "many".into(),
                index: 1,
                provider: "test.state-alternative".into(),
                export: "refs".into(),
            },
        ],
        composition: Composition {
            format: "phenix.composition.v1".into(),
            artifacts: vec![provider.into(), client.into(), alternative.into()],
            entry: selection("test.state@1/run", CLIENT),
            bindings: vec![],
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            authority: Authority {
                operations,
                resources,
            },
            resource: None,
        },
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "PROVIDER CLIENT ALTERNATIVE OUTPUT");
    let output = Path::new(&args[4]);
    fs::create_dir_all(output).unwrap();
    let c = composition(&args[1], &args[2], &args[3]);
    let kernel = Prepared::prepare_state(c.clone()).unwrap().load().unwrap();
    assert_eq!(
        kernel
            .admit_default_root()
            .unwrap()
            .invoke(&Value::String("typed".into()))
            .unwrap(),
        Outcome::Success(Value::List([1, 2, 2, 1].map(Value::U64).into()))
    );
    let before = kernel.inspect();
    assert_eq!(before["live_state"]["subscriptions"], 0);
    assert_eq!(before["live_state"]["epoch"], 2);
    kernel.shutdown().unwrap();
    assert_eq!(kernel.inspect()["live_state"]["cells"], 0);
    let mut denied = c.clone();
    denied.composition.grants.retain(|g| g.operation != "set");
    let kernel = Prepared::prepare_state(denied).unwrap().load().unwrap();
    assert_eq!(
        kernel
            .admit_default_root()
            .unwrap()
            .invoke(&Value::String("denied".into()))
            .unwrap(),
        Outcome::Success(Value::List(vec![Value::U64(1)]))
    );
    assert_eq!(kernel.inspect()["live_state"]["epoch"], 0);
    kernel.shutdown().unwrap();
    fs::write(
        output.join("composition.json"),
        serde_json::to_vec_pretty(&c).unwrap(),
    )
    .unwrap();
    fs::write(output.join("sdk.json"),serde_json::to_vec_pretty(&serde_json::json!({"format":"phenix.m1-value-sdk-evidence.v1","cases":["typed-property-binding","selected-slot-calls","typed-ref-schema-mismatch","immutable-snapshot-clones","explicit-vec-edit","cas-domain-conflict-details","draft-drop-zero-effects","heterogeneous-batch-one-epoch","watch-bootstrap-and-coalescing","explicit-idempotent-watch-close","consistent-epoch-snapshot","selected-many-ref-grants-no-authority","denied-write-zero-effects","root-stop-cell-cleanup"],"before_stop":before,"all_passed":true})).unwrap()).unwrap();
    println!("S3a typed native gate passed");
}
