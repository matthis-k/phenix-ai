struct V2;
struct V3;
struct MigrationError;
struct Store;

#[phenix_sdk::resource(schema = 3)]
impl Store {
    #[phenix(migrate(from = 2))]
    fn v2_to_v3(_old: V2) -> Result<V3, MigrationError> {
        Ok(V3)
    }
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.resource-only")]
struct Plugin {
    #[phenix(resource, features(Transactions, Migrations))]
    state: phenix_sdk::Durable<Store>,
}

#[test]
fn resource_only_plugin_derives_durable_registration_metadata() {
    let resources = <Plugin as phenix_sdk::StaticPluginResources>::resources();
    assert_eq!(resources.len(), 1);

    let resource = &resources[0];
    assert_eq!(resource.id.as_str(), "fixture.resource-only.state");
    assert_eq!(resource.schema.version, 3);
    assert!(resource
        .schema
        .required_features
        .contains(&phenix_sdk::BackendFeature::Transactions));
    assert!(resource
        .schema
        .required_features
        .contains(&phenix_sdk::BackendFeature::Migrations));
    assert_eq!(resource.migrations.len(), 1);
    assert_eq!(resource.migrations[0].from_version, 2);
    assert_eq!(resource.migrations[0].to_version, 3);
    assert_eq!(resource.migrations[0].method, "v2_to_v3");

    let manifest = <Plugin as phenix_sdk::StaticPluginDefinition>::manifest();
    let namespaces = manifest
        .resource_namespaces
        .iter()
        .map(|namespace| namespace.as_str())
        .collect::<Vec<_>>();
    assert_eq!(namespaces, ["fixture.resource-only.state"]);

    assert!(Store::v2_to_v3(V2).is_ok());
    let _ = MigrationError;
}
