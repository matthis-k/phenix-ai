use phenix_core::{
    BackendFeature, DurableSchema, DurableSchemaRegistration, PluginId, ResourceNamespace,
};
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Durable<T> {
    marker: PhantomData<fn() -> T>,
}

impl<T> Durable<T> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<T> Default for Durable<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait StaticResourceField {
    type Resource: StaticResourceDefinition;
}

impl<T: StaticResourceDefinition> StaticResourceField for Durable<T> {
    type Resource = T;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticResourceMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub method: &'static str,
}

pub trait StaticResourceDefinition {
    fn schema_version() -> u32;

    fn migrations() -> Vec<StaticResourceMigration> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticResourceDescriptor {
    pub id: ResourceNamespace,
    pub schema: DurableSchema,
    pub field: &'static str,
    pub resource_type: &'static str,
    pub migrations: Vec<StaticResourceMigration>,
}

impl StaticResourceDescriptor {
    #[must_use]
    pub fn derived<F: StaticResourceField>(
        owner: &PluginId,
        field: &'static str,
        features: impl IntoIterator<Item = BackendFeature>,
    ) -> Self {
        let namespace = ResourceNamespace::parse(format!("{}.{field}", owner.as_str()))
            .expect("plugin id and Rust field name derive a valid resource namespace");
        Self::new::<F>(namespace, field, features)
    }

    #[must_use]
    pub fn explicit<F: StaticResourceField>(
        namespace: &str,
        field: &'static str,
        features: impl IntoIterator<Item = BackendFeature>,
    ) -> Self {
        let namespace = ResourceNamespace::parse(namespace)
            .expect("resource attribute validated the static resource namespace");
        Self::new::<F>(namespace, field, features)
    }

    fn new<F: StaticResourceField>(
        namespace: ResourceNamespace,
        field: &'static str,
        features: impl IntoIterator<Item = BackendFeature>,
    ) -> Self {
        let resource_type = std::any::type_name::<<F as StaticResourceField>::Resource>();
        let version = <F::Resource as StaticResourceDefinition>::schema_version();
        let schema = DurableSchema::requiring(namespace.clone(), version, features);
        Self {
            id: namespace,
            schema,
            field,
            resource_type,
            migrations: <F::Resource as StaticResourceDefinition>::migrations(),
        }
    }
}

pub trait StaticPluginResources {
    fn resources() -> Vec<StaticResourceDescriptor>;

    fn durable_schemas() -> Vec<DurableSchema> {
        Self::resources()
            .into_iter()
            .map(|resource| resource.schema)
            .collect()
    }

    fn durable_schema_registrations(owner: &PluginId) -> Vec<DurableSchemaRegistration> {
        Self::durable_schemas()
            .into_iter()
            .map(|schema| DurableSchemaRegistration::new(owner.clone(), schema))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Store;

    impl StaticResourceDefinition for Store {
        fn schema_version() -> u32 {
            3
        }
    }

    struct Resources;

    impl StaticPluginResources for Resources {
        fn resources() -> Vec<StaticResourceDescriptor> {
            vec![StaticResourceDescriptor::derived::<Durable<Store>>(
                &PluginId::parse("fixture.resource-owner").unwrap(),
                "plans",
                [BackendFeature::Transactions],
            )]
        }
    }

    #[test]
    fn resource_namespace_derives_from_owner_and_field() {
        let owner = PluginId::parse("fixture.resource-owner").unwrap();
        let resource = StaticResourceDescriptor::derived::<Durable<Store>>(
            &owner,
            "plans",
            [BackendFeature::Transactions],
        );

        assert_eq!(resource.id.as_str(), "fixture.resource-owner.plans");
        assert_eq!(
            resource.schema.namespace.as_str(),
            "fixture.resource-owner.plans"
        );
        assert_eq!(resource.schema.version, 3);
        assert!(resource
            .schema
            .required_features
            .contains(&BackendFeature::Transactions));
        assert!(resource.resource_type.ends_with("::Store"));
    }

    #[test]
    fn plugin_resource_declarations_expose_complete_schema_set() {
        let owner = PluginId::parse("fixture.resource-owner").unwrap();
        let schemas = Resources::durable_schemas();
        let registrations = Resources::durable_schema_registrations(&owner);

        assert_eq!(schemas.len(), 1);
        assert_eq!(
            schemas[0].namespace.as_str(),
            "fixture.resource-owner.plans"
        );
        assert_eq!(schemas[0].version, 3);
        assert!(schemas[0]
            .required_features
            .contains(&BackendFeature::Transactions));
        assert_eq!(registrations[0].owner, owner);
        assert_eq!(registrations[0].schema, schemas[0]);
    }
}
