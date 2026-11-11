//! Ordinary plugins author, replace, splice and revoke high-level agent stages.
//! Deterministic string stages prove topology and authority, not LLM features.
use phenix_graph::{
    ComponentDef, GraphOrigin, Value,
    compose::{Composer, ConnectionKey, Edit, Footprint, NetworkPatch, Registration},
    network::{ConnectionDef, NetworkBuilder, PortPath},
};
use std::{num::NonZeroUsize, sync::Arc};

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}

fn stage(name: &'static str) -> Arc<ComponentDef> {
    Arc::new(ComponentDef::map::<String, String, _>(move |input| {
        format!("{name}[{input}]")
    }))
}

fn path(component: &str, port: &str) -> PortPath {
    PortPath {
        process: component.to_owned(),
        port: port.to_owned(),
    }
}

fn edge(from: &str, to: &str) -> ConnectionDef {
    ConnectionDef {
        from: path(from, "output"),
        to: path(to, "input"),
        capacity: capacity(),
    }
}

fn key(from: &str, to: &str) -> ConnectionKey {
    ConnectionKey::new(path(from, "output"), path(to, "input"))
}

fn registration(
    id: &str,
    dependencies: &[&str],
    origin: GraphOrigin,
    edits: Vec<Edit>,
    extra_grants: impl IntoIterator<Item = Footprint>,
) -> Registration {
    // The test host explicitly approves known changes. A production host must
    // inspect grants independently, not trust a plugin-supplied footprint list.
    let patch = NetworkPatch::new(edits);
    let mut grants = patch.footprints();
    grants.extend(extra_grants);
    Registration::new(
        id,
        dependencies.iter().map(|id| (*id).to_owned()),
        0,
        origin,
        grants,
        patch,
    )
}

fn execute(composer: &Composer) -> String {
    let mut session = composer.current().compiled().start();
    session
        .inject("prompt", Value::local(String::from("Q")))
        .unwrap();
    let result = session.run_until_idle(128);
    assert!(!result.budget_exhausted);
    let packet = session.take_output("reply").unwrap().unwrap();
    packet.value().borrow::<String>().unwrap().clone()
}

#[test]
fn unprivileged_preset_and_independent_plugins_edit_agent_semantics() {
    let origin = GraphOrigin::default();
    // The kernel starts with no graph and no distinguished agent component.
    let mut composer = Composer::new(
        NetworkBuilder::new()
            .with_origin(origin.clone())
            .build()
            .unwrap(),
    )
    .unwrap();
    assert!(composer.current().compiled().processes().is_empty());

    let mut preset_edits = Vec::new();
    for name in ["memory", "model", "tools", "reply"] {
        preset_edits.push(Edit::Add {
            path: name.to_owned(),
            definition: stage(name),
        });
    }
    for (from, to) in [("memory", "model"), ("model", "tools"), ("tools", "reply")] {
        preset_edits.push(Edit::Connect(edge(from, to)));
    }
    preset_edits.extend([
        Edit::ExposeInput {
            name: "prompt".to_owned(),
            targets: vec![path("memory", "input")],
            capacity: capacity(),
        },
        Edit::ExposeOutput {
            name: "reply".to_owned(),
            from: path("reply", "output"),
        },
    ]);
    let preset = registration("preset", &[], origin.child(), preset_edits, []);
    composer.register([preset]).unwrap();
    assert_eq!(execute(&composer), "reply[tools[model[memory[Q]]]]");

    // A provider plugin replaces the model without changing the interface.
    let provider = registration(
        "provider",
        &["preset"],
        origin.child(),
        vec![Edit::Replace {
            path: "model".to_owned(),
            definition: stage("alternative-model"),
        }],
        [],
    );
    // An independent compaction plugin injects a new ordinary process.
    let compaction = registration(
        "compaction",
        &["preset"],
        origin.child(),
        vec![
            Edit::Add {
                path: "compaction".to_owned(),
                definition: stage("compaction"),
            },
            Edit::Splice {
                anchor: key("memory", "model"),
                input: path("compaction", "input"),
                output: path("compaction", "output"),
            },
        ],
        [Footprint::Connection(key("memory", "compaction"))],
    );
    composer.register([compaction, provider]).unwrap();
    assert_eq!(
        execute(&composer),
        "reply[tools[alternative-model[compaction[memory[Q]]]]]"
    );

    composer.revoke("compaction").unwrap();
    assert_eq!(
        execute(&composer),
        "reply[tools[alternative-model[memory[Q]]]]"
    );
    // Removing the unprivileged preset transitively deactivates its dependents.
    composer.revoke("preset").unwrap();
    assert!(composer.current().active().is_empty());
    assert!(composer.current().compiled().processes().is_empty());
}
