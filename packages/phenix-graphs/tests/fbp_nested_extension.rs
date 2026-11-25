//! A1: private composite extension authority is an author opt-in plus exact
//! host grants, never authority inferred from a path or a replacement seam.
use phenix_graph::{
    ComponentDef, GraphOrigin, Value,
    compose::{ComposeError, Composer, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, NetworkDef, NetworkError, PortPath},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn cap() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}

fn stage(label: &'static str) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::map::<String, String, _>(move |text| {
        format!("{label}[{text}]")
    }))
}

fn inner(allow_extension: bool) -> NetworkDef {
    let mut builder = NetworkBuilder::new();
    let leaf = builder.add("leaf", stage("base")).unwrap();
    let sibling = builder.add("sibling", stage("sibling")).unwrap();
    builder
        .connect(
            leaf.output::<String>("output").unwrap(),
            sibling.input::<String>("input").unwrap(),
            cap(),
        )
        .unwrap();
    builder
        .expose_input("input", leaf.input::<String>("input").unwrap(), cap())
        .unwrap();
    builder
        .expose_output("output", sibling.output::<String>("output").unwrap())
        .unwrap();
    if allow_extension {
        builder.allow_extensions().unwrap();
    }
    builder.build().unwrap()
}

fn outer(allow_outer: bool, allow_inner: bool) -> NetworkDef {
    let mut builder = NetworkBuilder::new();
    let child = builder
        .add(
            "inner",
            Arc::new(ComponentDef::composite(inner(allow_inner))),
        )
        .unwrap();
    builder
        .expose_input("input", child.input::<String>("input").unwrap(), cap())
        .unwrap();
    builder
        .expose_output("output", child.output::<String>("output").unwrap())
        .unwrap();
    if allow_outer {
        builder.allow_extensions().unwrap();
    }
    builder.build().unwrap()
}

fn composer(allow_outer: bool, allow_inner: bool) -> Composer {
    let mut builder = NetworkBuilder::new();
    let module = builder
        .add(
            "m",
            Arc::new(ComponentDef::composite(outer(allow_outer, allow_inner))),
        )
        .unwrap();
    builder
        .expose_input("in", module.input::<String>("input").unwrap(), cap())
        .unwrap();
    builder
        .expose_output("out", module.output::<String>("output").unwrap())
        .unwrap();
    Composer::new(builder.build().unwrap()).unwrap()
}

fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}

fn connect(from: &str, out: &str, to: &str, input: &str) -> Edit {
    Edit::Connect(ConnectionDef {
        from: port(from, out),
        to: port(to, input),
        capacity: cap(),
    })
}

fn tap_patch(seen: Arc<AtomicUsize>) -> NetworkPatch {
    let sink = Arc::new(ComponentDef::sink(move |text: &String| {
        assert!(!text.is_empty());
        seen.fetch_add(1, Ordering::SeqCst);
    }));
    NetworkPatch::new([
        Edit::Add {
            path: "m/inner/tap".into(),
            definition: sink,
        },
        connect("m/inner/leaf", "output", "m/inner/tap", "input"),
    ])
}

fn register(
    id: &str,
    patch: NetworkPatch,
    grants: impl IntoIterator<Item = Footprint>,
) -> Registration {
    Registration::new(id, [], 0, GraphOrigin::default().child(), grants, patch)
}

fn run(session: &mut phenix_graph::runtime::NetworkSession) -> String {
    session
        .inject("in", Value::local(String::from("Q")))
        .unwrap();
    let report = session.run_until_idle(128);
    assert!(!report.budget_exhausted);
    session
        .take_output("out")
        .unwrap()
        .unwrap()
        .value()
        .borrow::<String>()
        .unwrap()
        .clone()
}

#[test]
fn approved_nested_add_and_connect_execute_with_provenance_and_safe_revoke() {
    let seen = Arc::new(AtomicUsize::new(0));
    let mut composer = composer(true, true);
    let mut old = composer.current().compiled().start();
    let patch = tap_patch(seen.clone());
    let origin = GraphOrigin::default().child();
    composer
        .register([Registration::new(
            "extension",
            [],
            0,
            origin.clone(),
            patch.footprints(),
            patch,
        )])
        .unwrap();

    assert_eq!(run(&mut old), "sibling[base[Q]]");
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[base[Q]]"
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);

    let compiled = composer.current().compiled();
    assert_eq!(
        compiled.inspect("m/inner/tap").unwrap().declared_origin,
        Some(origin)
    );
    for unchanged in ["m", "m/inner", "m/inner/leaf", "m/inner/sibling"] {
        assert!(
            compiled
                .inspect(unchanged)
                .unwrap()
                .declared_origin
                .is_none()
        );
    }
    assert!(composer.revoke("extension").unwrap().contains("extension"));
    assert_eq!(composer.current().compiled().processes().len(), 2);
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[base[Q]]"
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}

#[test]
fn each_enclosing_author_must_opt_in_and_every_write_needs_its_exact_grant() {
    for (outer, inner) in [(false, false), (true, false), (false, true)] {
        let mut composer = composer(outer, inner);
        let patch = tap_patch(Arc::new(AtomicUsize::new(0)));
        let before = composer.current().compiled().processes().len();
        let result = composer.register([register("private", patch.clone(), patch.footprints())]);
        assert!(matches!(result, Err(ComposeError::PrivateEditScope(_))));
        assert_eq!(composer.current().compiled().processes().len(), before);
        assert!(composer.current().active().is_empty());
    }
    let mut composer = composer(true, true);
    let patch = tap_patch(Arc::new(AtomicUsize::new(0)));
    assert!(matches!(
        composer.register([register(
            "missing-edge-grant",
            patch,
            [Footprint::Component("m/inner/tap".into())],
        )]),
        Err(ComposeError::Unauthorized {
            footprint: Footprint::Connection(_),
            ..
        })
    ));
    assert_eq!(composer.current().compiled().processes().len(), 2);
    assert!(composer.current().active().is_empty());
}

#[test]
fn invalid_nested_edges_and_cross_boundary_wiring_fail_atomically() {
    let mut composer = composer(true, true);
    let published = composer.current().compiled().clone();
    let patch = NetworkPatch::new([
        Edit::Add {
            path: "m/inner/tap".into(),
            definition: Arc::new(ComponentDef::sink(|_: &String| {})),
        },
        connect("m/inner/leaf", "missing", "m/inner/tap", "input"),
    ]);
    let result = composer.register([register("invalid", patch.clone(), patch.footprints())]);
    assert!(matches!(
        result,
        Err(ComposeError::Network(NetworkError::UnknownOutput(_)))
    ));
    assert_eq!(
        composer.current().compiled().processes().len(),
        published.processes().len()
    );
    assert_eq!(
        composer.current().compiled().connections(),
        published.connections()
    );
    assert!(composer.current().active().is_empty());
    assert!(composer.revoke("invalid").unwrap().is_empty());

    let cross = NetworkPatch::new([connect("m/inner/leaf", "output", "m/inner", "input")]);
    assert!(matches!(
        composer.register([register("cross", cross.clone(), cross.footprints())]),
        Err(ComposeError::PrivateEditScope(_))
    ));
    let malformed = NetworkPatch::new([Edit::Add {
        path: "m//tap".into(),
        definition: stage("bad"),
    }]);
    assert!(matches!(
        composer.register([register(
            "malformed",
            malformed.clone(),
            malformed.footprints()
        )]),
        Err(ComposeError::PrivateEditScope(_))
    ));
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[base[Q]]"
    );
}

#[test]
fn independent_ancestor_replacement_and_nested_connection_conflict() {
    let mut composer = composer(true, true);
    let replace = NetworkPatch::new([Edit::Replace {
        path: "m".into(),
        definition: Arc::new(ComponentDef::composite(outer(true, true))),
    }]);
    let internal = NetworkPatch::new([connect(
        "m/inner/leaf",
        "output",
        "m/inner/sibling",
        "input",
    )]);
    let result = composer.register([
        register("ancestor", replace.clone(), replace.footprints()),
        register("nested-connection", internal.clone(), internal.footprints()),
    ]);
    assert!(matches!(result, Err(ComposeError::Conflict { .. })));
    assert!(composer.current().active().is_empty());
    assert_eq!(
        run(&mut composer.current().compiled().start()),
        "sibling[base[Q]]"
    );
}

#[test]
fn extension_opt_in_survives_roundtrip_and_is_rollback_safe() {
    let mut builder = NetworkBuilder::new();
    let result: Result<(), ()> = builder.additions(|candidate| {
        candidate.allow_extensions().unwrap();
        Err(())
    });
    assert!(result.is_err());
    assert!(!builder.build().unwrap().extensions_allowed());

    let mut builder = NetworkBuilder::new();
    builder.allow_extensions().unwrap();
    let definition = builder.build().unwrap();
    assert!(definition.extensions_allowed());
    assert!(
        NetworkBuilder::from_def(&definition)
            .build()
            .unwrap()
            .extensions_allowed()
    );
}
