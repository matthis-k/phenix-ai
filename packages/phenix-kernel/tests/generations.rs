mod support;
use phenix_graph::{
    Value,
    compose::{Edit, Footprint, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::host::{Candidate, Generation, KernelHost, LifecycleError};
use phenix_kernel::{BuildOptions, CandidateArtifacts, Contribution, InputEvent, Key, Producer};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use std::sync::{Arc, Barrier, Mutex, mpsc};

type Log = Arc<Mutex<Vec<String>>>;

struct Artifact(String, Log);
impl Drop for Artifact {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(self.0.clone());
    }
}

#[derive(Clone)]
struct Plugin {
    name: String,
    revision: usize,
    log: Log,
    requires: Vec<Key>,
    compile_fail: bool,
    compose_fail: bool,
    start_fail: bool,
    gate: Option<Arc<(Barrier, Barrier)>>,
}

impl Plugin {
    fn new(name: &str, revision: usize, log: &Log) -> Self {
        Self {
            name: name.into(),
            revision,
            log: log.clone(),
            requires: vec![],
            compile_fail: false,
            compose_fail: false,
            start_fail: false,
            gate: None,
        }
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push(format!("plugin:{}:{}", self.name, self.revision));
    }
}

struct Source(String, Log);
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, phenix_kernel::CallbackError> {
        self.1.lock().unwrap().push(format!("poll:{}", self.0));
        Ok(Some(InputEvent {
            input: self.0.clone(),
            value: Value::local(1_i64),
        }))
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(format!("stop:{}", self.0));
    }
}

impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let name = self.name.clone();
        let revision = self.revision;
        let log = self.log.clone();
        let compile_fail = self.compile_fail;
        let compose_fail = self.compose_fail;
        let start_fail = self.start_fail;
        let start_log = log.clone();
        let start_name = name.clone();
        let gate = self.gate.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: self.requires.clone(),
            routes: vec![name.clone()],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                let mut context = CandidateArtifacts::default();
                log.lock()
                    .unwrap()
                    .push(format!("compose:{name}:{revision}"));
                context.export(Artifact(format!("export:{name}:{revision}"), log.clone()))?;
                let action_guard = Artifact(format!("action:{name}:{revision}"), log.clone());
                let factory_guard = Artifact(format!("factory:{name}:{revision}"), log.clone());
                let action_log = log.clone();
                let action_name = name.clone();
                let action_gate = gate.clone();
                let node = support::sink(move |_: i64| {
                    let _ = &action_guard;
                    if let Some(gate) = &action_gate {
                        gate.0.wait();
                        gate.1.wait();
                    }
                    action_log
                        .lock()
                        .unwrap()
                        .push(format!("fire:{action_name}:{revision}"));
                });
                context.patch = Some(NetworkPatch::new([
                    Edit::Add {
                        path: name.clone(),
                        definition: node,
                    },
                    Edit::ExposeInput {
                        name: name.clone(),
                        targets: vec![support::path(
                            if compile_fail { "missing" } else { &name },
                            "run",
                        )],
                        capacity: support::capacity(),
                    },
                ]));
                let start_log = start_log.clone();
                let start_name = start_name.clone();
                context.activate(move || {
                    let _ = &factory_guard;
                    start_log
                        .lock()
                        .unwrap()
                        .push(format!("start:{start_name}:{revision}"));
                    if start_fail {
                        return Err("activation failed".into());
                    }
                    Ok(Box::new(Source(start_name.clone(), start_log.clone())))
                })?;
                if compose_fail {
                    return Err("composition failed".into());
                }
                Ok(context)
            })),
        })
    }
}

fn prepare(host: &KernelHost, stages: Vec<StagedPlugin>) -> Result<Candidate, LifecycleError> {
    host.prepare(
        stages,
        options(),
        Arc::new(LocalProcessDispatcher),
        "generation",
        |_| Ok(()),
    )
}

fn fire(generation: &Generation, name: &str) {
    generation.run_steps(100).unwrap();
    generation.inject(name, Value::local(5_i64)).unwrap();
    generation.run_steps(100).unwrap();
}

fn options() -> BuildOptions {
    let mut options = BuildOptions::default();
    for name in ["alpha", "beta"] {
        support::grant(
            &mut options,
            Key::new(name, "main"),
            [
                Footprint::Component(name.into()),
                Footprint::InputExport(name.into()),
            ],
        );
    }
    options
}

#[test]
fn publication_pins_old_engines_but_stops_old_producers_and_batches_unload() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Generation>();
    send_sync::<KernelHost>();
    let log = Log::default();
    let mut catalog = PluginCatalog::new();
    let a = catalog.register("alpha", |p: Plugin| Ok(p)).unwrap();
    let b = catalog.register("beta", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    assert!(host.active().is_none());
    let candidate = prepare(
        &host,
        vec![
            host.plugins()
                .prepare_load(&a, Plugin::new("alpha", 1, &log))
                .unwrap(),
            host.plugins()
                .prepare_load(&b, Plugin::new("beta", 1, &log))
                .unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(host.plugins().snapshots().count(), 0);
    assert!(
        !log.lock()
            .unwrap()
            .iter()
            .any(|s| s.starts_with("poll:") || s.starts_with("fire:"))
    );
    let old = host.publish(candidate).unwrap();
    assert_eq!(old.epoch(), 1);
    assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
    assert_eq!(old.drain(2).unwrap().len(), 2);
    fire(&old, "alpha");

    for revision in 2..=4 {
        let candidate = prepare(
            &host,
            vec![
                host.plugins()
                    .prepare_reload(&a, Plugin::new("alpha", revision, &log))
                    .unwrap(),
            ],
        )
        .unwrap();
        let previous = host.active().unwrap();
        let current = host.publish(candidate).unwrap();
        assert_eq!(current.epoch(), revision as u64);
        assert!(previous.drain(100).unwrap().is_empty());
        assert_eq!(current.drain(1).unwrap().len(), 1);
        fire(&current, "alpha");
        assert_eq!(
            host.plugins().snapshot(&a).unwrap().revision(),
            revision as u64
        );
    }
    fire(&old, "alpha");
    assert!(!log.lock().unwrap().contains(&"plugin:alpha:1".into()));
    let latest = host.active().unwrap();
    let candidate = prepare(
        &host,
        vec![
            host.plugins().prepare_unload(&a).unwrap(),
            host.plugins().prepare_unload(&b).unwrap(),
        ],
    )
    .unwrap();
    // The exact candidate has neither route; the old active generation still does.
    fire(&latest, "beta");
    let empty = host.publish(candidate).unwrap();
    assert_eq!(host.plugins().snapshots().count(), 0);
    assert!(empty.inject("alpha", Value::local(1_i64)).is_err());
    assert!(latest.drain(100).unwrap().is_empty());
    fire(&old, "beta");
    drop(old);
    assert!(log.lock().unwrap().contains(&"plugin:alpha:1".into()));
    drop(host);
    assert!(empty.drain(100).unwrap().is_empty());
}

#[test]
fn resolution_compile_composition_binding_and_nth_start_failure_leave_active_unchanged() {
    let log = Log::default();
    let mut catalog = PluginCatalog::new();
    let a = catalog.register("alpha", |p: Plugin| Ok(p)).unwrap();
    let b = catalog.register("beta", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let initial = prepare(
        &host,
        vec![
            host.plugins()
                .prepare_load(&a, Plugin::new("alpha", 1, &log))
                .unwrap(),
        ],
    )
    .unwrap();
    let old = host.publish(initial).unwrap();
    for mode in 0..5 {
        log.lock().unwrap().clear();
        let mut replacement = Plugin::new("alpha", 2, &log);
        replacement.requires = if mode == 0 {
            vec![Key::new("missing", "main")]
        } else {
            vec![]
        };
        replacement.compile_fail = mode == 1;
        replacement.compose_fail = mode == 4;
        let first = host.plugins().prepare_reload(&a, replacement).unwrap();
        let mut second = Plugin::new("beta", 2, &log);
        second.start_fail = mode == 3;
        let stages = vec![first, host.plugins().prepare_load(&b, second).unwrap()];
        let result = if mode == 2 {
            host.prepare(
                stages,
                options(),
                Arc::new(LocalProcessDispatcher),
                "binding",
                |_| Err("store refused".into()),
            )
        } else {
            prepare(&host, stages)
        };
        match (mode, result) {
            (0 | 1 | 4, Err(LifecycleError::Build(_))) | (2, Err(LifecycleError::Binding(_))) => {}
            (3, Err(error @ LifecycleError::Start(_))) => {
                let start = std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<phenix_kernel::StartError>()
                    .unwrap();
                assert!(
                    matches!(start, phenix_kernel::StartError::Activation { contribution, .. } if *contribution == Key::new("beta", "main"))
                );
                assert!(std::error::Error::source(start).is_some());
            }
            _ => panic!("unexpected preparation outcome for {mode}"),
        }
        let events = log.lock().unwrap().clone();
        for name in ["alpha", "beta"] {
            let plugin = format!("plugin:{name}:2");
            let plugin_index = events.iter().position(|event| event == &plugin).unwrap();
            for kind in ["export", "action", "factory"] {
                let artifact = format!("{kind}:{name}:2");
                let positions: Vec<_> = events
                    .iter()
                    .enumerate()
                    .filter_map(|(index, event)| (event == &artifact).then_some(index))
                    .collect();
                if mode == 0 || (matches!(mode, 1 | 4) && name == "beta") {
                    assert!(positions.is_empty(), "{events:?}");
                } else {
                    assert_eq!(positions.len(), 1, "{events:?}");
                    assert!(positions[0] < plugin_index, "{events:?}");
                }
            }
        }
        assert!(
            !events
                .iter()
                .any(|event| event.starts_with("poll:") || event.starts_with("fire:"))
        );
        if mode == 3 {
            assert!(events.contains(&"start:alpha:2".into()));
            assert!(events.contains(&"start:beta:2".into()));
            assert!(events.contains(&"stop:alpha".into()));
        } else {
            assert!(!events.iter().any(|event| event.starts_with("start:")));
        }
        assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
        assert_eq!(host.plugins().snapshot(&a).unwrap().revision(), 1);
        assert!(host.plugins().snapshot(&b).is_err());
        fire(&old, "alpha");
        assert_eq!(old.drain(1).unwrap().len(), 1);
    }
}

#[test]
fn interleaved_candidates_foreign_hosts_and_empty_batch_epochs() {
    let log = Log::default();
    let mut catalog = PluginCatalog::new();
    let a = catalog.register("alpha", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let mut sibling = KernelHost::new(catalog.instance());
    let foreign_stage = sibling
        .plugins()
        .prepare_load(&a, Plugin::new("alpha", 9, &log))
        .unwrap();
    assert!(matches!(
        prepare(&host, vec![foreign_stage]),
        Err(LifecycleError::Plugins(_))
    ));
    let foreign = prepare(&host, vec![]).unwrap();
    assert!(matches!(
        sibling.publish(foreign),
        Err(LifecycleError::StaleCandidate)
    ));
    assert!(sibling.active().is_none());
    let abandoned = prepare(
        &host,
        vec![
            host.plugins()
                .prepare_load(&a, Plugin::new("alpha", 8, &log))
                .unwrap(),
        ],
    )
    .unwrap();
    drop(abandoned);
    assert!(host.active().is_none());
    assert!(log.lock().unwrap().contains(&"plugin:alpha:8".into()));
    let older = prepare(
        &host,
        vec![
            host.plugins()
                .prepare_load(&a, Plugin::new("alpha", 1, &log))
                .unwrap(),
        ],
    )
    .unwrap();
    let newer = prepare(
        &host,
        vec![
            host.plugins()
                .prepare_load(&a, Plugin::new("alpha", 2, &log))
                .unwrap(),
        ],
    )
    .unwrap();
    let current = host.publish(newer).unwrap();
    assert!(matches!(
        host.publish(older),
        Err(LifecycleError::StaleCandidate)
    ));
    assert!(Arc::ptr_eq(&current, &host.active().unwrap()));
    let stale = prepare(&host, vec![]).unwrap();
    let refresh = prepare(&host, vec![]).unwrap();
    let refreshed = host.publish(refresh).unwrap();
    assert_eq!(refreshed.epoch(), 2);
    assert!(matches!(
        host.publish(stale),
        Err(LifecycleError::StaleCandidate)
    ));
    assert!(current.drain(100).unwrap().is_empty());
    fire(&current, "alpha");
    drop(host);
    assert!(refreshed.drain(100).unwrap().is_empty());
    fire(&refreshed, "alpha");
}

#[test]
fn publication_does_not_wait_for_old_explicit_wave_or_callback() {
    let log = Log::default();
    let mut catalog = PluginCatalog::new();
    let a = catalog.register("alpha", |p: Plugin| Ok(p)).unwrap();
    let gate = Arc::new((Barrier::new(2), Barrier::new(2)));
    let mut plugin = Plugin::new("alpha", 1, &log);
    plugin.gate = Some(gate.clone());
    let mut host = KernelHost::new(catalog.instance());
    let candidate = prepare(
        &host,
        vec![host.plugins().prepare_load(&a, plugin).unwrap()],
    )
    .unwrap();
    let old = host.publish(candidate).unwrap();
    assert!(old.compiled().inputs().contains_key("alpha"));
    let candidate = prepare(&host, vec![host.plugins().prepare_unload(&a).unwrap()]).unwrap();
    let reader = old.clone();
    let wave = std::thread::spawn(move || fire(&reader, "alpha"));
    gate.0.wait();
    let (entered, entry) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let reader = old.clone();
    let callback = std::thread::spawn(move || {
        reader.with_kernel(|kernel| {
            entered.send(()).unwrap();
            released.recv().unwrap();
            assert!(kernel.compiled().inputs().contains_key("alpha"));
        })
    });
    // The callback must enter even while the explicit wave is blocked.
    if entry
        .recv_timeout(std::time::Duration::from_secs(2))
        .is_err()
    {
        gate.1.wait();
        release.send(()).unwrap();
        wave.join().unwrap();
        callback.join().unwrap();
        panic!("engine callback blocked behind explicit wave");
    }
    let (publishing, started) = mpsc::channel();
    let publisher = std::thread::spawn(move || {
        let current = host.publish(candidate).unwrap();
        publishing.send(()).unwrap();
        (host, current)
    });
    let published = started.recv_timeout(std::time::Duration::from_secs(2));
    gate.1.wait();
    release.send(()).unwrap();
    wave.join().unwrap();
    callback.join().unwrap();
    published.expect("explicit operations must not block publication");
    let (host, current) = publisher.join().unwrap();
    assert!(Arc::ptr_eq(&current, &host.active().unwrap()));
    assert!(old.drain(100).unwrap().is_empty());
    assert!(old.compiled().inputs().contains_key("alpha"));
    assert!(old.with_kernel(|kernel| kernel.compiled().inputs().contains_key("alpha")));
    assert!(!current.compiled().inputs().contains_key("alpha"));
}

#[test]
fn replacement_without_readers_drops_producers_before_plugin_leases() {
    let log = Log::default();
    let mut catalog = PluginCatalog::new();
    let a = catalog.register("alpha", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let candidate = prepare(
        &host,
        vec![
            host.plugins()
                .prepare_load(&a, Plugin::new("alpha", 1, &log))
                .unwrap(),
        ],
    )
    .unwrap();
    drop(host.publish(candidate).unwrap());
    let candidate = prepare(&host, vec![host.plugins().prepare_unload(&a).unwrap()]).unwrap();
    host.publish(candidate).unwrap();
    let events = log.lock().unwrap();
    let stopped = events
        .iter()
        .position(|event| event == "stop:alpha")
        .unwrap();
    let unloaded = events
        .iter()
        .position(|event| event == "plugin:alpha:1")
        .unwrap();
    assert!(stopped < unloaded);
}
