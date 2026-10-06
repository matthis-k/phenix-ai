use super::*;

#[test]
fn explicit_workspace_selection_does_not_inject_local_environment() {
    let enabled = BTreeSet::from(["phenix.workspace".to_owned()]);
    let builder = PhenixRuntimeBuilder::with_selected_suite(&enabled)
        .expect("workspace is a known first-party plugin");
    let selected = builder
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();

    assert!(selected.contains("phenix.workspace"));
    assert!(
        !selected.contains("phenix.environment.local"),
        "explicit composition must not choose an Environment implementation"
    );
}
