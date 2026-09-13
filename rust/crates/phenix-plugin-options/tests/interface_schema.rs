use phenix_core::ComponentInterface;
use phenix_plugin_options::{options_component_manifest, OptionsInterface};

#[test]
fn public_options_interface_matches_generated_component_export() {
    let component = options_component_manifest();
    let export = component
        .exports
        .iter()
        .find(|export| export.interface == OptionsInterface::interface_id())
        .expect("options component exports the public options interface");

    assert_eq!(export.schema, OptionsInterface::schema());
}
