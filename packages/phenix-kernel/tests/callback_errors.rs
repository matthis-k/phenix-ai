mod support;
use phenix_graph::{
    compose::{Edit, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, Contribution, DriverError, InputEvent, Kernel,
    Key, Producer, ProducerError, ResolutionError, StartError, WakeHandle,
    host::{KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{error::Error, fmt, sync::Arc};

#[derive(Debug)]
struct Cause(Arc<()>);
impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("original callback cause")
    }
}
impl Error for Cause {}

#[derive(Clone, Copy)]
enum Phase {
    Compose,
    Activate,
    Poll,
    PollLive,
    ComposePanic,
    ActivatePanic,
    WakePanic,
}
struct Plugin(Phase, Arc<()>);
struct Source(Arc<()>, bool);
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        Err(Box::new(Cause(self.0.clone())))
    }

    fn set_waker(&mut self, _wake: WakeHandle) -> bool {
        if self.1 {
            panic!("deliberate wake registration panic");
        }
        false
    }
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let phase = self.0;
        let identity = self.1.clone();
        registrar.add(Contribution {
            id: "callback".into(),
            requires: vec![],
            routes: vec![],
            patch: if matches!(phase, Phase::WakePanic | Phase::PollLive) {
                NetworkPatch::new([
                    Edit::Add {
                        path: "wake-sink".into(),
                        definition: support::sink(|_| {}),
                    },
                    Edit::ExposeInput {
                        name: "input".into(),
                        targets: vec![support::path("wake-sink", "run")],
                        capacity: support::capacity(),
                    },
                ])
            } else {
                NetworkPatch::new([])
            },
            prepare: Some(Box::new(move |_| {
                if matches!(phase, Phase::ComposePanic) {
                    panic!("deliberate preparation panic");
                }
                if matches!(phase, Phase::Compose) {
                    return Err(Box::new(Cause(identity.clone())));
                }
                let identity = identity.clone();
                let mut artifacts = CandidateArtifacts::default();
                artifacts.activate(move || {
                    if matches!(phase, Phase::ActivatePanic) {
                        panic!("deliberate activation panic");
                    }
                    if matches!(phase, Phase::Activate) {
                        return Err(Box::new(Cause(identity.clone())));
                    }
                    Ok(Box::new(Source(
                        identity.clone(),
                        matches!(phase, Phase::WakePanic),
                    )))
                })?;
                Ok(artifacts)
            })),
        })
    }
}

fn assert_cause(error: &dyn Error, identity: &Arc<()>) {
    let mut source = error.source();
    while let Some(error) = source {
        if let Some(cause) = error.downcast_ref::<Cause>() {
            assert!(Arc::ptr_eq(&cause.0, identity));
            return;
        }
        source = error.source();
    }
    panic!("original typed callback cause missing from source chain");
}

#[test]
fn callback_boundaries_preserve_exact_source_and_contribution_phase() {
    let identity = Arc::new(());
    let mut catalog = PluginCatalog::new();
    let captured = identity.clone();
    let key = catalog
        .register("plugin", move |phase| Ok(Plugin(phase, captured.clone())))
        .unwrap();
    let mut plugins = catalog.instance();
    let snapshot = plugins.load(&key, Phase::Compose).unwrap();
    let error = Kernel::build("compose", [snapshot], Arc::new(LocalProcessDispatcher))
        .err()
        .unwrap();
    assert!(
        matches!(&error, ResolutionError::Composition { contribution, .. }
        if *contribution == Key::new("plugin", "callback"))
    );
    assert_cause(&error, &identity);

    let snapshot = plugins.reload(&key, Phase::Activate).unwrap();
    let mut kernel =
        Kernel::build("activate", [snapshot], Arc::new(LocalProcessDispatcher)).unwrap();
    let error = kernel.start().unwrap_err();
    assert!(matches!(&error, StartError::Activation { contribution, .. }
        if *contribution == Key::new("plugin", "callback")));
    assert_cause(&error, &identity);
    assert!(kernel.drain(1).is_empty());

    // Unsupported wake registration is a live-session boundary. The
    // host's earlier Phase::Poll fixture stays an intentionally empty graph.
    let snapshot = plugins.reload(&key, Phase::PollLive).unwrap();
    let options = support::options(std::slice::from_ref(&snapshot));
    let mut kernel = Kernel::build_with_options(
        "poll",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    assert_eq!(
        kernel
            .run_until(&WakeHandle::default(), |_| {})
            .unwrap_err(),
        DriverError::NotStarted
    );
    kernel.start().unwrap();
    assert_eq!(
        kernel
            .run_until(&WakeHandle::default(), |_| {})
            .unwrap_err(),
        DriverError::UnsupportedWake {
            producer: Key::new("plugin", "callback")
        }
    );
    let event = kernel.drain(1).pop().unwrap();
    assert_eq!(event.producer, Key::new("plugin", "callback"));
    let error = event.result.err().unwrap();
    assert!(matches!(&error, ProducerError::Poll(_)));
    assert_cause(&error, &identity);
    assert!(kernel.drain(1).is_empty());
}

#[test]
fn host_binding_preserves_source_without_publishing_or_starting() {
    let identity = Arc::new(());
    let host = KernelHost::new(PluginCatalog::new().instance());
    let error = host
        .prepare(
            vec![],
            BuildOptions::default(),
            Arc::new(LocalProcessDispatcher),
            "binding",
            |_| Err(Box::new(Cause(identity.clone()))),
        )
        .err()
        .unwrap();
    assert!(matches!(&error, LifecycleError::Binding(_)));
    assert_cause(&error, &identity);
    assert!(host.active().is_none());
}

#[test]
fn panicking_plugin_prepare_and_activate_return_errors_and_preserve_active_generation() {
    let identity = Arc::new(());
    let mut catalog = PluginCatalog::new();
    let captured = identity.clone();
    let key = catalog
        .register("plugin", move |phase| Ok(Plugin(phase, captured.clone())))
        .unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let initial = host
        .prepare(
            vec![host.plugins().prepare_load(&key, Phase::Poll).unwrap()],
            BuildOptions::default(),
            Arc::new(LocalProcessDispatcher),
            "initial",
            |_| Ok(()),
        )
        .unwrap();
    let active = host.publish(initial).unwrap();

    for (phase, marker) in [
        (Phase::ComposePanic, "plugin preparation panicked"),
        (Phase::ActivatePanic, "producer activation panicked"),
    ] {
        let candidate = host.plugins().prepare_reload(&key, phase).unwrap();
        let error = host
            .prepare(
                vec![candidate],
                BuildOptions::default(),
                Arc::new(LocalProcessDispatcher),
                "panic",
                |_| Ok(()),
            )
            .err()
            .unwrap();
        match phase {
            Phase::ComposePanic => assert!(matches!(
                &error,
                LifecycleError::Build(cause)
                    if matches!(cause.as_ref(), ResolutionError::Composition {
                        contribution, ..
                    } if *contribution == Key::new("plugin", "callback"))
            )),
            Phase::ActivatePanic => assert!(matches!(
                &error,
                LifecycleError::Start(StartError::Activation { contribution, .. })
                    if *contribution == Key::new("plugin", "callback")
            )),
            _ => unreachable!(),
        }
        assert!(error.to_string().contains(marker), "{error}");
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);
    }
}

#[test]
fn wake_registration_panic_returns_typed_driver_error_and_retains_the_producer() {
    let identity = Arc::new(());
    let mut catalog = PluginCatalog::new();
    let captured = identity.clone();
    let key = catalog
        .register("plugin", move |phase| Ok(Plugin(phase, captured.clone())))
        .unwrap();
    let mut plugins = catalog.instance();
    let snapshot = plugins.load(&key, Phase::WakePanic).unwrap();
    let options = support::options(std::slice::from_ref(&snapshot));
    let mut kernel = Kernel::build_with_options(
        "wake-panic",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    kernel.start().unwrap();

    let failure = kernel
        .run_until(&WakeHandle::default(), |_| panic!("driver must not run"))
        .unwrap_err();
    assert_eq!(
        failure,
        DriverError::WakeRegistrationPanicked {
            producer: Key::new("plugin", "callback"),
        }
    );
    // A failed registration must restore the owned producer slot. The original
    // typed poll error can still be observed, and no extra producer is created.
    let event = kernel.drain(1).pop().expect("producer slot lost");
    assert_eq!(event.producer, Key::new("plugin", "callback"));
    let error = event.result.expect_err("expected one terminal poll error");
    assert_cause(&error, &identity);
    assert!(kernel.drain(1).is_empty());
}

#[test]
fn panicking_host_binding_returns_typed_error_without_replacing_active_generation() {
    let mut host = KernelHost::new(PluginCatalog::new().instance());
    let initial = host
        .prepare(
            vec![],
            BuildOptions::default(),
            Arc::new(LocalProcessDispatcher),
            "initial",
            |_| Ok(()),
        )
        .unwrap();
    let active = host.publish(initial).unwrap();
    let old_epoch = active.epoch();

    let error = host
        .prepare(
            vec![],
            BuildOptions::default(),
            Arc::new(LocalProcessDispatcher),
            "panicking-binding",
            |_| -> Result<(), CallbackError> {
                panic!("intentional host binding panic");
            },
        )
        .err()
        .unwrap();
    assert!(matches!(&error, LifecycleError::Binding(_)));
    assert!(error.to_string().contains("generation binding panicked"));
    assert!(Arc::ptr_eq(&host.active().unwrap(), &active));
    assert_eq!(host.active().unwrap().epoch(), old_epoch);
    assert_eq!(host.plugins().snapshots().count(), 0);

    // Failed preparation did not consume the next publication epoch.
    let after = host
        .prepare(
            vec![],
            BuildOptions::default(),
            Arc::new(LocalProcessDispatcher),
            "after",
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(host.publish(after).unwrap().epoch(), old_epoch + 1);
}
