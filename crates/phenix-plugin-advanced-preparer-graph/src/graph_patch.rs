//! The Advanced preparer contributes a portable graph patch.
use phenix_advanced_contracts::PreparationReceipt;
use phenix_basic_contracts::GraphStateChange;
use phenix_sdk::Failure;
pub fn graph_patch() -> Result<phenix_sdk::Patch, Failure> {
    use phenix_sdk::{Edit, InputPort, InputRef, Node, Readiness, Schema, Trigger};
    let input = Schema::of(&<GraphStateChange as phenix_sdk::Portable>::SCHEMA.owned()?)?;
    let prepare_out = Schema::of(&<PreparationReceipt as phenix_sdk::Portable>::SCHEMA.owned()?)?;
    let advance_out = Schema::of(
        &<phenix_basic_contracts::TurnAdvanceOutcome as phenix_sdk::Portable>::SCHEMA.owned()?
    )?;
    let mut p = phenix_sdk::Patch::new("phenix.advanced.graph-preparer")
        .after("phenix.basic.agent");
    for (name, action, output, trigger) in [
        ("prepare", "phenix.advanced.model-preparation-change@1/run",
         prepare_out, "state:phenix.basic.agent/pending_model_request"),
        ("advance", "phenix.basic.turn-advance@1/advance",
         advance_out, "state:phenix.advanced.graph-preparer/prepared_model_request"),
    ] {
        let path = format!("phenix.advanced.graph-preparer/{name}");
        p.push(Edit::AddNode(Node {
            path: path.clone(), action: action.into(),
            inputs: [("run".into(), InputPort {
                slots: [("notification".into(), input.clone())].into(),
                readiness: Readiness::All,
            })].into(),
            outputs: [("value".into(), output)].into(),
        }));
        p.push(Edit::AddTrigger(Trigger {
            name: trigger.into(),
            to: InputRef { node: path, port: "run".into(), slot: "notification".into() },
        }));
    }
    p.validate()?;
    Ok(p)
}

