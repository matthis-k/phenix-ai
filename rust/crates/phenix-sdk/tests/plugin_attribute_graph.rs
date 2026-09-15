#[allow(dead_code)]
mod composition {
    #[phenix_sdk::plugin(id = "fixture.attr.leaf")]
    pub struct Leaf;

    #[phenix_sdk::plugin(id = "fixture.attr.left")]
    pub struct Left {
        #[phenix(dep)]
        pub leaf: Leaf,
    }

    #[phenix_sdk::plugin(id = "fixture.attr.right")]
    pub struct Right {
        #[phenix(dep)]
        pub leaf: Leaf,
    }

    #[phenix_sdk::plugin(id = "fixture.attr.root")]
    pub struct Root {
        #[phenix(dep)]
        pub left: Left,
        #[phenix(dep)]
        pub right: Right,
    }

    #[phenix_sdk::plugin(id = "fixture.attr.conflict")]
    pub struct ConflictA;

    #[phenix_sdk::plugin(id = "fixture.attr.conflict")]
    pub struct ConflictB;

    #[phenix_sdk::plugin(id = "fixture.attr.conflict-root")]
    pub struct ConflictRoot {
        #[phenix(dep)]
        pub first: ConflictA,
        #[phenix(dep)]
        pub second: ConflictB,
    }

    #[phenix_sdk::interface("fixture.attr.models@1")]
    pub struct Models;

    #[phenix_sdk::plugin("fixture.attr.stateless")]
    pub mod stateless {
        #[phenix(export("fixture.attr.stateless.run@1"), public)]
        pub fn run() {}
    }

    pub struct PlanStore;

    #[phenix_sdk::resource(schema = 3)]
    impl PlanStore {
        #[phenix(migrate(from = 2))]
        fn v2_to_v3(_old: ()) -> Result<(), String> {
            Ok(())
        }
    }

    #[phenix_sdk::plugin("fixture.attr.resource-owner")]
    pub struct ResourceOwner {
        #[phenix(
            resource,
            id = "fixture.attr.plans",
            features(Transactions, Migrations)
        )]
        pub plans: phenix_sdk::Durable<PlanStore>,
    }
}

#[test]
fn dependencies_expand_recursively_and_deduplicate_diamonds() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<composition::Root>().unwrap();
    let ids = graph
        .ids()
        .map(phenix_core::PluginId::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        vec![
            "fixture.attr.leaf",
            "fixture.attr.left",
            "fixture.attr.right",
            "fixture.attr.root",
        ]
    );
}

#[test]
fn dependencies_reject_incompatible_duplicate_ids() {
    let error = phenix_sdk::StaticPluginGraph::compose::<composition::ConflictRoot>().unwrap_err();
    assert!(matches!(
        error,
        phenix_sdk::StaticPluginGraphError::DuplicateId { .. }
    ));
}

#[test]
fn interface_attribute_owns_canonical_runtime_identity() {
    let id = <composition::Models as phenix_sdk::InterfaceMarker>::interface_id();

    assert_eq!(id.as_str(), "fixture.attr.models@1");
}

#[test]
fn stateless_plugin_module_generates_default_component_and_export() {
    let plugin_id = composition::stateless::Plugin::plugin_id();
    let graph = phenix_sdk::StaticPluginGraph::compose::<composition::stateless::Plugin>().unwrap();
    let components =
        <composition::stateless::Plugin as phenix_sdk::StaticPluginComponents>::components();
    let exports =
        <composition::stateless::Component as phenix_sdk::StaticComponentBehavior>::exports();

    assert_eq!(plugin_id.as_str(), "fixture.attr.stateless");
    assert_eq!(
        graph.ids().next().unwrap().as_str(),
        "fixture.attr.stateless"
    );
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), "fixture.attr.stateless");
    assert_eq!(exports.len(), 1);
    assert_eq!(
        exports[0].interface.as_str(),
        "fixture.attr.stateless.run@1"
    );
    assert!(exports[0].public);
}

#[test]
fn resource_attribute_owns_schema_and_migration_metadata() {
    let migrations = <composition::PlanStore as phenix_sdk::StaticResourceDefinition>::migrations();

    assert_eq!(
        <composition::PlanStore as phenix_sdk::StaticResourceDefinition>::schema_version(),
        3
    );
    assert_eq!(migrations.len(), 1);
    assert_eq!(migrations[0].from_version, 2);
    assert_eq!(migrations[0].to_version, 3);
    assert_eq!(migrations[0].method, "v2_to_v3");
}

#[test]
fn plugin_resource_field_preserves_identity_schema_and_backend_features() {
    let resources = <composition::ResourceOwner as phenix_sdk::StaticPluginResources>::resources();

    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].id.as_str(), "fixture.attr.plans");
    assert_eq!(resources[0].schema.version, 3);
    assert_eq!(resources[0].field, "plans");
    assert!(resources[0]
        .schema
        .required_features
        .contains(&phenix_sdk::BackendFeature::Transactions));
    assert!(resources[0]
        .schema
        .required_features
        .contains(&phenix_sdk::BackendFeature::Migrations));
}
