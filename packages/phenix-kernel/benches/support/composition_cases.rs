//! Paired public-API Kernel fixtures. Includes real preparation and self-imports.
use phenix_graph::{
    ComponentDef, Contract, Value,
    compose::{Edit, NetworkPatch},
    network::PortPath,
    process::LocalProcessDispatcher,
};
use phenix_kernel::host::KernelHost;
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, ContractDefinition, Contribution, InputEvent,
    Kernel, Key, Producer,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, PluginSnapshot, Registrar};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub struct Counts {
    pub prepared: AtomicUsize,
    pub dropped: AtomicUsize,
    pub activated: AtomicUsize,
    pub factory_dropped: AtomicUsize,
}
struct Artifact(Arc<Counts>);
impl Drop for Artifact {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
struct FactoryArtifact(Arc<Counts>);
impl Drop for FactoryArtifact {
    fn drop(&mut self) {
        self.0.factory_dropped.fetch_add(1, Ordering::SeqCst);
    }
}
struct Idle;
impl Producer for Idle {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        Ok(None)
    }
}

pub struct Plugin {
    pub n: usize,
    pub previews: bool,
    pub dense: bool,
    pub revision: i64,
    pub counts: Arc<Counts>,
}
fn contract() -> Contract {
    Contract::new(
        "q3/identity",
        ComponentDef::map(|n: i64| n).interface().clone(),
    )
    .unwrap()
}
fn port(process: &str, port: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: port.into(),
    }
}
fn patch(i: usize, dense: bool, increment: i64) -> NetworkPatch {
    let path = if dense {
        "worker".into()
    } else {
        format!("unit{i:04}")
    };
    let input = if dense {
        "in".into()
    } else {
        format!("in{i:04}")
    };
    let output = if dense {
        "out".into()
    } else {
        format!("out{i:04}")
    };
    NetworkPatch::new([
        Edit::Add {
            path: path.clone(),
            definition: Arc::new(
                ComponentDef::map(move |n: i64| n + increment)
                    .claim(&contract())
                    .unwrap(),
            ),
        },
        Edit::ExposeInput {
            name: input,
            targets: vec![port(&path, "input")],
            capacity: NonZeroUsize::new(1).unwrap(),
        },
        Edit::ExposeOutput {
            name: output,
            from: port(&path, "output"),
        },
    ])
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(ContractDefinition {
            id: "identity".into(),
            contract: contract(),
        })?;
        for i in 0..self.n {
            let id = format!("c{i:04}");
            let mut contribution = Contribution::new(&id, patch(i, self.dense, self.revision));
            if i != 0 {
                contribution = contribution.after([Key::new("q3", format!("c{:04}", i - 1))]);
            }
            // Dense conflicts must remain independent to exercise priority, not
            // become dependency-ordered duplicate writes.
            if self.dense {
                contribution = Contribution::new(&id, patch(i, true, self.revision));
            }
            if self.previews {
                contribution = contribution
                    .offer(
                        format!("offer{i:04}"),
                        format!("unit{i:04}"),
                        Key::new("q3", "identity"),
                    )
                    .import(format!("self{i:04}"), Key::new("q3", "identity"));
                // Select this exact self offer through the host policy.
            }
            let previews = self.previews;
            let dense = self.dense;
            let revision = self.revision;
            let counts = self.counts.clone();
            contribution
                .prepare(move |context| {
                    counts.prepared.fetch_add(1, Ordering::SeqCst);
                    let expected = if dense {
                        usize::from(i != 0)
                    } else {
                        i + usize::from(previews)
                    };
                    assert_eq!(context.inspect_network()?.processes().len(), expected);
                    if previews {
                        assert_eq!(
                            context.bind_import(&format!("self{i:04}"))?.path(),
                            format!("unit{i:04}")
                        );
                    }
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.export(Artifact(counts.clone()))?;
                    let factory = FactoryArtifact(counts.clone());
                    let activated = counts.clone();
                    artifacts.activate(move || {
                        let _ = &factory;
                        activated.activated.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::new(Idle))
                    })?;
                    // The final payload is intentionally not the preview payload.
                    artifacts.patch = Some(patch(i, dense, revision + 1));
                    Ok(artifacts)
                })
                .register(registrar)?;
        }
        Ok(())
    }
}

pub fn options(n: usize, previews: bool, dense: bool) -> BuildOptions {
    let mut options = BuildOptions::default();
    for i in 0..n {
        let key = Key::new("q3", format!("c{i:04}"));
        options.inspect_network.insert(key.clone());
        options.composition.entry(key).or_default().grants = patch(i, dense, 0).footprints();
        options
            .composition
            .get_mut(&Key::new("q3", format!("c{i:04}")))
            .unwrap()
            .priority = i as i64;
        if previews {
            options.imports.selections.insert(
                Key::new("q3", format!("self{i:04}")),
                Key::new("q3", format!("offer{i:04}")),
            );
        }
    }
    options
}

pub fn snapshot(plugin: Plugin) -> PluginSnapshot {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("q3", |plugin: Plugin| Ok(plugin)).unwrap();
    catalog.instance().load(&key, plugin).unwrap()
}

pub fn build(n: usize, previews: bool, dense: bool, configure: fn(&mut BuildOptions)) {
    let counts = Arc::new(Counts::default());
    let snapshots = [snapshot(Plugin {
        n,
        previews,
        dense,
        revision: 1,
        counts: counts.clone(),
    })];
    let mut options = options(n, previews, dense);
    configure(&mut options);
    let mut kernel =
        Kernel::build_with_options("q3", snapshots, options.with_dispatcher(Arc::new(LocalProcessDispatcher)))
            .unwrap();
    assert_eq!(counts.activated.load(Ordering::SeqCst), 0);
    kernel.start().unwrap();
    assert_eq!(
        counts.activated.load(Ordering::SeqCst),
        if dense { 1 } else { n }
    );
    assert_eq!(counts.prepared.load(Ordering::SeqCst), n);
    assert_eq!(
        kernel.compiled().processes().len(),
        if dense { 1 } else { n }
    );
    let input = if dense {
        "in".into()
    } else {
        format!("in{:04}", n - 1)
    };
    let output = if dense {
        "out".into()
    } else {
        format!("out{:04}", n - 1)
    };
    kernel.inject(&input, Value::local(1_i64)).unwrap();
    kernel.run_steps(10_000).unwrap();
    kernel
        .with_session(|session| {
            assert_eq!(
                session
                    .take_output(&output)
                    .unwrap()
                    .unwrap()
                    .value()
                    .borrow::<i64>(),
                Some(&3)
            );
        })
        .unwrap();
    drop(kernel);
    assert_eq!(counts.dropped.load(Ordering::SeqCst), n);
    assert_eq!(counts.factory_dropped.load(Ordering::SeqCst), n);
}

pub fn rebuild(n: usize, previews: bool, configure: fn(&mut BuildOptions)) {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("q3", |plugin: Plugin| Ok(plugin)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let counts = Arc::new(Counts::default());
    for revision in 1..=3 {
        let plugin = Plugin {
            n,
            previews,
            dense: false,
            revision,
            counts: counts.clone(),
        };
        let stage = if revision == 1 {
            host.plugins().prepare_load(&key, plugin)
        } else {
            host.plugins().prepare_reload(&key, plugin)
        }
        .unwrap();
        let mut options = options(n, previews, false);
        configure(&mut options);
        let candidate = host
            .prepare(
                vec![stage],
                options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "q3-rebuild",
                |_| Ok(()),
            )
            .unwrap();
        let generation = host.publish(candidate).unwrap();
        assert_eq!(generation.epoch(), revision as u64);
        generation.inject("in0000", Value::local(1_i64)).unwrap();
        generation.run_steps(10_000).unwrap();
        generation
            .with_session(|session| {
                assert_eq!(
                    session
                        .take_output("out0000")
                        .unwrap()
                        .unwrap()
                        .value()
                        .borrow::<i64>(),
                    Some(&(revision + 2))
                )
            })
            .unwrap();
    }
    drop(host);
    assert_eq!(counts.prepared.load(Ordering::SeqCst), 3 * n);
    assert_eq!(counts.dropped.load(Ordering::SeqCst), 3 * n);
    assert_eq!(counts.factory_dropped.load(Ordering::SeqCst), 3 * n);
    assert_eq!(counts.activated.load(Ordering::SeqCst), 3 * n);
}
