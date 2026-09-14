from pathlib import Path


def replace_exact(path, old, new, expected=1):
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} matches, found {count}: {old[:120]!r}")
    file.write_text(text.replace(old, new))


# Core feature model: migrations are the only optional persistence operation.
replace_exact(
    "rust/crates/phenix-core/src/persistence.rs",
    '''pub enum BackendFeature {
    Transactions,
    UniqueKeys,
    ForeignKeys,
    OrderedAppend,
    IndexedRange,
    Migrations,
}''',
    '''pub enum BackendFeature {
    Migrations,
}''',
)
replace_exact(
    "rust/crates/phenix-core/src/persistence.rs",
    '''        [
            BackendFeature::Transactions,
            BackendFeature::UniqueKeys,
            BackendFeature::Migrations,
        ]
        .into_iter()
        .collect()''',
    '''        [BackendFeature::Migrations].into_iter().collect()''',
)
replace_exact(
    "rust/crates/phenix-core/src/persistence.rs",
    '''    #[test]
    fn unsupported_backend_feature_is_rejected_before_schema_registration() {
        let mut store = LocalPersistence::open_in_memory().unwrap();
        let schema =
            DurableSchema::requiring(namespace("owner.state"), 1, [BackendFeature::IndexedRange]);

        assert!(matches!(
            store.register_schema(&plugin("owner"), &schema),
            Err(PersistenceError::UnsupportedFeature {
                feature: BackendFeature::IndexedRange,
                ..
            })
        ));
    }
''',
    '''    #[test]
    fn local_backend_reports_only_callable_optional_features() {
        let store = LocalPersistence::open_in_memory().unwrap();

        assert_eq!(
            store.supported_features(),
            BTreeSet::from([BackendFeature::Migrations])
        );
    }
''',
)

replace_exact(
    "rust/crates/phenix-core/src/resolver.rs",
    '''fn backend_feature_name(feature: BackendFeature) -> &'static str {
    match feature {
        BackendFeature::Transactions => "transactions",
        BackendFeature::UniqueKeys => "unique_keys",
        BackendFeature::ForeignKeys => "foreign_keys",
        BackendFeature::OrderedAppend => "ordered_append",
        BackendFeature::IndexedRange => "indexed_range",
        BackendFeature::Migrations => "migrations",
    }
}''',
    '''fn backend_feature_name(feature: BackendFeature) -> &'static str {
    match feature {
        BackendFeature::Migrations => "migrations",
    }
}''',
)
replace_exact(
    "rust/crates/phenix-core/src/resolver.rs",
    "DurableSchema::requiring(namespace.clone(), 2, [BackendFeature::Transactions])",
    "DurableSchema::requiring(namespace.clone(), 2, [BackendFeature::Migrations])",
)

# Bootstrap tests use the one real optional feature to exercise negotiation.
replace_exact(
    "rust/crates/phenix-core/src/persistence_bootstrap.rs",
    '''            [provider(
                "fixture.provider",
                &[BackendFeature::Transactions],
                &["fixture-v1"],
            )],''',
    '''            [provider("fixture.provider", &[], &["fixture-v1"])],''',
)
replace_exact(
    "rust/crates/phenix-core/src/persistence_bootstrap.rs",
    "&[schema(&[BackendFeature::IndexedRange])],",
    "&[schema(&[BackendFeature::Migrations])],",
)
replace_exact(
    "rust/crates/phenix-core/src/persistence_bootstrap.rs",
    "missing: BTreeSet::from([BackendFeature::IndexedRange]),",
    "missing: BTreeSet::from([BackendFeature::Migrations]),",
)

replace_exact(
    "rust/crates/phenix-core/src/runtime/persistence_bootstrap.rs",
    "features: BTreeSet::from([BackendFeature::Transactions]),",
    "features: BTreeSet::new(),",
)
replace_exact(
    "rust/crates/phenix-core/src/runtime/persistence_bootstrap.rs",
    "DurableSchema::requiring(second_namespace, 1, [BackendFeature::IndexedRange]),",
    "DurableSchema::requiring(second_namespace, 1, [BackendFeature::Migrations]),",
)

provider_tests = "rust/crates/phenix-core/src/persistence_provider/tests.rs"
replace_exact(
    provider_tests,
    '''        descriptor: PersistenceProviderDescriptor::new(
            plugin("fixture.provider"),
            [BackendFeature::Transactions],
            ["mock-v1".to_owned()],
        ),''',
    '''        descriptor: PersistenceProviderDescriptor::new(
            plugin("fixture.provider"),
            [],
            ["mock-v1".to_owned()],
        ),''',
    expected=1,
)
replace_exact(
    provider_tests,
    "&[schema(BackendFeature::IndexedRange)],",
    "&[schema(BackendFeature::Migrations)],",
)
# Remaining provider fixtures are eligible and therefore advertise migrations.
file = Path(provider_tests)
text = file.read_text()
text = text.replace("[BackendFeature::Transactions]", "[BackendFeature::Migrations]")
text = text.replace("schema(BackendFeature::Transactions)", "schema(BackendFeature::Migrations)")
file.write_text(text)

harness = "rust/crates/phenix-harness/src/persistence.rs"
replace_exact(
    harness,
    "[BackendFeature::Transactions],\n                [\"sqlite-v1\".to_owned()],",
    "[BackendFeature::Migrations],\n                [\"sqlite-v1\".to_owned()],",
)
replace_exact(harness, "fixture(BackendFeature::Transactions)", "fixture(BackendFeature::Migrations)", expected=2)
replace_exact(
    harness,
    '''        let (builder, mut provider) = fixture(BackendFeature::IndexedRange);
        let result = builder.build_with_persistence_provider(&mut provider, binding());''',
    '''        let (builder, mut provider) = fixture(BackendFeature::Migrations);
        provider.descriptor.supported_features.clear();
        let result = builder.build_with_persistence_provider(&mut provider, binding());''',
)

# SDK authoring tests and macros now expose only the real optional feature.
static_resource = "rust/crates/phenix-sdk/src/authoring/static_resource.rs"
replace_exact(static_resource, "BackendFeature::Transactions", "BackendFeature::Migrations", expected=4)

resource_test = "rust/crates/phenix-sdk/tests/plugin_resource_authoring.rs"
replace_exact(resource_test, "features(Transactions, Migrations)", "features(Migrations)")
replace_exact(
    resource_test,
    '''    assert!(resource
        .schema
        .required_features
        .contains(&phenix_sdk::BackendFeature::Transactions));
''',
    "",
)

attribute_test = "rust/crates/phenix-sdk/tests/plugin_attribute_graph.rs"
replace_exact(attribute_test, "features(Transactions, Migrations)", "features(Migrations)")
replace_exact(
    attribute_test,
    '''    assert!(resources[0]
        .schema
        .required_features
        .contains(&phenix_sdk::BackendFeature::Transactions));
''',
    "",
)

replace_exact(
    "rust/crates/phenix-sdk/tests/plugin_manifest_authoring.rs",
    "features(Transactions)",
    "features(Migrations)",
)

macro_file = "rust/crates/phenix-sdk-macros/src/plugin_attr_core.rs"
replace_exact(
    macro_file,
    '''                        if !matches!(
                            feature.to_string().as_str(),
                            "Transactions"
                                | "UniqueKeys"
                                | "ForeignKeys"
                                | "OrderedAppend"
                                | "IndexedRange"
                                | "Migrations"
                        ) {''',
    '''                        if feature != "Migrations" {''',
)
replace_exact(macro_file, "features(Transactions, Migrations)", "features(Migrations)", expected=1)
replace_exact(macro_file, "features(Transactions, Telepathy)", "features(Migrations, Telepathy)", expected=1)

# Alternate backend conformance still exercises both supported and unsupported migrations.
conformance = "rust/crates/phenix-core/tests/persistence_backend_conformance.rs"
replace_exact(
    conformance,
    "#[derive(Clone, Debug, Default)]\nstruct MemoryPersistence {",
    "#[derive(Clone, Debug)]\nstruct MemoryPersistence {",
)
replace_exact(
    conformance,
    '''struct MemoryPersistence {
    schemas: BTreeMap<ResourceNamespace, (PluginId, u32)>,
    records: BTreeMap<(ResourceNamespace, String), Vec<u8>>,
}''',
    '''struct MemoryPersistence {
    schemas: BTreeMap<ResourceNamespace, (PluginId, u32)>,
    records: BTreeMap<(ResourceNamespace, String), Vec<u8>>,
    supports_migrations: bool,
}

impl Default for MemoryPersistence {
    fn default() -> Self {
        Self {
            schemas: BTreeMap::new(),
            records: BTreeMap::new(),
            supports_migrations: true,
        }
    }
}

impl MemoryPersistence {
    fn without_migrations() -> Self {
        Self {
            supports_migrations: false,
            ..Self::default()
        }
    }
}''',
)
# Merge the second impl block into the first is not required in Rust.
replace_exact(
    conformance,
    '''    fn supported_features(&self) -> BTreeSet<BackendFeature> {
        [
            BackendFeature::Transactions,
            BackendFeature::UniqueKeys,
            BackendFeature::Migrations,
        ]
        .into_iter()
        .collect()
    }''',
    '''    fn supported_features(&self) -> BTreeSet<BackendFeature> {
        if self.supports_migrations {
            BTreeSet::from([BackendFeature::Migrations])
        } else {
            BTreeSet::new()
        }
    }''',
)
replace_exact(
    conformance,
    '''            &DurableSchema::requiring(
                first_namespace.clone(),
                1,
                [BackendFeature::Transactions, BackendFeature::UniqueKeys],
            ),''',
    '''            &DurableSchema::new(first_namespace.clone(), 1),''',
)
replace_exact(
    conformance,
    "[BackendFeature::Transactions, BackendFeature::Migrations],",
    "[BackendFeature::Migrations],",
)
replace_exact(
    conformance,
    '''    let requested =
        DurableSchema::requiring(namespace("feature.test"), 1, [BackendFeature::IndexedRange]);''',
    '''    let requested =
        DurableSchema::requiring(namespace("feature.test"), 1, [BackendFeature::Migrations]);''',
)
replace_exact(
    conformance,
    "feature: BackendFeature::IndexedRange,",
    "feature: BackendFeature::Migrations,",
)
replace_exact(
    conformance,
    '''    assert_unsupported_feature(LocalPersistence::open_in_memory().unwrap());
    assert_unsupported_feature(MemoryPersistence::default());''',
    '''    assert_unsupported_feature(MemoryPersistence::without_migrations());''',
)

# No removed feature may survive in Rust source, including macro test fixtures.
removed = ["Transactions", "UniqueKeys", "ForeignKeys", "OrderedAppend", "IndexedRange"]
for path in Path("rust").rglob("*.rs"):
    text = path.read_text()
    for name in removed:
        if f"BackendFeature::{name}" in text or f"features({name}" in text or f'"{name}"' in text and "backend feature" in text:
            raise SystemExit(f"removed backend feature {name} remains in {path}")
