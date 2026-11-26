//! Actual Kernel prefixes and exact revision ownership, using baseline public APIs.
use phenix_graph::{
    ComponentDef, Contract, GraphOrigin, Value,
    compose::{Edit, Footprint, NetworkPatch},
    network::{CompiledNetwork, PortPath},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, ContractDefinition, Contribution, InputEvent,
    Kernel, Key, Producer,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, PluginSnapshot, Registrar};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub fn owner(group: usize, step: usize) -> String {
    format!("g{group:03}-{}", ["0p", "1q", "2Q", "3P", "4R", "5s"][step])
}
fn path(group: usize, step: usize) -> String {
    format!("{}{group:03}", ["a", "b", "a", "x", "b", "s"][step])
}
fn selected(groups: usize) -> BTreeMap<String, String> {
    (0..groups)
        .flat_map(|g| [0, 3, 4, 5].map(move |s| (path(g, s), owner(g, s))))
        .collect()
}
fn contract() -> Contract {
    Contract::new(
        "q3/recovery",
        ComponentDef::map(|n: i64| n).interface().clone(),
    )
    .unwrap()
}

#[derive(Default)]
pub struct Trace {
    events: Mutex<Vec<(String, &'static str)>>,
    roots: Mutex<BTreeMap<String, GraphOrigin>>,
    origins: Mutex<BTreeMap<String, GraphOrigin>>,
}
impl Trace {
    fn record(&self, owner: &str, kind: &'static str) {
        self.events.lock().unwrap().push((owner.into(), kind));
    }
    pub fn owners(&self, kind: &str) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, k)| *k == kind)
            .map(|(o, _)| o.clone())
            .collect()
    }
    fn network(&self, network: &CompiledNetwork, expected: &BTreeMap<String, String>) {
        assert_eq!(network.processes().len(), expected.len());
        assert_eq!(
            network
                .processes()
                .iter()
                .map(|p| p.path().to_owned())
                .collect::<BTreeSet<_>>(),
            expected.keys().cloned().collect()
        );
        assert_eq!(
            network.inputs().keys().cloned().collect::<BTreeSet<_>>(),
            expected.keys().map(|p| format!("in_{p}")).collect()
        );
        assert_eq!(
            network.outputs().keys().cloned().collect::<BTreeSet<_>>(),
            expected.keys().map(|p| format!("out_{p}")).collect()
        );
        let roots = self.roots.lock().unwrap();
        let mut origins = self.origins.lock().unwrap();
        for (path, owner) in expected {
            let declaration = network.inspect(path).unwrap();
            let origin = declaration.declared_origin.unwrap();
            assert_eq!(declaration.origin, Some(origin.clone()));
            assert!(origin.belongs_to(&roots[owner]));
            assert_ne!(origin, roots[owner]);
            assert_eq!(
                &origin,
                origins
                    .entry(owner.clone())
                    .or_insert_with(|| origin.clone())
            );
            for (other, root) in roots.iter().filter(|(other, _)| *other != owner) {
                assert!(
                    !origin.belongs_to(root),
                    "{owner} acquired {other}'s origin"
                );
            }
            assert_eq!(
                network
                    .processes()
                    .iter()
                    .find(|p| p.path() == path)
                    .unwrap()
                    .origin(),
                Some(&origin)
            );
        }
    }
    fn prefix(&self, network: &CompiledNetwork, group: usize, step: usize, preview: bool) {
        let mut expected = selected(group);
        let current: &[usize] = match step {
            0 => &[],
            1 => &[0],
            2 => &[0, 1],
            3 | 4 => &[2, 1],
            5 => &[0, 3, 4],
            _ => unreachable!(),
        };
        expected.extend(current.iter().map(|&s| (path(group, s), owner(group, s))));
        if step == 5 && preview {
            expected.insert(path(group, 5), owner(group, 5));
        }
        self.network(network, &expected);
    }

    /// A real Loaded revision drops contributions, implementation, then factory.
    /// The trace contains flags/origin tokens, never a snapshot or code lease.
    pub fn assert_released(&self, groups: usize, prepared: &[String]) {
        let events = self.events.lock().unwrap();
        for g in 0..groups {
            for s in 0..6 {
                let name = owner(g, s);
                let positions = |kind| {
                    events
                        .iter()
                        .enumerate()
                        .filter_map(|(i, (o, k))| (o == &name && *k == kind).then_some(i))
                        .collect::<Vec<_>>()
                };
                let revision = positions("revision");
                let code = positions("code");
                assert_eq!(revision.len(), 1, "{name}: {events:?}");
                assert_eq!(code.len(), 1, "{name}: {events:?}");
                assert!(revision[0] < code[0]);
                for kind in ["static", "executable", "artifact", "activation"] {
                    let drops = positions(kind);
                    let expected = usize::from(kind == "static" || prepared.contains(&name));
                    assert_eq!(drops.len(), expected, "{name}/{kind}: {events:?}");
                    if expected != 0 {
                        assert!(drops[0] < revision[0], "{name}/{kind}: {events:?}");
                    }
                }
                for drop in positions("producer") {
                    assert!(drop < revision[0]);
                }
            }
        }
    }
}

struct Life {
    owner: String,
    kind: &'static str,
    alive: Arc<AtomicBool>,
    trace: Arc<Trace>,
}
impl Life {
    fn touch(&self) {
        assert!(self.alive.load(Ordering::SeqCst));
    }
}
impl Drop for Life {
    fn drop(&mut self) {
        self.touch();
        self.trace.record(&self.owner, self.kind);
        self.alive.store(false, Ordering::SeqCst);
    }
}
struct Mark {
    owner: String,
    kind: &'static str,
    revision: Arc<AtomicBool>,
    code: Arc<AtomicBool>,
    trace: Arc<Trace>,
}
impl Mark {
    fn touch(&self) {
        assert!(
            self.revision.load(Ordering::SeqCst),
            "{} outlived revision",
            self.kind
        );
        assert!(
            self.code.load(Ordering::SeqCst),
            "{} outlived code",
            self.kind
        );
    }
}
impl Drop for Mark {
    fn drop(&mut self) {
        self.touch();
        self.trace.record(&self.owner, self.kind);
    }
}
struct Idle(Mark);
impl Producer for Idle {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        self.0.touch();
        Ok(None)
    }
}

struct Plugin {
    group: usize,
    step: usize,
    previews: bool,
    revision: Life,
    code: Arc<AtomicBool>,
}
fn patch(group: usize, step: usize, mark: Mark, prepared: bool) -> NetworkPatch {
    let name = path(group, step);
    let mut definition = ComponentDef::map(move |n: i64| {
        mark.touch();
        mark.trace.record(
            &mark.owner,
            if prepared { "effect" } else { "static_effect" },
        );
        n + if prepared { step as i64 + 1 } else { -100 }
    });
    if step == 5 {
        definition = definition.claim(&contract()).unwrap();
    }
    NetworkPatch::new([
        Edit::Add {
            path: name.clone(),
            definition: Arc::new(definition),
        },
        Edit::ExposeInput {
            name: format!("in_{name}"),
            targets: vec![PortPath {
                process: name.clone(),
                port: "input".into(),
            }],
            capacity: NonZeroUsize::new(1).unwrap(),
        },
        Edit::ExposeOutput {
            name: format!("out_{name}"),
            from: PortPath {
                process: name,
                port: "output".into(),
            },
        },
    ])
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        self.revision.touch();
        let group = self.group;
        let step = self.step;
        let previews = self.previews;
        let name = owner(group, step);
        let trace = self.revision.trace.clone();
        let revision = self.revision.alive.clone();
        let code = self.code.clone();
        let mark = |kind| Mark {
            owner: name.clone(),
            kind,
            revision: revision.clone(),
            code: code.clone(),
            trace: trace.clone(),
        };
        let mut contribution = Contribution::new("main", patch(group, step, mark("static"), false));
        if step == 2 {
            contribution = contribution.after([Key::new(owner(group, 1), "main")]);
        }
        if step == 3 {
            contribution = contribution.after([Key::new(owner(group, 0), "main")]);
        }
        // Only the stable witness publishes an offer; no defeated offer is validated.
        if step == 5 && previews {
            registrar.add(ContractDefinition {
                id: "contract".into(),
                contract: contract(),
            })?;
            contribution = contribution
                .offer("offer", path(group, step), Key::new(&name, "contract"))
                .import("self", Key::new(&name, "contract"));
        }
        contribution
            .prepare(move |context| {
                trace.record(&name, "prepare");
                assert!(revision.load(Ordering::SeqCst) && code.load(Ordering::SeqCst));
                trace.prefix(context.inspect_network()?, group, step, previews);
                if step == 5 && previews {
                    assert_eq!(context.bind_import("self")?.path(), path(group, step));
                }
                let mark = |kind| Mark {
                    owner: name.clone(),
                    kind,
                    revision: revision.clone(),
                    code: code.clone(),
                    trace: trace.clone(),
                };
                let mut artifacts = CandidateArtifacts {
                    patch: Some(patch(group, step, mark("executable"), true)),
                    ..Default::default()
                };
                artifacts.export(mark("artifact"))?;
                let activation = mark("activation");
                artifacts.activate(move || {
                    activation.touch();
                    activation.trace.record(&activation.owner, "activate");
                    Ok(Box::new(Idle(Mark {
                        owner: activation.owner.clone(),
                        kind: "producer",
                        revision: activation.revision.clone(),
                        code: activation.code.clone(),
                        trace: activation.trace.clone(),
                    })))
                })?;
                Ok(artifacts)
            })
            .register(registrar)
    }
}

pub fn fixture(groups: usize, previews: bool) -> (Vec<PluginSnapshot>, BuildOptions, Arc<Trace>) {
    let trace = Arc::new(Trace::default());
    let mut options = BuildOptions::default();
    let mut snapshots = Vec::new();
    for group in 0..groups {
        for step in 0..6 {
            let name = owner(group, step);
            let revision = Arc::new(AtomicBool::new(true));
            let code = Arc::new(AtomicBool::new(true));
            let lease = Life {
                owner: name.clone(),
                kind: "code",
                alive: code.clone(),
                trace: trace.clone(),
            };
            let mut catalog = PluginCatalog::new();
            let key = catalog
                .register(&name, move |plugin: Plugin| {
                    lease.touch();
                    Ok(plugin)
                })
                .unwrap();
            let snapshot = catalog
                .instance()
                .load(
                    &key,
                    Plugin {
                        group,
                        step,
                        previews,
                        revision: Life {
                            owner: name.clone(),
                            kind: "revision",
                            alive: revision,
                            trace: trace.clone(),
                        },
                        code,
                    },
                )
                .unwrap();
            trace
                .roots
                .lock()
                .unwrap()
                .insert(
                    name.clone(),
                    phenix_graph::GraphOrigin::from_root(snapshot.revision_identity().clone()),
                );
            snapshots.push(snapshot);
            let key = Key::new(&name, "main");
            options.inspect_network.insert(key.clone());
            let name_path = path(group, step);
            let policy = options.composition.entry(key).or_default();
            policy.priority = [0, 0, 5, 10, 3, 0][step];
            policy.grants = BTreeSet::from([
                Footprint::Component(name_path.clone()),
                Footprint::InputExport(format!("in_{name_path}")),
                Footprint::OutputExport(format!("out_{name_path}")),
            ]);
            if step == 5 && previews {
                options
                    .imports
                    .selections
                    .insert(Key::new(&name, "self"), Key::new(&name, "offer"));
            }
        }
    }
    snapshots.reverse();
    (snapshots, options, trace)
}

pub fn run(groups: usize, previews: bool, configure: fn(&mut BuildOptions)) {
    let (snapshots, mut options, trace) = fixture(groups, previews);
    configure(&mut options);
    let mut kernel = Kernel::build_with_options(
        "recovery",
        snapshots,
        options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
    )
    .unwrap();
    let prepared: Vec<_> = (0..groups)
        .flat_map(|g| (0..6).map(move |s| owner(g, s)))
        .collect();
    assert_eq!(trace.owners("prepare"), prepared);
    let expected = selected(groups);
    trace.network(kernel.compiled(), &expected);
    let winners: BTreeSet<_> = expected.values().cloned().collect();
    let losers: BTreeSet<_> = prepared
        .iter()
        .filter(|o| !winners.contains(*o))
        .cloned()
        .collect();
    assert_eq!(
        trace
            .owners("artifact")
            .into_iter()
            .collect::<BTreeSet<_>>(),
        losers
    );
    assert_eq!(
        trace
            .owners("executable")
            .into_iter()
            .collect::<BTreeSet<_>>(),
        losers
    );
    for name in &prepared {
        let artifact = kernel.export::<Mark>(&Key::new(name, "main"));
        assert_eq!(artifact.is_some(), winners.contains(name));
        if let Some(artifact) = artifact {
            assert_eq!(&artifact.owner, name);
            artifact.touch();
        }
    }
    assert!(trace.owners("activate").is_empty());
    kernel.start().unwrap();
    assert_eq!(
        trace
            .owners("activate")
            .into_iter()
            .collect::<BTreeSet<_>>(),
        winners
    );
    for path in expected.keys() {
        kernel
            .inject(&format!("in_{path}"), Value::local(1_i64))
            .unwrap();
    }
    kernel.run_steps(10_000).unwrap();
    let check_outputs = |session: &mut phenix_graph::NetworkSession| {
        for g in 0..groups {
            for s in [0, 3, 4, 5] {
                assert_eq!(
                    session
                        .take_output(&format!("out_{}", path(g, s)))
                        .unwrap()
                        .unwrap()
                        .value()
                        .borrow::<i64>(),
                    Some(&(s as i64 + 2))
                );
            }
        }
    };
    kernel.with_session(check_outputs).unwrap();
    let detached = kernel.compiled().clone();
    drop(kernel);
    assert_eq!(
        trace.owners("code").into_iter().collect::<BTreeSet<_>>(),
        losers
    );
    assert_eq!(
        trace
            .owners("revision")
            .into_iter()
            .collect::<BTreeSet<_>>(),
        losers
    );
    trace.network(&detached, &expected);
    let factory = detached.processes()[0].factory().clone();
    let mut session = detached.start();
    drop(detached);
    for path in expected.keys() {
        session
            .inject(&format!("in_{path}"), Value::local(1_i64))
            .unwrap();
    }
    session.run_until_idle(10_000);
    check_outputs(&mut session);
    drop(session);
    let process = factory.spawn();
    drop(factory);
    assert_eq!(
        trace.owners("code").into_iter().collect::<BTreeSet<_>>(),
        losers
    );
    drop(process);
    let mut effects = trace.owners("effect");
    effects.sort();
    assert_eq!(
        effects,
        winners
            .iter()
            .flat_map(|o| [o.clone(), o.clone()])
            .collect::<Vec<_>>()
    );
    assert!(trace.owners("static_effect").is_empty());
    trace.assert_released(groups, &prepared);
}
