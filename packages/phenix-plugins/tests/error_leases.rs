use phenix_graph::{Interface, contract::Contract, network::NetworkBuilder};
use phenix_plugin::{
    CallbackPhase, Code, Contribution, Failure, PluginCatalog, PluginInstance, Registrar,
};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

type Log = Arc<Mutex<Vec<&'static str>>>;
struct Life {
    name: &'static str,
    alive: Arc<AtomicBool>,
    log: Log,
}
impl Life {
    fn check(&self) {
        assert!(self.alive.load(Ordering::SeqCst));
    }
}
impl Drop for Life {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.log.lock().unwrap().push(self.name);
    }
}
struct Cause {
    dependencies: Vec<Arc<AtomicBool>>,
    log: Log,
}
impl Cause {
    fn check(&self) {
        assert!(
            self.dependencies
                .iter()
                .all(|alive| alive.load(Ordering::SeqCst))
        );
    }
}
impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.check();
        f.write_str("original plugin callback")
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
#[derive(Clone, Copy)]
enum Phase {
    Construct,
    Contribute,
    Bind,
}
struct Artifact(Life);
impl Contribution for Artifact {
    const KIND: &'static str = "test.artifact";
    fn id(&self) -> &str {
        self.0.check();
        "artifact"
    }
    fn component_path(&self) -> Option<&str> {
        self.0.check();
        Some("missing")
    }
}
struct Plugin {
    life: Life,
    factory: Arc<AtomicBool>,
    partial: Arc<AtomicBool>,
    phase: Phase,
}
impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        self.life.check();
        into.add(Artifact(Life {
            name: "partial",
            alive: self.partial.clone(),
            log: self.life.log.clone(),
        }))?;
        if matches!(self.phase, Phase::Contribute) {
            return Err(Failure::with_source(
                Code::PluginFailed,
                "contribution failed",
                Box::new(Cause {
                    dependencies: vec![
                        self.factory.clone(),
                        self.life.alive.clone(),
                        self.partial.clone(),
                    ],
                    log: self.life.log.clone(),
                }),
            ));
        }
        Ok(())
    }
}
#[test]
fn retained_constructor_contribution_and_bind_failures_pin_their_owners_in_drop_order() {
    for phase in [Phase::Construct, Phase::Contribute, Phase::Bind] {
        let log = Log::default();
        let factory = Arc::new(AtomicBool::new(true));
        let implementation = Arc::new(AtomicBool::new(true));
        let partial = Arc::new(AtomicBool::new(true));
        let factory_life = Life {
            name: "factory",
            alive: factory.clone(),
            log: log.clone(),
        };
        let error = {
            let mut catalog = PluginCatalog::new();
            let implementation = implementation.clone();
            let partial = partial.clone();
            let log = log.clone();
            let key = catalog
                .register("plugin", move |_: ()| {
                    factory_life.check();
                    if matches!(phase, Phase::Construct) {
                        return Err(Failure::with_source(
                            Code::PluginFailed,
                            "construction failed",
                            Box::new(Cause {
                                dependencies: vec![factory_life.alive.clone()],
                                log: log.clone(),
                            }),
                        ));
                    }
                    Ok(Plugin {
                        life: Life {
                            name: "implementation",
                            alive: implementation.clone(),
                            log: log.clone(),
                        },
                        factory: factory_life.alive.clone(),
                        partial: partial.clone(),
                        phase,
                    })
                })
                .unwrap();
            let mut plugins = catalog.instance();
            if matches!(phase, Phase::Bind) {
                let snapshot = plugins.load(&key, ()).unwrap();
                let contract =
                    Contract::new("test/empty@1", Arc::new(Interface::new([], []).unwrap()))
                        .unwrap();
                let compiled = NetworkBuilder::new().build().unwrap().compile().unwrap();
                snapshot
                    .bind::<Artifact>("artifact", &contract, &compiled)
                    .err()
                    .unwrap()
            } else {
                plugins.prepare_load(&key, ()).err().unwrap()
            }
        };
        assert!(factory.load(Ordering::SeqCst));
        if !matches!(phase, Phase::Construct) {
            assert!(implementation.load(Ordering::SeqCst));
            assert!(partial.load(Ordering::SeqCst));
        }
        let expected_phase = match phase {
            Phase::Construct => CallbackPhase::Construct,
            Phase::Contribute => CallbackPhase::Contribute,
            Phase::Bind => CallbackPhase::Bind,
        };
        assert_eq!(error.context.as_ref().unwrap().phase, expected_phase);
        assert!(!error.to_string().is_empty());
        assert!(!format!("{error:?}").is_empty());
        let mut source = error.source();
        let mut found = false;
        while let Some(error) = source {
            if let Some(cause) = error.downcast_ref::<Cause>() {
                cause.check();
                assert_eq!(cause.to_string(), "original plugin callback");
                found = true;
            }
            source = error.source();
        }
        assert_eq!(found, !matches!(phase, Phase::Bind));
        drop(error);
        assert!(!factory.load(Ordering::SeqCst));
        let expected: &[&str] = match phase {
            Phase::Construct => &["cause", "factory"],
            Phase::Contribute => &["cause", "partial", "implementation", "factory"],
            Phase::Bind => &["partial", "implementation", "factory"],
        };
        assert_eq!(log.lock().unwrap().as_slice(), expected);
    }
}
