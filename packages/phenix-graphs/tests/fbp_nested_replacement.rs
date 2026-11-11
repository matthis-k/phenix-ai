//! A composite opts into exact nested replacement seams; no private edit authority.
use phenix_graph::{
    ComponentDef, GraphOrigin, Value,
    compose::{ComposeError, Composer, Edit, Footprint, NetworkPatch, Registration},
    network::{NetworkBuilder, NetworkDef},
};
use std::{num::NonZeroUsize, sync::Arc};

fn cap() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}

fn stage(label: &'static str) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::map::<String, String, _>(move |input| {
        format!("{label}[{input}]")
    }))
}

fn inner(expose_leaf: bool) -> NetworkDef {
    let mut b = NetworkBuilder::new();
    let leaf = b.add("leaf", stage("base")).unwrap();
    let sibling = b.add("sibling", stage("sibling")).unwrap();
    b.connect(
        leaf.output::<String>("output").unwrap(),
        sibling.input::<String>("input").unwrap(),
        cap(),
    )
    .unwrap();
    b.expose_input("input", leaf.input::<String>("input").unwrap(), cap())
        .unwrap();
    b.expose_output("output", sibling.output::<String>("output").unwrap())
        .unwrap();
    if expose_leaf {
        b.allow_replace(&leaf).unwrap();
    }
    b.build().unwrap()
}

fn outer(expose_inner: bool, expose_leaf: bool) -> NetworkDef {
    let mut b = NetworkBuilder::new();
    let child = b
        .add(
            "inner",
            Arc::new(ComponentDef::composite(inner(expose_leaf))),
        )
        .unwrap();
    b.expose_input("input", child.input::<String>("input").unwrap(), cap())
        .unwrap();
    b.expose_output("output", child.output::<String>("output").unwrap())
        .unwrap();
    if expose_inner {
        b.allow_replace(&child).unwrap();
    }
    b.build().unwrap()
}

fn composer(expose_inner: bool, expose_leaf: bool) -> Composer {
    let mut b = NetworkBuilder::new();
    let module = b
        .add(
            "m",
            Arc::new(ComponentDef::composite(outer(expose_inner, expose_leaf))),
        )
        .unwrap();
    b.expose_input("in", module.input::<String>("input").unwrap(), cap())
        .unwrap();
    b.expose_output("out", module.output::<String>("output").unwrap())
        .unwrap();
    Composer::new(b.build().unwrap()).unwrap()
}

fn replacement(
    path: &str,
    definition: Arc<ComponentDef>,
    grants: impl IntoIterator<Item = Footprint>,
) -> Registration {
    Registration::new(
        "extension",
        [],
        0,
        GraphOrigin::default().child(),
        grants,
        NetworkPatch::new([Edit::Replace {
            path: path.to_owned(),
            definition,
        }]),
    )
}

fn run(session: &mut phenix_graph::runtime::NetworkSession) -> String {
    session.inject("in", Value::local(String::from("Q"))).unwrap();
    let report = session.run_until_idle(100);
    assert!(!report.budget_exhausted);
    let out = session.take_output("out").unwrap().unwrap();
    out.value().borrow::<String>().unwrap().clone()
}

#[test]
fn host_granted_deep_replacement_executes_and_revokes_without_touching_old_sessions() {
    let mut composer = composer(true, true);
    let mut old_session = composer.current().compiled().start();
    let origin = GraphOrigin::default().child();
    let patch = replacement(
        "m/inner/leaf",
        stage("alternative"),
        [Footprint::Component("m/inner/leaf".to_owned())],
    );
    composer.register([patch]).unwrap();
    assert_eq!(run(&mut old_session), "sibling[base[Q]]");
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[alternative[Q]]"
    );
    let changed = composer.current().compiled().inspect("m/inner/leaf").unwrap();
    assert_eq!(changed.declared_origin, Some(origin));
    for unchanged in ["m", "m/inner", "m/inner/sibling"] {
        assert_eq!(
            composer
                .current()
                .compiled()
                .inspect(unchanged)
                .unwrap()
                .declared_origin,
            None
        );
    }
    assert!(composer.revoke("extension").unwrap().contains("extension"));
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[base[Q]]"
    );
}

#[test]
fn private_siblings_and_unexposed_ancestors_reject_even_exact_grants() {
    for (expose_inner, expose_leaf, path) in [
        (false, true, "m/inner/leaf"),
        (true, false, "m/inner/leaf"),
        (true, true, "m/inner/sibling"),
    ] {
        let mut composer = composer(expose_inner, expose_leaf);
        let before = composer.current().compiled().processes().len();
        let result = composer.register([replacement(
            path,
            stage("other"),
            [Footprint::Component(path.to_owned())],
        )]);
        assert!(matches!(result, Err(ComposeError::PrivateEditScope(_))));
        assert_eq!(composer.current().compiled().processes().len(), before);
        assert_eq!(
            run(&mut composer.current().compiled().start()),
            "sibling[base[Q]]"
        );
    }
}

#[test]
fn nested_replacement_checks_exact_grant_shape_and_path_atomically() {
    let mut composer = composer(true, true);
    assert!(matches!(
        composer.register([replacement("m/inner/leaf", stage("bad"), [])]),
        Err(ComposeError::Unauthorized {
            footprint: Footprint::Component(_),
            ..
        })
    ));
    let mut different = Composer::new(composer.current().network().clone()).unwrap();
    let sink = Arc::new(ComponentDef::sink(|_: &String| {}));
    assert!(matches!(
        different.register([replacement(
            "m/inner/leaf",
            sink,
            [Footprint::Component("m/inner/leaf".to_owned())]
        )]),
        Err(ComposeError::Network(phenix_graph::network::NetworkError::InterfaceMismatch(_)))
    ));
    for path in ["m//leaf", "m/../leaf", "m/inner/leaf/"] {
        let result = composer.register([replacement(
            path,
            stage("invalid"),
            [Footprint::Component(path.to_owned())],
        )]);
        assert!(matches!(result, Err(ComposeError::PrivateEditScope(_))));
    }
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[base[Q]]"
    );
}

#[test]
fn replacement_manifest_is_authoring_scoped_and_additions_rollback_safe() {
    let mut b = NetworkBuilder::new();
    let leaf = b.add("leaf", stage("old")).unwrap();
    let mut other = NetworkBuilder::new();
    let foreign = other.add("leaf", stage("foreign")).unwrap();
    assert!(matches!(
        b.allow_replace(&foreign),
        Err(phenix_graph::network::NetworkError::ForeignScope)
    ));
    let failed: Result<(), ()> = b.additions(|candidate| {
        candidate.allow_replace(&leaf).unwrap();
        Err(())
    });
    assert!(failed.is_err());
    assert!(b.build().unwrap().replacement_points().is_empty());

    let mut author = NetworkBuilder::new();
    let leaf = author.add("leaf", stage("old")).unwrap();
    author.allow_replace(&leaf).unwrap();
    let snapshot = author.build().unwrap();
    assert!(snapshot.replacement_points().contains("leaf"));
    let restored = NetworkBuilder::from_def(&snapshot).build().unwrap();
    assert_eq!(restored.replacement_points(), snapshot.replacement_points());
}

#[test]
fn ancestor_and_nested_replacements_conflict_unless_explicitly_dependent() {
    fn registration_named(
        id: &str,
        deps: &[&str],
        path: &str,
        definition: Arc<ComponentDef>,
        priority: i64,
    ) -> Registration {
        Registration::new(
            id,
            deps.iter().map(|id| (*id).to_owned()),
            priority,
            GraphOrigin::default().child(),
            [Footprint::Component(path.to_owned())],
            NetworkPatch::new([Edit::Replace {
                path: path.to_owned(),
                definition,
            }]),
        )
    }

    let parent = registration_named(
        "parent",
        &[],
        "m",
        Arc::new(ComponentDef::composite(outer(true, true))),
        0,
    );
    let child = registration_named("child", &[], "m/inner/leaf", stage("new"), 0);
    let mut overlapping = composer(true, true);
    assert!(matches!(
        overlapping.register([parent.clone(), child.clone()]),
        Err(ComposeError::Conflict { .. })
    ));
    assert_eq!(
        run(&mut overlapping.current().compiled().start()),
        "sibling[base[Q]]"
    );

    // Higher priority wins as a whole, never silently replays both edits.
    let mut winner = composer(true, true);
    let high_parent = registration_named(
        "parent",
        &[],
        "m",
        Arc::new(ComponentDef::composite(outer(true, true))),
        5,
    );
    winner.register([child.clone(), high_parent]).unwrap();
    assert_eq!(
        winner.current().active().iter().cloned().collect::<Vec<_>>(),
        vec!["parent"]
    );
    assert_eq!(
        run(&mut winner.current().compiled().start()),
        "sibling[base[Q]]"
    );

    // Dependency makes the order explicit; the narrowed child seam survives
    // replacement of the parent definition and is applied afterward.
    let mut dependent = composer(true, true);
    let dependent_child =
        registration_named("child", &["parent"], "m/inner/leaf", stage("new"), 0);
    dependent.register([dependent_child, parent]).unwrap();
    assert_eq!(
        run(&mut dependent.current().compiled().start()),
        "sibling[new[Q]]"
    );
}
