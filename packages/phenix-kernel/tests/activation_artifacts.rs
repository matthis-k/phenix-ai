mod support;
use phenix_graph::{
    Value,
    compose::{Edit, Footprint, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, Contribution, InputEvent, Kernel, Key, Producer,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, PluginSnapshot, Registrar};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Log = Arc<Mutex<Vec<String>>>;

struct Mark(String, Log);
impl Mark {
    fn touch(&self) {
        assert!(!self.0.is_empty());
    }
}
impl Drop for Mark {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(self.0.clone());
    }
}

struct State {
    id: usize,
    attempts: AtomicUsize,
    polls: AtomicUsize,
    _mark: Mark,
}

struct Source {
    state: Arc<State>,
    _mark: Mark,
}
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, phenix_kernel::CallbackError> {
        self.state.polls.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}

struct Plugin {
    log: Log,
    composed: Arc<AtomicUsize>,
    activations: Arc<Mutex<Vec<(String, usize)>>>,
    failure: &'static str,
}
impl Drop for Plugin {
    fn drop(&mut self) {
        self.log.lock().unwrap().push("lease".into());
    }
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        for name in ["a", "b"] {
            let log = self.log.clone();
            let composed = self.composed.clone();
            let activations = self.activations.clone();
            let failure = self.failure;
            registrar.add(Contribution {
                id: name.into(),
                requires: if name == "b" {
                    vec![Key::new("plugin", "a")]
                } else {
                    vec![]
                },
                routes: if failure == "route" {
                    vec!["missing".into()]
                } else {
                    vec![]
                },
                patch: NetworkPatch::new([]),
                prepare: Some(Box::new(move |_| {
                    let mut context = CandidateArtifacts::default();
                    let id = composed.fetch_add(1, Ordering::SeqCst);
                    let state = Arc::new(State {
                        id,
                        attempts: AtomicUsize::new(0),
                        polls: AtomicUsize::new(0),
                        _mark: Mark(format!("export:{name}:{id}"), log.clone()),
                    });
                    context.export(state.clone())?;
                    let action = Mark(format!("engine:{name}:{id}"), log.clone());
                    let definition = support::function(move |value: i64| {
                        action.touch();
                        value
                    });
                    let mut edits = vec![Edit::Add {
                        path: name.into(),
                        definition,
                    }];
                    if failure == "compile" && name == "a" {
                        edits.push(Edit::AddSource("missing".into()));
                    }
                    context.patch = Some(NetworkPatch::new(edits));
                    let factory = Mark(format!("factory:{name}:{id}"), log.clone());
                    let activations = activations.clone();
                    let log = log.clone();
                    context.activate(move || {
                        factory.touch();
                        activations.lock().unwrap().push((name.into(), state.id));
                        let attempt = state.attempts.fetch_add(1, Ordering::SeqCst);
                        if failure == "activation" && name == "b" && attempt == 0 {
                            return Err("nth activation failed".into());
                        }
                        Ok(Box::new(Source {
                            state: state.clone(),
                            _mark: Mark(format!("producer:{name}:{id}"), log.clone()),
                        }))
                    })?;
                    if failure == "compose" {
                        return Err("composition failed".into());
                    }
                    Ok(context)
                })),
            })?;
        }
        Ok(())
    }
}

fn snapshots(plugin: Plugin) -> Vec<PluginSnapshot> {
    let mut catalog = PluginCatalog::new();
    let kind = catalog
        .register("plugin", |plugin: Plugin| Ok(plugin))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, plugin).unwrap();
    plugins.snapshots().collect()
}

fn build(
    name: &str,
    snapshots: Vec<PluginSnapshot>,
) -> Result<Kernel, phenix_kernel::ResolutionError> {
    let mut options = BuildOptions::default();
    for name in ["a", "b"] {
        support::grant(
            &mut options,
            Key::new("plugin", name),
            [
                Footprint::Component(name.into()),
                Footprint::Source("missing".into()),
            ],
        );
    }
    Kernel::build_with_options(name, snapshots, Arc::new(LocalProcessDispatcher), options)
}

#[test]
fn artifacts_and_engine_drop_before_plugin_leases_on_success_and_build_failure() {
    for failure in ["", "unstarted", "compose", "compile", "route"] {
        let log = Log::default();
        let result = build(
            "drop-order",
            snapshots(Plugin {
                log: log.clone(),
                composed: Arc::default(),
                activations: Arc::default(),
                failure,
            }),
        );
        if matches!(failure, "" | "unstarted") {
            let mut kernel = result.unwrap();
            if failure.is_empty() {
                kernel.start().unwrap();
            }
            assert!(log.lock().unwrap().is_empty());
            drop(kernel);
        } else {
            drop(result.err().expect(failure));
        }
        let events = log.lock().unwrap();
        assert_eq!(
            events.last().map(String::as_str),
            Some("lease"),
            "{failure}: {events:?}"
        );
        assert!(events.iter().any(|event| event.starts_with("factory:")));
        assert!(events.iter().any(|event| event.starts_with("export:")));
        assert!(events.iter().any(|event| event.starts_with("engine:")));
        if failure.is_empty() {
            let last_producer = events
                .iter()
                .rposition(|event| event.starts_with("producer:"))
                .unwrap();
            let first_artifact = events
                .iter()
                .position(|event| !event.starts_with("producer:"))
                .unwrap();
            assert!(last_producer < first_artifact, "{events:?}");
        } else {
            assert!(!events.iter().any(|event| event.starts_with("producer:")));
        }
    }
}

#[test]
fn nth_activation_failure_retries_the_exact_composed_state_without_recomposition() {
    let log = Log::default();
    let composed = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(Mutex::new(Vec::new()));
    let snapshots = snapshots(Plugin {
        log: log.clone(),
        composed: composed.clone(),
        activations: activations.clone(),
        failure: "activation",
    });
    let mut first = build("first", snapshots.clone()).unwrap();
    let mut second = build("second", snapshots).unwrap();
    let state = |kernel: &Kernel, name| {
        kernel
            .export::<Arc<State>>(&Key::new("plugin", name))
            .unwrap()
            .clone()
    };
    let a = state(&first, "a");
    let b = state(&first, "b");
    let other_a = state(&second, "a");
    let other_b = state(&second, "b");
    assert!(!Arc::ptr_eq(&a, &other_a));
    assert!(!Arc::ptr_eq(&b, &other_b));
    assert!(activations.lock().unwrap().is_empty());
    let expected = phenix_kernel::StartError::Activation {
        contribution: Key::new("plugin", "b"),
        cause: "nth activation failed".into(),
    };
    assert_eq!(first.start().unwrap_err().to_string(), expected.to_string());
    assert_eq!(*log.lock().unwrap(), [format!("producer:a:{}", a.id)]);
    assert!(first.drain(4).is_empty());
    assert!(first.inject("missing", Value::local(())).is_err());
    first.start().unwrap();
    assert!(matches!(
        first.start().unwrap_err(),
        phenix_kernel::StartError::AlreadyStarted
    ));
    assert!(first.drain(2).is_empty());
    assert_eq!(a.polls.load(Ordering::SeqCst), 1);
    assert_eq!(b.polls.load(Ordering::SeqCst), 1);
    assert_eq!(other_a.polls.load(Ordering::SeqCst), 0);
    assert_eq!(
        second.start().unwrap_err().to_string(),
        expected.to_string()
    );
    second.start().unwrap();
    assert_eq!(composed.load(Ordering::SeqCst), 4);
    assert_eq!(
        *activations.lock().unwrap(),
        [
            ("a".into(), a.id),
            ("b".into(), b.id),
            ("a".into(), a.id),
            ("b".into(), b.id),
            ("a".into(), other_a.id),
            ("b".into(), other_b.id),
            ("a".into(), other_a.id),
            ("b".into(), other_b.id),
        ]
    );
}
