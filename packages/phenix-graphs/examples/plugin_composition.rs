//! Owned plugin composition driving the canonical NetworkSession, not the old engine.
use phenix_graph::compose::{
    ComposeError, Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration,
};
use phenix_graph::contract::Contract;
use phenix_graph::network::{ConnectionDef, NetworkBuilder, PortPath};
use phenix_graph::process::{Process, ProcessContext, ProcessEmission, Step};
use phenix_graph::runtime::{NetworkSession, SessionState};
use phenix_graph::{ComponentDef, GraphOrigin, Interface, PortDef, Value};
use std::num::NonZeroUsize;
use std::sync::Arc;

struct Transform {
    multiply: i64,
    add: i64,
    emitted: bool,
}
impl Process for Transform {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::of(value.get::<i64>()? * self.multiply + self.add),
        )))
    }
}
fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}
fn interface() -> Arc<Interface> {
    Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    )
}
fn transform(multiply: i64, add: i64) -> ComponentDef {
    ComponentDef::primitive(
        interface(),
        Arc::new(move || Transform {
            multiply,
            add,
            emitted: false,
        }),
    )
}
fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}
fn key(from: &str, to: &str) -> ConnectionKey {
    ConnectionKey::new(port(from, "out"), port(to, "in"))
}

// The host approves these known demo patches. Production hosts must inspect the
// payload and choose grants themselves, never accept grants from plugin data.
fn approve(
    id: &str,
    dependencies: &[&str],
    origin: GraphOrigin,
    edits: Vec<Edit>,
    extra: Vec<Footprint>,
) -> Registration {
    let patch = NetworkPatch::new(edits);
    let mut grants = patch.footprints();
    grants.extend(extra);
    Registration::new(
        id,
        dependencies.iter().map(|s| (*s).into()),
        0,
        origin,
        grants,
        patch,
    )
}
fn run(composer: &Composer, expected: i64) -> Result<(), Box<dyn std::error::Error>> {
    let mut session: NetworkSession = composer.current().compiled().start();
    session.inject("request", Value::of(2_i64))?;
    let report = session.run_until_idle(64);
    assert!(!report.budget_exhausted);
    let packet = session.take_output("reply")?.ok_or("missing reply")?;
    assert_eq!(packet.value().get::<i64>()?, expected);
    assert_eq!(
        packet.origin,
        composer.current().compiled().inspect("b")?.origin
    );
    session.close_input("request")?;
    assert_eq!(
        session.run_until_idle(64).state,
        SessionState::Completed
    );
    println!("Native session: input 2 -> {expected}");
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = GraphOrigin::default();
    let contract = Contract::new("demo/transform", interface())?;
    let a = approve(
        "A",
        &[],
        host.child(),
        vec![
            Edit::Add {
                path: "a".into(),
                definition: Arc::new(transform(1, 1).claim(&contract)?),
            },
            Edit::ExposeInput {
                name: "request".into(),
                targets: vec![port("a", "in")],
                capacity: capacity(),
            },
        ],
        vec![],
    );
    let b = approve(
        "B",
        &["A"],
        host.child(),
        vec![
            Edit::Add {
                path: "b".into(),
                definition: Arc::new(transform(2, 0)),
            },
            Edit::Connect(ConnectionDef {
                from: port("a", "out"),
                to: port("b", "in"),
                capacity: capacity(),
            }),
            Edit::ExposeOutput {
                name: "reply".into(),
                from: port("b", "out"),
            },
        ],
        vec![],
    );
    let mut composer = Composer::new(NetworkBuilder::new().with_origin(host.clone()).build()?)?;
    composer.register([b, a])?;
    run(&composer, 6)?;

    let mut child = NetworkBuilder::new();
    let leaf = child.add("leaf", Arc::new(transform(1, 10)))?;
    child.expose_input("in", leaf.wire_input::<i64>("in")?, capacity())?;
    child.expose_output("out", leaf.wire_output::<i64>("out")?)?;
    child.claim(&contract)?;
    let replacement_origin = host.child();
    let c = approve(
        "C",
        &["B"],
        replacement_origin.clone(),
        vec![Edit::Replace {
            path: "a".into(),
            definition: Arc::new(ComponentDef::composite(child.build()?)),
        }],
        vec![],
    );
    let x = approve(
        "X",
        &["C"],
        host.child(),
        vec![
            Edit::Add {
                path: "x".into(),
                definition: Arc::new(transform(3, 0)),
            },
            Edit::Splice {
                anchor: key("a", "b"),
                input: port("x", "in"),
                output: port("x", "out"),
            },
        ],
        vec![Footprint::Connection(key("a", "x"))],
    );
    let y = approve(
        "Y",
        &["X"],
        host.child(),
        vec![
            Edit::Add {
                path: "y".into(),
                definition: Arc::new(transform(1, 5)),
            },
            Edit::Splice {
                anchor: key("a", "b"),
                input: port("y", "in"),
                output: port("y", "out"),
            },
        ],
        vec![
            Footprint::Connection(key("x", "b")),
            Footprint::Connection(key("x", "y")),
        ],
    );
    composer.register([y, c, x])?;
    composer.current().compiled().bind("a", &contract)?;
    let actual = composer.current().compiled().inspect("a")?;
    assert_eq!(actual.declared_origin, Some(replacement_origin.clone()));
    assert_eq!(
        composer.current().compiled().inspect("a/leaf")?.origin,
        Some(replacement_origin)
    );
    println!("Cross-kind replacement plus ordered splices: a/leaf -> x -> y -> b");
    run(&composer, 82)?;

    let conflict = approve(
        "conflict",
        &["B"],
        host.child(),
        vec![Edit::Replace {
            path: "a".into(),
            definition: Arc::new(transform(1, 99).claim(&contract)?),
        }],
        vec![],
    );
    assert!(matches!(
        composer.register([conflict]),
        Err(ComposeError::Conflict { .. })
    ));
    println!("Equal-priority conflict rejected; published plan unchanged");
    run(&composer, 82)?;

    let removed = composer.revoke("C")?;
    assert_eq!(removed, ["C".into(), "X".into(), "Y".into()].into());
    assert!(composer.current().compiled().inspect("a/leaf").is_err());
    println!("Revoked C, X, Y; recomposed and recompiled base + A + B");
    run(&composer, 6)?;
    Ok(())
}
