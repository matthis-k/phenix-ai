mod support;
use phenix_graph::{
    Value,
    compose::{Edit, Footprint, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::host::{Candidate, Generation, KernelHost, LifecycleError};
use phenix_kernel::{BuildOptions, CandidateArtifacts, Contribution, InputEvent, Key, Producer};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Log = Arc<Mutex<Vec<String>>>;

struct Guard(&'static str, usize, Log);

impl Guard {
    fn touch(&self) {
        assert!(!self.0.is_empty());
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.2
            .lock()
            .unwrap()
            .push(format!("{}:{}", self.0, self.1));
    }
}

struct Plugin {
    id: usize,
    log: Log,
    polls: Arc<AtomicUsize>,
    gate: Option<Arc<(Barrier, Barrier)>>,
}

impl Drop for Plugin {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(format!("plugin:{}", self.id));
    }
}

struct Source {
    polls: Arc<AtomicUsize>,
    gate: Option<Arc<(Barrier, Barrier)>>,
    _guard: Guard,
}

impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, phenix_kernel::CallbackError> {
        let tick = self.polls.fetch_add(1, Ordering::SeqCst) + 1;
        if tick == 1
            && let Some(gate) = &self.gate
        {
            gate.0.wait();
            gate.1.wait();
        }
        Ok(Some(InputEvent {
            input: "tick".into(),
            value: Value::local(tick as i64),
        }))
    }
}

impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let id = self.id;
        let log = self.log.clone();
        let polls = self.polls.clone();
        let gate = self.gate.clone();
        into.add(Contribution {
            id: "source".into(),
            requires: vec![],
            routes: vec!["tick".into()],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                let mut context = CandidateArtifacts::default();
                context.export(Guard("export", id, log.clone()))?;
                let action = Guard("action", id, log.clone());
                let action_log = log.clone();
                let node = support::sink(move |tick: i64| {
                    action.touch();
                    action_log.lock().unwrap().push(format!("tick:{id}:{tick}"));
                });
                context.patch = Some(NetworkPatch::new([
                    Edit::Add {
                        path: "clock".into(),
                        definition: node,
                    },
                    Edit::ExposeInput {
                        name: "tick".into(),
                        targets: vec![support::path("clock", "run")],
                        capacity: std::num::NonZeroUsize::new(3).unwrap(),
                    },
                ]));
                let factory = Guard("factory", id, log.clone());
                let log = log.clone();
                let polls = polls.clone();
                let gate = gate.clone();
                context.activate(move || {
                    factory.touch();
                    log.lock().unwrap().push(format!("start:{id}"));
                    Ok(Box::new(Source {
                        polls: polls.clone(),
                        gate: gate.clone(),
                        _guard: Guard("producer", id, log.clone()),
                    }))
                })?;
                Ok(context)
            })),
        })
    }
}

fn prepare(host: &KernelHost, stages: Vec<StagedPlugin>) -> Candidate {
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("clock", "source"),
        [
            Footprint::Component("clock".into()),
            Footprint::InputExport("tick".into()),
        ],
    );
    host.prepare(
        stages,
        options,
        Arc::new(LocalProcessDispatcher),
        "retirement",
        |_| Ok(()),
    )
    .unwrap()
}

fn assert_live(generation: &Generation) {
    let events = generation.drain(1);
    assert_eq!(events.len(), 1);
    assert!(events[0].result.is_ok());
}

#[test]
fn publication_retires_a_blocked_poll_and_rejects_its_late_event() {
    let log = Log::default();
    let polls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new((Barrier::new(2), Barrier::new(2)));
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("clock", |plugin: Plugin| Ok(plugin))
        .unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Plugin {
                id: 1,
                log: log.clone(),
                polls: polls.clone(),
                gate: Some(gate.clone()),
            },
        )
        .unwrap();
    let candidate = prepare(&host, vec![stage]);
    let old = host.publish(candidate).unwrap();
    let candidate = prepare(&host, vec![host.plugins().prepare_unload(&key).unwrap()]);
    let reader = old.clone();
    let drain = std::thread::spawn(move || reader.drain(3));
    gate.0.wait();
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    let publishing = Arc::new(Barrier::new(2));
    let publisher_gate = publishing.clone();
    let (published, publication_complete) = std::sync::mpsc::channel();
    let polls_during_publish = polls.clone();
    let publisher = std::thread::spawn(move || {
        publisher_gate.wait();
        let current = host.publish(candidate).unwrap();
        assert_eq!(polls_during_publish.load(Ordering::SeqCst), 1);
        published.send(()).unwrap();
        (host, current, polls_during_publish)
    });
    publishing.wait();
    publication_complete
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("publication waited for plugin poll");
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    gate.1.wait();
    let events = drain.join().unwrap();
    assert!(events.is_empty());
    let (host, current, polls) = publisher.join().unwrap();
    assert!(Arc::ptr_eq(&current, &host.active().unwrap()));
    assert_eq!(current.epoch(), 2);
    for _ in 0..3 {
        assert!(old.drain(10).is_empty());
    }
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    // Retirement rejects the in-flight result but does not preempt the callback.
    old.run_steps(100).unwrap();
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("tick:"))
            .cloned()
            .collect::<Vec<_>>(),
        Vec::<String>::new()
    );
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|event| *event == "producer:1")
            .count(),
        1
    );
}

#[test]
fn abandoned_and_stale_activated_candidates_drop_artifacts_before_exact_plugin() {
    for stale in [false, true] {
        let log = Log::default();
        let active_polls = Arc::new(AtomicUsize::new(0));
        let candidate_polls = Arc::new(AtomicUsize::new(0));
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("clock", |plugin: Plugin| Ok(plugin))
            .unwrap();
        let mut host = KernelHost::new(catalog.instance());
        let stage = host
            .plugins()
            .prepare_load(
                &key,
                Plugin {
                    id: 1,
                    log: log.clone(),
                    polls: active_polls.clone(),
                    gate: None,
                },
            )
            .unwrap();
        let initial = prepare(&host, vec![stage]);
        let mut active = host.publish(initial).unwrap();
        let stage = host
            .plugins()
            .prepare_reload(
                &key,
                Plugin {
                    id: 2,
                    log: log.clone(),
                    polls: candidate_polls.clone(),
                    gate: None,
                },
            )
            .unwrap();
        let snapshot = stage.candidate().unwrap();
        let snapshot_clone = snapshot.clone();
        let candidate = prepare(&host, vec![stage]);
        assert!(log.lock().unwrap().contains(&"start:2".into()));
        assert_eq!(candidate_polls.load(Ordering::SeqCst), 0);
        if stale {
            let refresh = prepare(&host, vec![]);
            active = host.publish(refresh).unwrap();
        }
        let epoch = active.epoch();
        // Do not let inspection leases mask incorrect candidate drop ordering.
        drop(snapshot);
        drop(snapshot_clone);
        assert!(!log.lock().unwrap().contains(&"plugin:2".into()));
        if stale {
            assert!(matches!(
                host.publish(candidate),
                Err(LifecycleError::StaleCandidate)
            ));
        } else {
            drop(candidate);
        }
        let events = log.lock().unwrap().clone();
        for kind in ["producer", "export", "factory", "action"] {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| **event == format!("{kind}:2"))
                    .count(),
                1,
                "{events:?}"
            );
        }
        assert_eq!(events.last().unwrap(), "plugin:2");
        assert_eq!(
            events.iter().filter(|event| *event == "plugin:2").count(),
            1
        );
        assert!(!events.contains(&"plugin:1".into()));
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
        assert_eq!(active.epoch(), epoch);
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);
        assert_live(&active);
        assert_eq!(active_polls.load(Ordering::SeqCst), 1);
        assert_eq!(candidate_polls.load(Ordering::SeqCst), 0);
    }
}
