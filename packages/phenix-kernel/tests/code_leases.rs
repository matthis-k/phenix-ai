mod support;
use phenix_graph::{
    Interface,
    component::ComponentDef,
    compose::{Edit, NetworkPatch},
    network::CompiledNetwork,
    process::{LocalProcessDispatcher, Process, ProcessContext, Step},
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, Contribution, InputEvent, Kernel, Key,
    Producer, ProducerError, ResolutionError, StartError,
    host::{KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

type Log = Arc<Mutex<Vec<&'static str>>>;
struct Life {
    name: &'static str,
    alive: Arc<AtomicBool>,
    log: Log,
}
impl Life {
    fn new(name: &'static str, log: &Log) -> Self {
        Self {
            name,
            alive: Arc::new(AtomicBool::new(true)),
            log: log.clone(),
        }
    }
}
impl Drop for Life {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.log.lock().unwrap().push(self.name);
    }
}
struct Cause {
    alive: Arc<AtomicBool>,
    log: Log,
}
impl Cause {
    fn check(&self) {
        assert!(
            self.alive.load(Ordering::SeqCst),
            "callback outlived plugin code"
        );
    }
}
impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.check();
        f.write_str("retained callback cause")
    }
}
impl fmt::Debug for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.check();
        f.write_str("Cause")
    }
}
impl Error for Cause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.check();
        None
    }
}
impl Drop for Cause {
    fn drop(&mut self) {
        self.check();
        self.log.lock().unwrap().push("cause");
    }
}
fn inspect(error: &(dyn Error + 'static)) {
    assert!(!error.to_string().is_empty());
    assert!(!format!("{error:?}").is_empty());
    let mut source = Some(error);
    while let Some(error) = source {
        if let Some(cause) = error.downcast_ref::<Cause>() {
            cause.check();
            assert!(cause.source().is_none());
            return;
        }
        source = error.source();
    }
    panic!("original concrete callback cause missing");
}

#[derive(Clone, Copy)]
enum Phase {
    Prepare,
    Activate,
    Poll,
    None,
}
struct Plugin {
    life: Life,
    phase: Phase,
}
struct Source {
    alive: Arc<AtomicBool>,
    log: Log,
    fail: bool,
}
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        if self.fail {
            Err(Box::new(Cause {
                alive: self.alive.clone(),
                log: self.log.clone(),
            }))
        } else {
            Ok(None)
        }
    }
}
impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let phase = self.phase;
        let alive = self.life.alive.clone();
        let log = self.life.log.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec![],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                if matches!(phase, Phase::Prepare) {
                    return Err(Box::new(Cause {
                        alive: alive.clone(),
                        log: log.clone(),
                    }));
                }
                let mut artifacts = CandidateArtifacts::default();
                let alive = alive.clone();
                let log = log.clone();
                artifacts.activate(move || {
                    if matches!(phase, Phase::Activate) {
                        return Err(Box::new(Cause {
                            alive: alive.clone(),
                            log: log.clone(),
                        }));
                    }
                    Ok(Box::new(Source {
                        alive: alive.clone(),
                        log: log.clone(),
                        fail: matches!(phase, Phase::Poll),
                    }))
                })?;
                Ok(artifacts)
            })),
        })
    }
}
#[test]
fn moved_prepare_activation_and_poll_causes_pin_code_and_application_until_cause_drop() {
    for phase in [Phase::Prepare, Phase::Activate, Phase::Poll] {
        let log = Log::default();
        let life = Life::new("plugin", &log);
        let alive = life.alive.clone();
        let application = Life::new("application", &log);
        let application_alive = application.alive.clone();
        let snapshot = support::snapshot("plugin", Plugin { life, phase });
        let mut options = BuildOptions::default();
        options.execution.lease = Some(Arc::new(application));
        let result = Kernel::build_with_options(
            "errors",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options,
        );
        let cause = match phase {
            Phase::Prepare => {
                let error = result.err().unwrap();
                inspect(&error);
                let ResolutionError::Composition { cause, .. } = error else {
                    unreachable!()
                };
                cause
            }
            Phase::Activate => {
                let mut kernel = result.unwrap();
                let error = kernel.start().unwrap_err();
                drop(kernel);
                inspect(&error);
                let StartError::Activation { cause, .. } = error else {
                    unreachable!()
                };
                cause
            }
            Phase::Poll => {
                let mut kernel = result.unwrap();
                kernel.start().unwrap();
                let result = kernel.drain(1).pop().unwrap().result;
                drop(kernel);
                let error = result.unwrap_err();
                inspect(&error);
                let ProducerError::Poll(cause) = error else {
                    unreachable!()
                };
                cause
            }
            Phase::None => unreachable!(),
        };
        assert!(alive.load(Ordering::SeqCst));
        assert!(application_alive.load(Ordering::SeqCst));
        inspect(cause.as_ref());
        drop(cause);
        assert!(!alive.load(Ordering::SeqCst));
        assert!(!application_alive.load(Ordering::SeqCst));
        assert_eq!(*log.lock().unwrap(), ["cause", "application", "plugin"]);
    }
}
#[test]
fn moved_host_binding_cause_pins_the_unpublished_candidate() {
    let log = Log::default();
    let life = Life::new("plugin", &log);
    let alive = life.alive.clone();
    let application = Life::new("application", &log);
    let application_alive = application.alive.clone();
    let error = {
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("plugin", |plugin: Plugin| Ok(plugin))
            .unwrap();
        let host = KernelHost::new(catalog.instance());
        let stage = host
            .plugins()
            .prepare_load(
                &key,
                Plugin {
                    life,
                    phase: Phase::None,
                },
            )
            .unwrap();
        let mut options = BuildOptions::default();
        options.execution.lease = Some(Arc::new(application));
        host.prepare(
            vec![stage],
            options,
            Arc::new(LocalProcessDispatcher),
            "binding",
            |_| {
                Err(Box::new(Cause {
                    alive: alive.clone(),
                    log: log.clone(),
                }))
            },
        )
        .err()
        .unwrap()
    };
    inspect(&error);
    let LifecycleError::Binding(cause) = error else {
        unreachable!()
    };
    inspect(cause.as_ref());
    assert!(application_alive.load(Ordering::SeqCst));
    drop(cause);
    assert!(!alive.load(Ordering::SeqCst));
    assert!(!application_alive.load(Ordering::SeqCst));
    assert_eq!(*log.lock().unwrap(), ["cause", "application", "plugin"]);
}

struct DeclarationPlugin {
    life: Life,
    calls: Arc<AtomicUsize>,
}
struct CheckProcess {
    alive: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl Process for CheckProcess {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        assert!(self.alive.load(Ordering::SeqCst));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Finished)
    }
}
impl Drop for CheckProcess {
    fn drop(&mut self) {
        assert!(self.alive.load(Ordering::SeqCst));
    }
}
impl PluginInstance for DeclarationPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let alive = self.life.alive.clone();
        let calls = self.calls.clone();
        let component = ComponentDef::primitive(
            Arc::new(Interface::new([], []).unwrap()),
            Arc::new(move || {
                assert!(alive.load(Ordering::SeqCst));
                CheckProcess {
                    alive: alive.clone(),
                    calls: calls.clone(),
                }
            }),
        );
        into.add(support::contribution(
            "declarations",
            NetworkPatch::new([
                Edit::Add {
                    path: "worker".into(),
                    definition: Arc::new(component),
                },
                Edit::AddSource("worker".into()),
            ]),
        ))?;
        into.add(Contribution {
            id: "export".into(),
            requires: vec![Key::new("plugin", "declarations")],
            routes: vec![],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(|context| {
                let mut artifacts = CandidateArtifacts::default();
                artifacts.export(context.compiled.clone())?;
                Ok(artifacts)
            })),
        })
    }
}
#[test]
fn detached_compiled_getters_exports_factories_processes_and_sessions_pin_both_leases() {
    for exported in [false, true] {
        let log = Log::default();
        let life = Life::new("plugin", &log);
        let alive = life.alive.clone();
        let application = Life::new("application", &log);
        let application_alive = application.alive.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let snapshot = support::snapshot(
            "plugin",
            DeclarationPlugin {
                life,
                calls: calls.clone(),
            },
        );
        let mut options = support::options(std::slice::from_ref(&snapshot));
        options.execution.lease = Some(Arc::new(application));
        let kernel = Kernel::build_with_options(
            "declarations",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options,
        )
        .unwrap();
        let compiled = if exported {
            kernel
                .export::<CompiledNetwork>(&Key::new("plugin", "export"))
                .unwrap()
                .clone()
        } else {
            kernel.compiled().clone()
        };
        drop(kernel);
        assert!(alive.load(Ordering::SeqCst));
        assert!(application_alive.load(Ordering::SeqCst));
        let factory = compiled.processes()[0].factory().clone();
        let mut session = compiled.start();
        drop(compiled);
        assert_eq!(session.run_until_idle(1).steps, 1);
        drop(session);
        assert!(alive.load(Ordering::SeqCst));
        assert!(application_alive.load(Ordering::SeqCst));
        let mut process = factory.spawn();
        drop(factory);
        assert!(alive.load(Ordering::SeqCst));
        assert!(application_alive.load(Ordering::SeqCst));
        assert!(matches!(
            process.step(&mut ProcessContext::default()).unwrap(),
            Step::Finished
        ));
        drop(process);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!alive.load(Ordering::SeqCst));
        assert!(!application_alive.load(Ordering::SeqCst));
        assert_eq!(*log.lock().unwrap(), ["application", "plugin"]);
    }
}
