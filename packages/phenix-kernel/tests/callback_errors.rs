use phenix_graph::LocalExecutor;
use phenix_kernel::{
    BuildOptions, CallbackError, Contribution, DriverError, Kernel, Key, Notice, Producer,
    ProducerError, ResolutionError, StartError, WakeHandle,
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
}
struct Plugin(Phase, Arc<()>);
struct Source(Arc<()>);
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<Notice>, CallbackError> {
        Err(Box::new(Cause(self.0.clone())))
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
            compose: Box::new(move |context| {
                if matches!(phase, Phase::Compose) {
                    return Err(Box::new(Cause(identity.clone())));
                }
                let identity = identity.clone();
                context.activate(move || {
                    if matches!(phase, Phase::Activate) {
                        return Err(Box::new(Cause(identity.clone())));
                    }
                    Ok(Box::new(Source(identity.clone())))
                })
            }),
        })
    }
}

fn assert_cause(error: &dyn Error, identity: &Arc<()>) {
    let cause = error.source().unwrap().downcast_ref::<Cause>().unwrap();
    assert!(Arc::ptr_eq(&cause.0, identity));
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
    let error = Kernel::build("compose", [snapshot], Arc::new(LocalExecutor))
        .err()
        .unwrap();
    assert!(
        matches!(&error, ResolutionError::Composition { contribution, .. }
        if *contribution == Key::new("plugin", "callback"))
    );
    assert_cause(&error, &identity);

    let snapshot = plugins.reload(&key, Phase::Activate).unwrap();
    let mut kernel = Kernel::build("activate", [snapshot], Arc::new(LocalExecutor)).unwrap();
    let error = kernel.start().unwrap_err();
    assert!(matches!(&error, StartError::Activation { contribution, .. }
        if *contribution == Key::new("plugin", "callback")));
    assert_cause(&error, &identity);
    assert!(kernel.drain(1).is_empty());

    let snapshot = plugins.reload(&key, Phase::Poll).unwrap();
    let mut kernel = Kernel::build("poll", [snapshot], Arc::new(LocalExecutor)).unwrap();
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
            Arc::new(LocalExecutor),
            "binding",
            |_| Err(Box::new(Cause(identity.clone()))),
        )
        .err()
        .unwrap();
    assert!(matches!(&error, LifecycleError::Binding(_)));
    assert_cause(&error, &identity);
    assert!(host.active().is_none());
}
