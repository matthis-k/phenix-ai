use phenix_core::{
    Authority, ComponentExport, ComponentGraphError, ComponentId, ComponentImport,
    ComponentManifest, ConfigContribution, ConfigContributionSource, ConfigMergeError,
    ConfigNamespace, ConfigSourceClass, ConfigurationFrontendId, ConfigurationFrontendMetadata,
    FrontendConfigContribution, FrontendConfigError, GenerationResolutionError, InterfaceId,
    PermissionId, PluginExecution, PluginId, PluginManifest, ResolvedGeneration,
};
use phenix_harness::{PhenixRuntimeBuildError, PhenixRuntimeBuilder};
use std::collections::BTreeSet;

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn component(value: &str) -> ComponentId {
    ComponentId::parse(value).unwrap()
}

fn interface(value: &str) -> InterfaceId {
    InterfaceId::parse(value).unwrap()
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).unwrap()
}

fn owner(id: &str, authority: Authority) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: authority,
    }
}

#[test]
fn harness_fails_closed_before_execution_when_required_component_import_is_missing() {
    let mut builder = PhenixRuntimeBuilder::new();
    builder.add_manifest(owner("consumer-owner", Authority::default()));
    builder.add_component(ComponentManifest {
        listeners: Vec::new(),
        id: component("consumer"),
        owner: plugin("consumer-owner"),
        imports: vec![ComponentImport {
            interface: interface("phenix.fixture@1"),
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    });

    let error = match builder.build() {
        Ok(_) => panic!("required component import unexpectedly resolved"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PhenixRuntimeBuildError::Resolution(GenerationResolutionError::ComponentGraph(
            ComponentGraphError::MissingRequiredImport { .. }
        ))
    ));
}

#[test]
fn harness_exposes_the_resolved_component_binding_and_attenuated_authority() {
    let read = capability("fixture.read");
    let write = capability("fixture.write");
    let harness_authority = Authority::new([read.clone(), write.clone()]);

    let mut builder = PhenixRuntimeBuilder::new();
    builder.set_component_authority(harness_authority.clone());
    builder.add_manifest(owner(
        "consumer-owner",
        Authority::new([read.clone(), write.clone()]),
    ));
    builder.add_manifest(owner("provider-owner", Authority::new([read.clone()])));
    builder.add_component(ComponentManifest {
        listeners: Vec::new(),
        id: component("provider"),
        owner: plugin("provider-owner"),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: interface("phenix.fixture@1"),
            schema: Default::default(),
            priority: 10,
            required_authority: Authority::new([read.clone()]),
        }],
        maximum_authority: Authority::new([read.clone()]),
    });
    builder.add_component(ComponentManifest {
        listeners: Vec::new(),
        id: component("consumer"),
        owner: plugin("consumer-owner"),
        imports: vec![ComponentImport {
            interface: interface("phenix.fixture@1"),
            schema: Default::default(),
            required: true,
            authority: harness_authority,
        }],
        exports: Vec::new(),
        maximum_authority: Authority::new([read.clone(), write.clone()]),
    });

    let harness = builder.build().unwrap();
    let graph = harness.component_graph();
    assert_eq!(graph.components().count(), 2);

    let binding = graph
        .import_handle(&component("consumer"), &interface("phenix.fixture@1"))
        .unwrap()
        .expect("required import is resolved");
    assert_eq!(binding.importer(), &component("consumer"));
    assert_eq!(binding.exporter(), &component("provider"));
    assert_eq!(binding.owning_plugin(), &plugin("provider-owner"));
    assert_eq!(binding.execution(), &PluginExecution::Embedded);
    assert!(binding.effective_authority().permits(&read));
    assert!(!binding.effective_authority().permits(&write));
}

#[test]
fn configuration_frontend_cannot_bypass_source_authority_or_stable_semantics() {
    let read = capability("config.read");
    let frontend = ConfigurationFrontendId::parse("phenix-config-lua").unwrap();
    let namespace = ConfigNamespace::parse("acme.engineering@1").unwrap();
    let metadata = ConfigurationFrontendMetadata {
        id: frontend.clone(),
        version: 1,
        accepted_source_kinds: BTreeSet::from(["lua".into()]),
        exposed_namespaces: BTreeSet::from([namespace.clone()]),
        watch: true,
        required_authority: Authority::new([read.clone()]),
    };
    let contribution = |source_class| FrontendConfigContribution {
        source_kind: "lua".into(),
        source_identity: "file:phenix.lua".into(),
        source_revision: "sha256:fixture".into(),
        source_class,
        namespace: namespace.clone(),
        contract_version: 1,
        precedence: 10,
        value: serde_json::json!({"team":"compiler"}).into(),
        requested_authority: Authority::default(),
    };

    let denied = ResolvedGeneration::resolve_frontends(
        [],
        [],
        [metadata.clone()],
        [(
            frontend.clone(),
            contribution(ConfigSourceClass::Materialized),
        )],
        &Authority::default(),
    )
    .unwrap_err();
    assert_eq!(
        denied,
        GenerationResolutionError::ConfigurationFrontend {
            frontend: frontend.clone(),
            error: FrontendConfigError::SourceAuthorityDenied,
        }
    );

    let unstable = ResolvedGeneration::resolve_frontends(
        [],
        [],
        [metadata],
        [(
            frontend.clone(),
            contribution(ConfigSourceClass::EnvironmentBinding),
        )],
        &Authority::new([read]),
    )
    .unwrap_err();
    assert_eq!(
        unstable,
        GenerationResolutionError::ConfigurationFrontend {
            frontend,
            error: FrontendConfigError::EnvironmentBindingChangesSemantics,
        }
    );
}

fn canonical_contribution(frontend: &str, value: serde_json::Value) -> ConfigContribution {
    ConfigContribution {
        source: ConfigContributionSource {
            frontend: ConfigurationFrontendId::parse(frontend).unwrap(),
            source_identity: format!("fixture:{frontend}"),
            source_revision: "rev-1".into(),
        },
        namespace: ConfigNamespace::parse("fixture.policy@1").unwrap(),
        contract_version: 1,
        precedence: 10,
        value: value.into(),
        requested_authority: Authority::default(),
    }
}

#[test]
fn harness_builder_exposes_the_canonical_resolved_generation() {
    let mut baseline = PhenixRuntimeBuilder::new();
    baseline.add_config_contribution(canonical_contribution(
        "phenix-config-nix",
        serde_json::json!({"mode":"strict"}),
    ));
    let baseline = baseline.build().unwrap();

    let mut changed = PhenixRuntimeBuilder::new();
    changed.add_config_contribution(canonical_contribution(
        "phenix-config-lua",
        serde_json::json!({"mode":"relaxed"}),
    ));
    let changed = changed.build().unwrap();

    assert_eq!(
        baseline
            .resolved_generation()
            .configuration()
            .entries()
            .len(),
        1
    );
    assert_eq!(
        baseline.resolved_generation().configuration().entries()[0].attributions[0]
            .source
            .frontend
            .as_str(),
        "phenix-config-nix"
    );
    assert_ne!(baseline.generation(), changed.generation());
}

#[test]
fn harness_builder_rejects_configuration_conflicts_before_activation() {
    let mut builder = PhenixRuntimeBuilder::new();
    builder.add_config_contribution(canonical_contribution(
        "phenix-config-nix",
        serde_json::json!({"mode":"strict"}),
    ));
    builder.add_config_contribution(canonical_contribution(
        "phenix-config-lua",
        serde_json::json!({"mode":"relaxed"}),
    ));

    let error = match builder.build() {
        Ok(_) => panic!("conflicting contributions unexpectedly resolved"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PhenixRuntimeBuildError::Resolution(GenerationResolutionError::ConfigurationMerge(
            ConfigMergeError::ConflictingContributions { .. }
        ))
    ));
}
