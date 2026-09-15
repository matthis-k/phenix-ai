mod sessions {
    #[phenix_sdk::plugin("fixture.manifest.sessions")]
    pub struct Plugin;
}

#[phenix_sdk::interface("fixture.manifest.run@1")]
struct Run;

#[derive(phenix_sdk::PhenixValue)]
struct Request;

#[derive(phenix_sdk::PhenixValue)]
struct Response;

#[phenix_sdk::component]
struct Api;

#[allow(dead_code)]
#[phenix_sdk::component]
impl Api {
    #[phenix(export(Run), terminal, priority = 7)]
    fn run(&self, _request: Request) -> Response {
        Response
    }
}

struct Store;

#[phenix_sdk::resource(schema = 2)]
impl Store {}

#[allow(dead_code)]
#[phenix_sdk::plugin(
    id = "fixture.manifest",
    authority = phenix_sdk::Authority::new([
        phenix_sdk::CapabilityId::parse("fixture.read").unwrap()
    ])
)]
struct Plugin {
    #[phenix(dep)]
    sessions: sessions::Plugin,

    #[phenix(component)]
    api: Api,

    #[phenix(resource, features(Transactions))]
    state: phenix_sdk::Durable<Store>,
}

#[test]
fn plugin_manifest_is_derived_from_authored_relationships() {
    let manifest = <Plugin as phenix_sdk::StaticPluginDefinition>::manifest();
    let components = <Plugin as phenix_sdk::StaticPluginDefinition>::component_manifests();

    assert_eq!(manifest.id.as_str(), "fixture.manifest");
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].maximum_authority, manifest.maximum_authority);
    assert!(matches!(
        manifest.execution,
        phenix_sdk::PluginExecution::Embedded
    ));
    assert_eq!(
        manifest
            .dependencies
            .iter()
            .map(phenix_sdk::PluginId::as_str)
            .collect::<Vec<_>>(),
        ["fixture.manifest.sessions"]
    );
    assert!(manifest.services.is_empty());
    assert_eq!(components[0].exports.len(), 1);
    assert_eq!(
        components[0].exports[0].interface.as_str(),
        "fixture.manifest.run@1"
    );
    assert_eq!(components[0].exports[0].priority, 7);
    assert_eq!(manifest.resource_namespaces.len(), 1);
    assert_eq!(
        manifest.resource_namespaces[0].as_str(),
        "fixture.manifest.state"
    );
}
