//! Compare the fake model's actual serialized view with independently
//! constructed expectations. These fixtures contain synthetic data only.

use phenix_agent::{
    AgentEvent,
    frontend::run_turn,
    harness::{self, Environment, Extension},
    model::{
        context::{ModelContext, Skill},
        fake::provider,
    },
    tools::{Tool, ToolDescriptor},
};
use phenix_graph::{ContractValue, ValidatedResourceSet};
use phenix_kernel::Key;
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::collections::BTreeMap;

#[derive(ContractValue)]
#[contract(schema = "test.find.args")]
struct FindArgs {
    query: String,
    limit: i64,
}

struct SearchTools;

impl PluginInstance for SearchTools {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Tool::new("find", |args: FindArgs| Ok(args.query))
            .description("Search the selected environment")
            .placement("search-find")
            .register(registrar)?;
        Tool::new("count", |args: String| Ok(args.len() as i64))
            .description("Count bytes")
            .placement("search-count")
            .register(registrar)
    }
}

struct Skills;

impl PluginInstance for Skills {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(skill(
            "review",
            "Review code",
            "Check invariants before edits.",
        ))?;
        registrar.add(skill("private", "Not selected", "UNSELECTED-SKILL"))
    }
}

fn skill(id: &str, description: &str, instructions: &str) -> Skill {
    Skill {
        id: id.into(),
        description: description.into(),
        instructions: instructions.into(),
    }
}

fn descriptor<A: ContractValue, R: ContractValue>(name: &str, description: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.into(),
        description: description.into(),
        arguments: A::schema(),
        result: R::schema(),
    }
}

fn dump(
    local: Environment,
    additional: BTreeMap<String, Environment>,
    extensions: Vec<Extension>,
    expected: ModelContext,
    exercise_plugins: bool,
) {
    let host = harness::compose(
        |context| provider(context.clone()),
        local,
        additional,
        extensions,
    )
    .unwrap();
    if exercise_plugins {
        assert!(
            host.plugins()
                .snapshots()
                .any(|s| s.name() == "search-plugin")
        );
        assert!(
            host.plugins()
                .snapshots()
                .any(|s| s.name() == "skills-plugin")
        );
    }
    let generation = host.active().unwrap();
    let mut events = Vec::new();
    run_turn(&generation, "inspect context".into(), 256, |event| {
        events.push(event.clone())
    })
    .unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Delta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 1);
    println!("[model-visible-context]\n{}", deltas[0]);
    assert_eq!(deltas[0], format!("{expected:#?}"));
    assert!(!deltas[0].contains("UNSELECTED-SKILL"));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolRequested(_)))
    );
    assert_eq!(events.last(), Some(&AgentEvent::Completed));
}

#[test]
fn empty_context_is_visible_without_requiring_a_scripted_tool() {
    dump(
        Environment::new(),
        BTreeMap::new(),
        Vec::new(),
        ModelContext {
            tools: BTreeMap::from([("local".into(), BTreeMap::new())]),
            ..Default::default()
        },
        false,
    );
}

#[test]
fn independently_contributed_tools_and_skills_in_both_profiles() {
    for advanced in [false, true] {
        let local = Environment::new().with_tool(
            Tool::new("echo", Ok::<String, String>).description("Echo text"),
        );
        let search = Environment::new().with_plugin(
            "search-plugin",
            SearchTools,
            BTreeMap::from([
                ("find".into(), ValidatedResourceSet::default()),
                ("count".into(), ValidatedResourceSet::default()),
            ]),
        );
        let mut expected = ModelContext {
            tools: BTreeMap::from([(
                "local".into(),
                BTreeMap::from([(
                    "echo".into(),
                    descriptor::<String, String>("echo", "Echo text"),
                )]),
            )]),
            skills: BTreeMap::from([(
                Key::new("skills-plugin", "review"),
                skill("review", "Review code", "Check invariants before edits."),
            )]),
        };
        let search_descriptors = BTreeMap::from([
            (
                "find".into(),
                descriptor::<FindArgs, String>("find", "Search the selected environment"),
            ),
            (
                "count".into(),
                descriptor::<String, i64>("count", "Count bytes"),
            ),
        ]);
        let (local, additional) = if advanced {
            expected.tools.insert("reference".into(), search_descriptors);
            expected.tools.insert("empty".into(), BTreeMap::new());
            (
                local,
                BTreeMap::from([
                    ("reference".into(), search),
                    ("empty".into(), Environment::new()),
                ]),
            )
        } else {
            // The same plugin is usable directly in P1; no environment wrapper
            // or tool-specific integration code.
            expected
                .tools
                .get_mut("local")
                .unwrap()
                .extend(search_descriptors);
            (
                local.with_plugin(
                    "search-plugin",
                    SearchTools,
                    BTreeMap::from([
                        ("find".into(), ValidatedResourceSet::default()),
                        ("count".into(), ValidatedResourceSet::default()),
                    ]),
                ),
                BTreeMap::new(),
            )
        };
        let skills =
            Extension::new("skills-plugin", Skills, |_, _| Ok(())).with_skill("review");
        dump(local, additional, vec![skills], expected, true);
    }
}
