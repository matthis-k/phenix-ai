//! Nested exports are explicit author-owned extension seams, not root aliases.
use phenix_graph::{
    ComponentDef, GraphOrigin, Value,
    compose::{ComposeError, Composer, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, NetworkDef, PortPath},
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
    Arc::new(ComponentDef::map::<String, String, _>(move |s| {
        format!("{label}[{s}]")
    }))
}

fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}

fn edge(from: &str, to: &str) -> Edit {
    Edit::Connect(ConnectionDef {
        from: port(from, "output"),
        to: port(to, "input"),
        capacity: cap(),
    })
}

fn inner(allow: bool) -> NetworkDef {
    let mut b = NetworkBuilder::new();
    let leaf = b.add("leaf", stage("base")).unwrap();
    b.expose_input("input", leaf.input::<String>("input").unwrap(), cap())
        .unwrap();
    b.expose_output("output", leaf.output::<String>("output").unwrap())
        .unwrap();
    if allow {
        b.allow_extensions().unwrap();
    }
    b.build().unwrap()
}

fn composer(outer_extension: bool, inner_extension: bool) -> Composer {
    let mut outer = NetworkBuilder::new();
    let child = outer
        .add(
            "inner",
            Arc::new(ComponentDef::composite(inner(inner_extension))),
        )
        .unwrap();
    outer
        .expose_input("input", child.input::<String>("input").unwrap(), cap())
        .unwrap();
    outer
        .expose_output("output", child.output::<String>("output").unwrap())
        .unwrap();
    if outer_extension {
        outer.allow_extensions().unwrap();
    }

    let mut root = NetworkBuilder::new();
    let module = root
        .add(
            "m",
            Arc::new(ComponentDef::composite(outer.build().unwrap())),
        )
        .unwrap();
    root.expose_input("in", module.input::<String>("input").unwrap(), cap())
        .unwrap();
    root.expose_output("out", module.output::<String>("output").unwrap())
        .unwrap();
    Composer::new(root.build().unwrap()).unwrap()
}

fn registration(
    id: &str,
    deps: &[&str],
    patch: NetworkPatch,
    grants: impl IntoIterator<Item = Footprint>,
) -> Registration {
    Registration::new(
        id,
        deps.iter().map(|id| (*id).to_owned()),
        0,
        GraphOrigin::default().child(),
        grants,
        patch,
    )
}

fn run(session: &mut phenix_graph::runtime::NetworkSession) -> String {
    session
        .inject("in", Value::local(String::from("Q")))
        .unwrap();
    assert!(!session.run_until_idle(256).budget_exhausted);
    session
        .take_output("out")
        .unwrap()
        .unwrap()
        .value()
        .borrow::<String>()
        .unwrap()
        .clone()
}

fn output() -> Edit {
    Edit::ExposeNestedOutput {
        parent: "m/inner".into(),
        name: "telemetry".into(),
        from: port("m/inner/side", "output"),
    }
}

#[test]
fn nested_output_can_wire_into_enclosing_composite_without_changing_root_interface() {
    let seen = Arc::new(AtomicUsize::new(0));
    let observed = seen.clone();
    let mut composer = composer(true, true);
    let mut old = composer.current().compiled().start();

    let patch = NetworkPatch::new([
        Edit::Add {
            path: "m/inner/side".into(),
            definition: stage("side"),
        },
        edge("m/inner/leaf", "m/inner/side"),
        output(),
        Edit::Add {
            path: "m/monitor".into(),
            definition: Arc::new(ComponentDef::sink(move |s: &String| {
                assert_eq!(s, "side[base[Q]]");
                observed.fetch_add(1, Ordering::SeqCst);
            })),
        },
        Edit::Connect(ConnectionDef {
            from: port("m/inner", "telemetry"),
            to: port("m/monitor", "input"),
            capacity: cap(),
        }),
    ]);
    composer
        .register([registration(
            "extend",
            &[],
            patch.clone(),
            patch.footprints(),
        )])
        .unwrap();
    let compiled = composer.current().compiled();
    assert!(
        compiled
            .inspect("m/inner")
            .unwrap()
            .interface
            .output("telemetry")
            .is_some()
    );
    assert!(
        compiled
            .inspect("m")
            .unwrap()
            .interface
            .output("telemetry")
            .is_none()
    );
    assert_eq!(run(&mut old), "base[Q]");
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(run(&mut compiled.start()), "base[Q]");
    assert_eq!(seen.load(Ordering::SeqCst), 1);

    composer.revoke("extend").unwrap();
    assert!(
        composer
            .current()
            .compiled()
            .inspect("m/inner")
            .unwrap()
            .interface
            .output("telemetry")
            .is_none()
    );
    assert_eq!(run(&mut composer.current().compiled().start()), "base[Q]");
}

#[test]
fn nested_input_declares_only_local_projection_and_leaves_root_boundary_unchanged() {
    let mut composer = composer(true, true);
    let patch = NetworkPatch::new([Edit::ExposeNestedInput {
        parent: "m/inner".into(),
        name: "alternative".into(),
        targets: vec![port("m/inner/leaf", "input")],
        capacity: cap(),
    }]);
    composer
        .register([registration(
            "input",
            &[],
            patch.clone(),
            patch.footprints(),
        )])
        .unwrap();
    let compiled = composer.current().compiled();
    assert!(
        compiled
            .inspect("m/inner")
            .unwrap()
            .interface
            .input("alternative")
            .is_some()
    );
    assert!(
        compiled
            .inspect("m")
            .unwrap()
            .interface
            .input("alternative")
            .is_none()
    );
    assert_eq!(run(&mut compiled.start()), "base[Q]");
}

#[test]
fn private_export_refusals_require_every_author_opt_in_and_exact_host_grants() {
    for (outer, inner) in [(false, false), (true, false), (false, true)] {
        let mut composer = composer(outer, inner);
        let patch = NetworkPatch::new([Edit::ExposeNestedOutput {
            parent: "m/inner".into(),
            name: "telemetry".into(),
            from: port("m/inner/leaf", "output"),
        }]);
        assert!(matches!(
            composer.register([registration(
                "denied",
                &[],
                patch.clone(),
                patch.footprints()
            )]),
            Err(ComposeError::PrivateEditScope(_))
        ));
    }

    let mut composer = composer(true, true);
    let patch = NetworkPatch::new([Edit::ExposeNestedOutput {
        parent: "m/inner".into(),
        name: "telemetry".into(),
        from: port("m/inner/leaf", "output"),
    }]);
    assert!(matches!(
        composer.register([registration("ungranted", &[], patch, [])]),
        Err(ComposeError::Unauthorized {
            footprint: Footprint::NestedOutputExport { .. },
            ..
        })
    ));

    for from in ["m/inner/leaf/child", "m/other", "m/inner", "m/inner2/leaf"] {
        let patch = NetworkPatch::new([Edit::ExposeNestedOutput {
            parent: "m/inner".into(),
            name: "telemetry".into(),
            from: port(from, "output"),
        }]);
        assert!(matches!(
            composer.register([registration(
                "cross-scope",
                &[],
                patch.clone(),
                patch.footprints()
            )]),
            Err(ComposeError::PrivateEditScope(_))
        ));
    }
    assert!(composer.current().active().is_empty());
    assert_eq!(run(&mut composer.current().compiled().start()), "base[Q]");
}

#[test]
fn invalid_nested_output_is_atomic_and_does_not_mutate_previous_generation() {
    let mut composer = composer(true, true);
    let previous = composer.current().compiled().clone();
    let patch = NetworkPatch::new([Edit::ExposeNestedOutput {
        parent: "m/inner".into(),
        name: "telemetry".into(),
        from: port("m/inner/leaf", "missing"),
    }]);
    assert!(matches!(
        composer.register([registration(
            "broken",
            &[],
            patch.clone(),
            patch.footprints()
        )]),
        Err(ComposeError::InvalidEdit { .. })
    ));
    assert_eq!(
        composer.current().compiled().connections(),
        previous.connections()
    );
    assert!(
        composer
            .current()
            .compiled()
            .inspect("m/inner")
            .unwrap()
            .interface
            .output("telemetry")
            .is_none()
    );
    assert!(composer.current().active().is_empty());
}

#[test]
fn exports_require_explicit_dependency_on_new_component_and_on_ancestor_replacement() {
    let mut composer = composer(true, true);
    let add = NetworkPatch::new([Edit::Add {
        path: "m/inner/side".into(),
        definition: stage("side"),
    }]);
    let export = NetworkPatch::new([output()]);
    assert!(matches!(
        composer.register([
            registration("a-add", &[], add.clone(), add.footprints()),
            registration("z-export", &[], export.clone(), export.footprints()),
        ]),
        Err(ComposeError::Conflict { .. })
    ));

    composer
        .register([
            registration("a-add", &[], add.clone(), add.footprints()),
            registration("z-export", &["a-add"], export.clone(), export.footprints()),
        ])
        .unwrap();
    assert!(
        composer
            .current()
            .compiled()
            .inspect("m/inner")
            .unwrap()
            .interface
            .output("telemetry")
            .is_some()
    );
    let removed = composer.revoke("a-add").unwrap();
    assert!(removed.contains("z-export"));
    assert!(
        composer
            .current()
            .compiled()
            .inspect("m/inner")
            .unwrap()
            .interface
            .output("telemetry")
            .is_none()
    );

    let replacement = NetworkPatch::new([Edit::Replace {
        path: "m".into(),
        definition: Arc::new(ComponentDef::composite({
            let mut outer = NetworkBuilder::new();
            outer.allow_extensions().unwrap();
            let child = outer
                .add("inner", Arc::new(ComponentDef::composite(inner(true))))
                .unwrap();
            outer
                .expose_input("input", child.input::<String>("input").unwrap(), cap())
                .unwrap();
            outer
                .expose_output("output", child.output::<String>("output").unwrap())
                .unwrap();
            outer.build().unwrap()
        })),
    }]);
    assert!(matches!(
        composer.register([
            registration(
                "a-replace",
                &[],
                replacement.clone(),
                replacement.footprints()
            ),
            registration("z-export", &[], export.clone(), export.footprints()),
        ]),
        Err(ComposeError::Conflict { .. })
    ));
    assert!(composer.current().active().is_empty());
}

#[test]
fn nested_export_grants_cannot_be_reused_as_identically_named_root_grants() {
    let mut composer = composer(true, true);
    let nested_output = NetworkPatch::new([Edit::ExposeNestedOutput {
        parent: "m/inner".into(),
        name: "telemetry".into(),
        from: port("m/inner/leaf", "output"),
    }]);
    let root_output = NetworkPatch::new([Edit::ExposeOutput {
        name: "m/inner/telemetry".into(),
        from: port("m", "output"),
    }]);
    assert_ne!(nested_output.footprints(), root_output.footprints());
    assert!(matches!(
        composer.register([registration(
            "false-output-grant",
            &[],
            nested_output.clone(),
            root_output.footprints(),
        )]),
        Err(ComposeError::Unauthorized {
            footprint: Footprint::NestedOutputExport { .. },
            ..
        })
    ));
    assert!(matches!(
        composer.register([registration(
            "false-root-grant",
            &[],
            root_output.clone(),
            nested_output.footprints(),
        )]),
        Err(ComposeError::Unauthorized {
            footprint: Footprint::OutputExport(_),
            ..
        })
    ));

    let nested_input = NetworkPatch::new([Edit::ExposeNestedInput {
        parent: "m/inner".into(),
        name: "alternate".into(),
        targets: vec![port("m/inner/leaf", "input")],
        capacity: cap(),
    }]);
    let root_input = NetworkPatch::new([Edit::ExposeInput {
        name: "m/inner/alternate".into(),
        targets: vec![port("m", "input")],
        capacity: cap(),
    }]);
    assert_ne!(nested_input.footprints(), root_input.footprints());
    assert!(matches!(
        composer.register([registration(
            "false-input-grant",
            &[],
            nested_input.clone(),
            root_input.footprints(),
        )]),
        Err(ComposeError::Unauthorized {
            footprint: Footprint::NestedInputExport { .. },
            ..
        })
    ));
    assert!(composer.current().active().is_empty());
}
