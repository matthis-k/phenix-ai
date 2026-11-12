use phenix_graph::compose::{
    ComposeError, Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration,
};
use phenix_graph::network::{NetworkBuilder, PortPath};
use phenix_graph::{ComponentDef, GraphOrigin};
use std::sync::Arc;

fn definition() -> Arc<ComponentDef> {
    Arc::new(ComponentDef::composite(
        NetworkBuilder::new().build().unwrap(),
    ))
}
fn add(path: &str, definition: Arc<ComponentDef>) -> NetworkPatch {
    NetworkPatch::new([Edit::Add {
        path: path.into(),
        definition,
    }])
}
fn composer() -> Composer {
    Composer::new(NetworkBuilder::new().build().unwrap()).unwrap()
}

#[test]
fn payload_path_or_semantic_looking_namespace_does_not_grant_authority() {
    for path in ["host", "trusted.contract", "plugin"] {
        let patch = add(path, definition());
        let registration = Registration::new("plugin", [], 100, GraphOrigin::default(), [], patch);
        assert!(matches!(
            composer().register([registration]),
            Err(ComposeError::Unauthorized {
                footprint: Footprint::Component(_),
                ..
            })
        ));
    }
}

#[test]
fn grants_are_exact_not_prefixes_or_origin_ancestry() {
    let origin = GraphOrigin::default();
    let patch = add("plugin-other", definition());
    let registration = Registration::new(
        "plugin",
        [],
        0,
        origin.child(),
        [Footprint::Component("plugin".into())],
        patch,
    );
    assert!(matches!(
        composer().register([registration]),
        Err(ComposeError::Unauthorized { .. })
    ));
}

#[test]
fn component_grant_never_authorizes_connection_export_or_source_writes() {
    let path = PortPath {
        process: "host".into(),
        port: "port".into(),
    };
    for edit in [
        Edit::AddSource("host".into()),
        Edit::ExposeOutput {
            name: "public".into(),
            from: path.clone(),
        },
        Edit::Disconnect(ConnectionKey::new(path.clone(), path)),
    ] {
        let patch = NetworkPatch::new([edit]);
        let registration = Registration::new(
            "plugin",
            [],
            0,
            GraphOrigin::default(),
            [Footprint::Component("host".into())],
            patch,
        );
        assert!(matches!(
            composer().register([registration]),
            Err(ComposeError::Unauthorized { .. })
        ));
    }
}

#[test]
fn nested_private_edits_fail_explicitly_despite_exact_grants() {
    let patch = add("module/private", definition());
    let registration = Registration::new(
        "plugin",
        [],
        0,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    );
    assert!(
        matches!(composer().register([registration]), Err(ComposeError::PrivateEditScope(path)) if path == "module/private")
    );
}

#[test]
fn forged_parent_or_sibling_origins_cannot_be_laundered_through_payloads() {
    let parent = GraphOrigin::default();
    for claimed in [parent.clone(), parent.child()] {
        let definition = Arc::new((*definition()).clone().with_origin(claimed));
        let patch = add("plugin", definition);
        let registration =
            Registration::new("plugin", [], 0, parent.child(), patch.footprints(), patch);
        assert!(matches!(
            composer().register([registration]),
            Err(ComposeError::ForgedOrigin(_))
        ));
    }
}

#[test]
fn nested_payload_cannot_hide_a_forged_origin() {
    let mut builder = NetworkBuilder::new();
    builder
        .add(
            "hidden",
            Arc::new((*definition()).clone().with_origin(GraphOrigin::default())),
        )
        .unwrap();
    let patch = add(
        "module",
        Arc::new(ComponentDef::composite(builder.build().unwrap())),
    );
    let registration = Registration::new(
        "plugin",
        [],
        0,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    );
    assert!(
        matches!(composer().register([registration]), Err(ComposeError::ForgedOrigin(path)) if path == "module/hidden")
    );
}

#[test]
fn final_origin_is_exact_registration_scope_not_parent_scope() {
    let parent = GraphOrigin::default();
    let origin = parent.child();
    let patch = add("module", definition());
    let registration =
        Registration::new("plugin", [], 0, origin.clone(), patch.footprints(), patch);
    let mut composer = composer();
    composer.register([registration]).unwrap();
    let actual = composer.current().network().instances()["module"]
        .origin()
        .unwrap();
    assert_eq!(actual, &origin);
    assert_ne!(actual, &parent);
}

#[test]
fn unauthorized_defeated_registration_is_not_silently_accepted() {
    let patch = add("host", definition());
    let good = Registration::new(
        "good",
        [],
        9,
        GraphOrigin::default(),
        patch.footprints(),
        patch.clone(),
    );
    let bad = Registration::new("bad", [], -9, GraphOrigin::default(), [], patch);
    assert!(matches!(
        composer().register([good, bad]),
        Err(ComposeError::Unauthorized { .. })
    ));
}

#[test]
fn defeated_payloads_still_cannot_launder_origins() {
    let patch = add("host", definition());
    let good = Registration::new(
        "good",
        [],
        9,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    );
    let forged = Arc::new((*definition()).clone().with_origin(GraphOrigin::default()));
    let patch = add("host", forged);
    let bad = Registration::new(
        "bad",
        [],
        -9,
        GraphOrigin::default(),
        patch.footprints(),
        patch,
    );
    assert!(matches!(
        composer().register([good, bad]),
        Err(ComposeError::ForgedOrigin(_))
    ));
}

#[test]
fn native_inspection_preserves_exact_root_declared_and_inherited_provenance() {
    let host = GraphOrigin::default();
    let added = host.child();
    let replaced = host.child();
    let mut child = NetworkBuilder::new();
    child.add("leaf", definition()).unwrap();
    let module = Arc::new(ComponentDef::composite(child.build().unwrap()));
    let mut composer = Composer::new(
        NetworkBuilder::new()
            .with_origin(host.clone())
            .build()
            .unwrap(),
    )
    .unwrap();
    let patch = add("module", definition());
    composer
        .register([Registration::new(
            "add",
            [],
            0,
            added.clone(),
            patch.footprints(),
            patch,
        )])
        .unwrap();
    assert_eq!(
        composer
            .current()
            .compiled()
            .inspect("module")
            .unwrap()
            .declared_origin,
        Some(added.clone())
    );
    let patch = NetworkPatch::new([Edit::Replace {
        path: "module".into(),
        definition: module.clone(),
    }]);
    composer
        .register([Registration::new(
            "replace",
            ["add".into()],
            0,
            replaced.clone(),
            patch.footprints(),
            patch,
        )])
        .unwrap();
    let compiled = composer.current().compiled();
    let root = compiled.inspect("").unwrap();
    assert_eq!(root.origin, Some(host.clone()));
    assert_eq!(root.declared_origin, Some(host));
    let actual = compiled.inspect("module").unwrap();
    assert_eq!(actual.origin, Some(replaced.clone()));
    assert_eq!(actual.declared_origin, Some(replaced.clone()));
    assert_eq!(actual.definition_key, Some(module.definition_key()));
    let leaf = compiled.inspect("module/leaf").unwrap();
    assert_eq!(leaf.origin, Some(replaced));
    assert_eq!(leaf.declared_origin, None);
    composer.revoke("replace").unwrap();
    assert_eq!(
        composer
            .current()
            .compiled()
            .inspect("module")
            .unwrap()
            .declared_origin,
        Some(added)
    );
    assert!(
        composer
            .current()
            .compiled()
            .inspect("module/leaf")
            .is_err()
    );
}

#[test]
fn native_claim_and_binding_never_grant_composition_writes() {
    let contract =
        phenix_graph::contract::Contract::new("trusted/host", definition().interface().clone())
            .unwrap();
    let claimed = Arc::new((*definition()).clone().claim(&contract).unwrap());
    let origin = GraphOrigin::default();
    let mut builder = NetworkBuilder::new();
    builder.add("host", claimed.clone()).unwrap();
    let mut composer = Composer::new(builder.build().unwrap()).unwrap();
    composer
        .current()
        .compiled()
        .bind("host", &contract)
        .unwrap();
    let patch = NetworkPatch::new([Edit::Replace {
        path: "host".into(),
        definition: claimed,
    }]);
    assert!(matches!(
        composer.register([Registration::new(
            "plugin",
            [],
            0,
            origin.clone(),
            [],
            patch.clone()
        )]),
        Err(ComposeError::Unauthorized { .. })
    ));
    composer
        .register([Registration::new(
            "approved",
            [],
            0,
            origin.clone(),
            [Footprint::Component("host".into())],
            patch,
        )])
        .unwrap();
    composer
        .current()
        .compiled()
        .bind("host", &contract)
        .unwrap();
    assert_eq!(
        composer
            .current()
            .compiled()
            .inspect("host")
            .unwrap()
            .declared_origin,
        Some(origin)
    );
}
