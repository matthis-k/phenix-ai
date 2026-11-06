//! The plugin's single source of graph topology. Shared with build.rs.
pub fn graph_patch() -> Result<phenix_sdk::Patch, phenix_sdk::Failure> {
    use phenix_sdk::{Edit, InputPort, InputRef, Node, Patch, Readiness, Schema, Trigger};
    let request = Schema::of(
        &<phenix_basic_contracts::PromptSubmitted as phenix_sdk::Portable>::SCHEMA.owned()?
    )?;
    #[cfg(feature = "staged-graph")]
    let (name, action, response) = (
        "prepare", "phenix.basic.turn-prepare@1/prepare",
        Schema::of(&<phenix_basic_contracts::ModelRequest as phenix_sdk::Portable>::SCHEMA.owned()?)?,
    );
    #[cfg(not(feature = "staged-graph"))]
    let (name, action, response) = (
        "submit", "phenix.basic.input@1/submit",
        Schema::of(&<phenix_basic_contracts::RunResult as phenix_sdk::Portable>::SCHEMA.owned()?)?,
    );
    let path = format!("phenix.basic.agent/{name}");
    let mut patch = Patch::new("phenix.basic.agent");
    patch.push(Edit::AddNode(Node {
        path: path.clone(), action: action.into(),
        inputs: [("run".into(), InputPort {
            slots: [("input".into(), request)].into(),
            readiness: Readiness::All,
        })].into(),
        outputs: [("value".into(), response)].into(),
    }));
    patch.push(Edit::AddTrigger(Trigger {
        name: "phenix.basic.prompt".into(),
        to: InputRef { node: path, port: "run".into(), slot: "input".into() },
    }));
    patch.validate()?;
    Ok(patch)
}

