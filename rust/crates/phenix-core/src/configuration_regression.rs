use crate::{
    Authority, PermissionId, ConfigContribution, ConfigContributionSource, ConfigMergeError,
    ConfigNamespace, ConfigSourceClass, ConfigurationFrontendId, ConfigurationFrontendMetadata,
    FrontendConfigContribution, FrontendConfigError, ResolvedComposition, ResolvedCompositionError,
};
use std::collections::BTreeSet;

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).unwrap()
}

fn contribution(frontend: &str, source: &str, value: serde_json::Value) -> ConfigContribution {
    ConfigContribution {
        source: ConfigContributionSource {
            frontend: ConfigurationFrontendId::parse(frontend).unwrap(),
            source_identity: source.into(),
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
fn equivalent_equal_precedence_frontends_converge_independent_of_registration_order() {
    let nix = contribution(
        "phenix-config-nix",
        "flake:fixture",
        serde_json::json!({"mode":"strict"}),
    );
    let lua = contribution(
        "phenix-config-lua",
        "file:phenix.lua",
        serde_json::json!({"mode":"strict"}),
    );

    let first = ResolvedComposition::resolve([], [], [nix.clone(), lua.clone()], &Authority::default())
        .unwrap();
    let second = ResolvedComposition::resolve([], [], [lua, nix], &Authority::default()).unwrap();

    assert_eq!(first.generation(), second.generation());
    assert_eq!(first.configuration(), second.configuration());
    assert_eq!(first.configuration().entries().len(), 1);
}

#[test]
fn conflicting_equal_precedence_frontends_fail_closed() {
    let nix = contribution(
        "phenix-config-nix",
        "flake:fixture",
        serde_json::json!({"mode":"strict"}),
    );
    let lua = contribution(
        "phenix-config-lua",
        "file:phenix.lua",
        serde_json::json!({"mode":"relaxed"}),
    );

    assert_eq!(
        ResolvedComposition::resolve([], [], [nix, lua], &Authority::default()).unwrap_err(),
        ResolvedCompositionError::ConfigurationMerge(ConfigMergeError::ConflictingContributions {
            namespace: ConfigNamespace::parse("fixture.policy@1").unwrap(),
            contract_version: 1,
            precedence: 10,
        })
    );
}

#[test]
fn third_party_frontend_can_lower_plugin_defined_configuration_without_core_changes() {
    let namespace = ConfigNamespace::parse("acme.compiler-review@7").unwrap();
    let frontend = ConfigurationFrontendId::parse("acme-config-lua").unwrap();
    let metadata = ConfigurationFrontendMetadata {
        id: frontend.clone(),
        version: 1,
        accepted_source_kinds: BTreeSet::from(["acme-lua".into()]),
        exposed_namespaces: BTreeSet::from([namespace.clone()]),
        watch: true,
        required_authority: Authority::default(),
    };
    let contribution = FrontendConfigContribution {
        source_kind: "acme-lua".into(),
        source_identity: "file:acme.lua".into(),
        source_revision: "sha256:fixture".into(),
        source_class: ConfigSourceClass::Materialized,
        namespace: namespace.clone(),
        contract_version: 7,
        precedence: 40,
        value: serde_json::json!({"team":"compiler","review":"strict"}).into(),
        requested_authority: Authority::default(),
    };

    let resolved = ResolvedComposition::resolve_frontends(
        [],
        [],
        [metadata],
        [(frontend.clone(), contribution)],
        &Authority::default(),
    )
    .unwrap();
    let entry = &resolved.configuration().entries()[0];

    assert_eq!(entry.namespace, namespace);
    assert_eq!(entry.contract_version, 7);
    assert_eq!(
        entry.value,
        serde_json::json!({"team":"compiler","review":"strict"}).into()
    );
    assert_eq!(entry.attributions.len(), 1);
    assert_eq!(entry.attributions[0].source.frontend, frontend);
    assert_eq!(
        entry.attributions[0].source.source_identity,
        "file:acme.lua"
    );
    assert_eq!(
        entry.attributions[0].source.source_revision,
        "sha256:fixture"
    );
}

#[test]
fn frontend_requested_authority_cannot_bypass_resolver_policy() {
    let read = capability("workspace.read");
    let write = capability("workspace.write");
    let namespace = ConfigNamespace::parse("acme.compiler-review@7").unwrap();
    let frontend = ConfigurationFrontendId::parse("acme-config-ipc").unwrap();
    let metadata = ConfigurationFrontendMetadata {
        id: frontend.clone(),
        version: 1,
        accepted_source_kinds: BTreeSet::from(["ipc".into()]),
        exposed_namespaces: BTreeSet::from([namespace.clone()]),
        watch: true,
        required_authority: Authority::default(),
    };
    let contribution = FrontendConfigContribution {
        source_kind: "ipc".into(),
        source_identity: "socket:acme".into(),
        source_revision: "request:42".into(),
        source_class: ConfigSourceClass::Materialized,
        namespace,
        contract_version: 7,
        precedence: 40,
        value: serde_json::json!({"review":"strict"}).into(),
        requested_authority: Authority::new([read.clone(), write.clone()]),
    };

    let resolved = ResolvedComposition::resolve_frontends(
        [],
        [],
        [metadata],
        [(frontend, contribution)],
        &Authority::new([read.clone()]),
    )
    .unwrap();
    let entry = &resolved.configuration().entries()[0];

    assert!(entry.attributions[0].requested_authority.permits(&write));
    assert!(entry.granted_authority.permits(&read));
    assert!(!entry.granted_authority.permits(&write));
}

#[test]
fn stable_frontend_rejects_unmaterialized_environment_binding() {
    let namespace = ConfigNamespace::parse("acme.compiler-review@7").unwrap();
    let frontend = ConfigurationFrontendId::parse("acme-config-env").unwrap();
    let metadata = ConfigurationFrontendMetadata {
        id: frontend.clone(),
        version: 1,
        accepted_source_kinds: BTreeSet::from(["environment".into()]),
        exposed_namespaces: BTreeSet::from([namespace.clone()]),
        watch: false,
        required_authority: Authority::default(),
    };
    let contribution = FrontendConfigContribution {
        source_kind: "environment".into(),
        source_identity: "env:ACME_REVIEW".into(),
        source_revision: "process-start".into(),
        source_class: ConfigSourceClass::EnvironmentBinding,
        namespace,
        contract_version: 7,
        precedence: 40,
        value: serde_json::json!({"review":"strict"}).into(),
        requested_authority: Authority::default(),
    };

    assert_eq!(
        ResolvedComposition::resolve_frontends(
            [],
            [],
            [metadata],
            [(frontend.clone(), contribution)],
            &Authority::default(),
        )
        .unwrap_err(),
        ResolvedCompositionError::ConfigurationFrontend {
            frontend,
            error: FrontendConfigError::EnvironmentBindingChangesSemantics,
        }
    );
}

#[test]
fn equivalent_nix_and_lua_frontends_resolve_to_the_same_semantic_generation() {
    let namespace = ConfigNamespace::parse("acme.compiler-review@7").unwrap();
    let frontend_metadata = |id: &str, source_kind: &str| ConfigurationFrontendMetadata {
        id: ConfigurationFrontendId::parse(id).unwrap(),
        version: 1,
        accepted_source_kinds: BTreeSet::from([source_kind.into()]),
        exposed_namespaces: BTreeSet::from([namespace.clone()]),
        watch: true,
        required_authority: Authority::default(),
    };
    let frontend_contribution =
        |source_kind: &str, source_identity: &str, source_revision: &str| {
            FrontendConfigContribution {
                source_kind: source_kind.into(),
                source_identity: source_identity.into(),
                source_revision: source_revision.into(),
                source_class: ConfigSourceClass::Materialized,
                namespace: namespace.clone(),
                contract_version: 7,
                precedence: 40,
                value: serde_json::json!({"team":"compiler","review":"strict"}).into(),
                requested_authority: Authority::default(),
            }
        };

    let nix_frontend = ConfigurationFrontendId::parse("phenix-config-nix").unwrap();
    let lua_frontend = ConfigurationFrontendId::parse("phenix-config-lua").unwrap();
    let nix = ResolvedComposition::resolve_frontends(
        [],
        [],
        [frontend_metadata("phenix-config-nix", "nix")],
        [(
            nix_frontend,
            frontend_contribution("nix", "flake:acme", "sha256:nix"),
        )],
        &Authority::default(),
    )
    .unwrap();
    let lua = ResolvedComposition::resolve_frontends(
        [],
        [],
        [frontend_metadata("phenix-config-lua", "lua")],
        [(
            lua_frontend,
            frontend_contribution("lua", "file:phenix.lua", "sha256:lua"),
        )],
        &Authority::default(),
    )
    .unwrap();

    assert_ne!(
        nix.configuration().entries()[0].attributions,
        lua.configuration().entries()[0].attributions
    );
    assert_eq!(
        nix.configuration().semantic_payload(),
        lua.configuration().semantic_payload()
    );
    assert_eq!(nix.generation(), lua.generation());
}
