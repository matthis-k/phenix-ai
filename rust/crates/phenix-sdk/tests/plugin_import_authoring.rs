use phenix_sdk::{
    Authority, Call, Emit, HasPhenixSchema, Host, Optional, PermissionId, Required,
    StaticComponentImports,
};

#[phenix_sdk::interface("fixture.models@1")]
struct Models;

#[derive(phenix_sdk::PhenixValue)]
struct Completed;

#[allow(dead_code)]
#[phenix_sdk::component]
struct Api {
    #[phenix(import, authority = Authority::new([PermissionId::parse("models.invoke").unwrap()]))]
    models: Required<Call<Models, String, String>>,
    #[phenix(import)]
    fallback_models: Optional<Call<Models, String, String>>,
    #[phenix(host, authority = Authority::new([PermissionId::parse("models.host").unwrap()]))]
    models_host: Host<Models>,
    #[phenix(event("fixture.completed"))]
    completed: Emit<Completed>,
}

#[allow(dead_code)]
#[phenix_sdk::plugin(
    id = "fixture.root-authority",
    authority = Authority::new([PermissionId::parse("root.invoke").unwrap()])
)]
struct RootAuthorityPlugin {
    #[phenix(import)]
    models: Required<Call<Models, String, String>>,
}

#[test]
fn component_fields_preserve_import_host_and_event_semantics() {
    let imports = <Api as StaticComponentImports>::imports();
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].interface.as_str(), "fixture.models@1");
    assert_eq!(imports[0].field, "models");
    assert!(imports[0].required);
    assert_eq!(
        imports[0].authority,
        Authority::new([PermissionId::parse("models.invoke").unwrap()])
    );
    assert_eq!(imports[1].interface.as_str(), "fixture.models@1");
    assert_eq!(imports[1].field, "fallback_models");
    assert!(!imports[1].required);
    assert_eq!(imports[1].authority, Authority::default());

    let hosts = <Api as StaticComponentImports>::hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].interface.as_str(), "fixture.models@1");
    assert_eq!(hosts[0].field, "models_host");
    assert_eq!(
        hosts[0].authority,
        Authority::new([PermissionId::parse("models.host").unwrap()])
    );

    let events = <Api as StaticComponentImports>::events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.as_str(), "fixture.completed");
    assert_eq!(events[0].field, "completed");
    assert!(events[0].payload_type.ends_with("::Completed"));
    assert_eq!(events[0].payload_schema, Completed::phenix_schema());
}

#[test]
fn plugin_root_component_inherits_plugin_maximum_authority() {
    let manifests =
        <RootAuthorityPlugin as phenix_sdk::StaticPluginDefinition>::component_manifests();
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].id.as_str(), "fixture.root-authority.root");
    assert_eq!(
        manifests[0].maximum_authority,
        Authority::new([PermissionId::parse("root.invoke").unwrap()])
    );
}
