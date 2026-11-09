use phenix_graph::{CompileLimits, GraphBuilder, LocalExecutor, Value};
use phenix_kernel::host::{Candidate, Generation, KernelHost, LifecycleError};
use phenix_kernel::{Contribution, ImportPolicy, Key, Notice, Producer};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use std::sync::{Arc, Barrier, Mutex, mpsc};

type Log = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
struct Plugin {
    name: String,
    revision: usize,
    log: Log,
    requires: Vec<Key>,
    compile_fail: bool,
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
    fn poll(&mut self) -> Result<Option<Notice>, String> {
        self.1.lock().unwrap().push(format!("poll:{}", self.0));
        Ok(Some(Notice {
            trigger: self.0.clone(),
            values: vec![Value::local(1_i64)],
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
        let start_fail = self.start_fail;
        let start_log = log.clone();
        let start_name = name.clone();
        let gate = self.gate.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: self.requires.clone(),
            routes: vec![name.clone()],
            compose: Box::new(move |context| {
                log.lock()
                    .unwrap()
                    .push(format!("compose:{name}:{revision}"));
                let graph = &mut *context.graph;
                if compile_fail {
                    *graph = GraphBuilder::new("invalid").limits(CompileLimits {
                        max_work: 0,
                        ..Default::default()
                    });
                }
                let action_log = log.clone();
                let action_name = name.clone();
                let action_gate = gate.clone();
                let node = graph
                    .node(&name, move |x: i64| {
                        if let Some(gate) = &action_gate {
                            gate.0.wait();
                            gate.1.wait();
                        }
                        action_log
                            .lock()
                            .unwrap()
                            .push(format!("fire:{action_name}:{revision}"));
                        x + revision as i64
                    })
                    .map_err(|error| error.to_string())?;
                let input = graph.expose_input(format!("input_{name}"), node.inlet())?;
                graph.trigger(&name, [input])?;
                let start_log = start_log.clone();
                let start_name = start_name.clone();
                context.activate(move || {
                    start_log
                        .lock()
                        .unwrap()
                        .push(format!("start:{start_name}:{revision}"));
                    if start_fail {
                        return Err("activation failed".into());
                    }
                    Ok(Box::new(Source(start_name.clone(), start_log.clone())))
                })
            }),
        })
    }
}

fn prepare(host: &KernelHost, stages: Vec<StagedPlugin>) -> Result<Candidate, LifecycleError> {
    host.prepare(
        stages,
        &ImportPolicy::default(),
        Arc::new(LocalExecutor),
        "generation",
    )
}

fn fire(generation: &Generation, name: &str) {
    assert!(
        generation
            .fire(name, vec![Value::local(5_i64)])
            .unwrap()
            .success()
    );
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
    assert_eq!(old.drain(2).len(), 2);
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
        assert!(previous.drain(100).is_empty());
        assert_eq!(current.drain(1).len(), 1);
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
    assert!(empty.fire("alpha", vec![Value::local(1_i64)]).is_err());
    assert!(latest.drain(100).is_empty());
    fire(&old, "beta");
    drop(old);
    assert!(log.lock().unwrap().contains(&"plugin:alpha:1".into()));
    drop(host);
    assert!(empty.drain(100).is_empty());
}

#[test]
fn resolution_compile_binding_and_nth_start_failure_leave_active_unchanged() {
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
    for mode in 0..4 {
        log.lock().unwrap().clear();
        let mut replacement = Plugin::new("alpha", 2, &log);
        replacement.requires = if mode == 0 {
            vec![Key::new("missing", "main")]
        } else {
            vec![]
        };
        replacement.compile_fail = mode == 1;
        let first = host.plugins().prepare_reload(&a, replacement).unwrap();
        let mut second = Plugin::new("beta", 2, &log);
        second.start_fail = mode == 3;
        let stages = vec![first, host.plugins().prepare_load(&b, second).unwrap()];
        let result = if mode == 2 {
            host.prepare_with(
                stages,
                &ImportPolicy::default(),
                Arc::new(LocalExecutor),
                "binding",
                |_| Err("store refused".into()),
            )
        } else {
            prepare(&host, stages)
        };
        match (mode, result) {
            (0 | 1, Err(LifecycleError::Build(_)))
            | (2, Err(LifecycleError::Binding(_)))
            | (3, Err(LifecycleError::Start(_))) => {}
            _ => panic!("unexpected preparation outcome for {mode}"),
        }
        let events = log.lock().unwrap().clone();
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
        assert_eq!(old.drain(1).len(), 1);
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
    assert!(current.drain(100).is_empty());
    fire(&current, "alpha");
    drop(host);
    assert!(refreshed.drain(100).is_empty());
    fire(&refreshed, "alpha");
}

#[test]
fn publication_waits_for_old_in_flight_wave_without_invalidating_its_handle() {
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
    assert!(old.with_engine(|engine| engine.trigger_targets("alpha").is_some()));
    let candidate = prepare(&host, vec![host.plugins().prepare_unload(&a).unwrap()]).unwrap();
    let reader = old.clone();
    let wave = std::thread::spawn(move || fire(&reader, "alpha"));
    gate.0.wait();
    let (publishing, started) = mpsc::channel();
    let publisher = std::thread::spawn(move || {
        publishing.send(()).unwrap();
        let current = host.publish(candidate).unwrap();
        (host, current)
    });
    started.recv().unwrap();
    gate.1.wait();
    wave.join().unwrap();
    let (host, current) = publisher.join().unwrap();
    assert!(Arc::ptr_eq(&current, &host.active().unwrap()));
    assert!(old.drain(100).is_empty());
    assert!(old.with_engine(|engine| engine.trigger_targets("alpha").is_some()));
    assert!(current.with_engine(|engine| engine.trigger_targets("alpha").is_none()));
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
