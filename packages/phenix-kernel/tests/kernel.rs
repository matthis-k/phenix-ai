use phenix_graph::{CompileLimits, GraphBuilder, LocalExecutor, Value};
use phenix_kernel::{Contribution, Kernel, Key, Notice, Producer};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

type Log = Arc<Mutex<Vec<String>>>;
struct Mark(String, Log);
impl Drop for Mark {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(self.0.clone());
    }
}
struct Plugin {
    name: String,
    requires: Vec<Key>,
    log: Log,
    compile_fail: bool,
    start_fail: bool,
}
impl Drop for Plugin {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push(format!("plugin:{}", self.name));
    }
}
struct Source {
    _mark: Mark,
    notices: VecDeque<Result<Notice, String>>,
}
impl Producer for Source {
    fn poll(&mut self) -> Result<Option<Notice>, phenix_kernel::CallbackError> {
        self.notices.pop_front().transpose().map_err(Into::into)
    }
}
impl PluginInstance for Plugin {
    fn contribute(&self, r: &mut Registrar) -> Result<(), Failure> {
        let name = self.name.clone();
        let log = self.log.clone();
        let compile_fail = self.compile_fail;
        let start_name = name.clone();
        let start_log = log.clone();
        let start_fail = self.start_fail;
        Contribution::new("main", move |context| {
                let g = &mut *context.graph;
                log.lock().unwrap().push(format!("compose:{name}"));
                if compile_fail {
                    *g = GraphBuilder::new("invalid").limits(CompileLimits {
                        max_work: 0,
                        ..Default::default()
                    });
                }
                let mark = Mark(format!("engine:{name}"), log.clone());
                let n = g.node(&name, move |x: i64| {
                    mark.1.lock().unwrap().push(format!("execute:{}", mark.0));
                    x + 1
                })?;
                let input = g.expose_input(format!("input_{name}"), n.inlet())?;
                g.trigger(&name, [input])?;
                let start_log = start_log.clone();
                let start_name = start_name.clone();
                context.activate(move || {
                    start_log
                        .lock()
                        .unwrap()
                        .push(format!("start:{start_name}"));
                    if start_fail {
                        return Err("startup failed".into());
                    }
                    Ok(Box::new(Source {
                        _mark: Mark(format!("producer:{start_name}"), start_log.clone()),
                        notices: VecDeque::from([
                            Ok(Notice {
                                trigger: start_name.clone(),
                                values: vec![Value::local(1_i64)],
                            }),
                            Ok(Notice {
                                trigger: "missing".into(),
                                values: vec![],
                            }),
                            Err("poll failed".into()),
                        ]),
                    }))
                })
            })
.after(self.requires.clone())
.route(vec![name.clone()])
.register(r)
    }
}
fn snapshots(
    a: Vec<Key>,
    b: Vec<Key>,
    compile_fail: bool,
    start_fail: bool,
    log: &Log,
) -> Vec<phenix_plugin::PluginSnapshot> {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("alpha", |p: Plugin| Ok(p)).unwrap();
    let other = catalog.register("beta", |p: Plugin| Ok(p)).unwrap();
    let mut set = catalog.instance();
    let first = set
        .load(
            &key,
            Plugin {
                name: "alpha".into(),
                requires: a,
                log: log.clone(),
                compile_fail,
                start_fail: false,
            },
        )
        .unwrap();
    let second = set
        .load(
            &other,
            Plugin {
                name: "beta".into(),
                requires: b,
                log: log.clone(),
                compile_fail: false,
                start_fail,
            },
        )
        .unwrap();
    vec![second, first]
}
#[test]
fn loader_order_waves_bounds_and_lifetimes() {
    let log = Log::default();
    let mut kernel = Kernel::build(
        "test",
        snapshots(vec![Key::new("beta", "main")], vec![], false, false, &log),
        Arc::new(LocalExecutor),
    )
    .unwrap();
    assert_eq!(*log.lock().unwrap(), ["compose:beta", "compose:alpha"]);
    assert!(kernel.fire("alpha", vec![Value::local(4_i64)]).is_err());
    assert!(kernel.drain(10).is_empty());
    kernel.start().unwrap();
    assert!(matches!(
        kernel.start().unwrap_err(),
        phenix_kernel::StartError::AlreadyStarted
    ));
    assert_eq!(
        *log.lock().unwrap(),
        ["compose:beta", "compose:alpha", "start:beta", "start:alpha"]
    );
    assert!(kernel.drain(0).is_empty());
    let events = kernel.drain(2);
    assert_eq!(events[0].producer, Key::new("beta", "main"));
    assert!(events.iter().all(|e| e.result.as_ref().unwrap().success()));
    assert!(
        kernel
            .fire("alpha", vec![Value::local(4_i64)])
            .unwrap()
            .success()
    );
    assert!(kernel.drain(4).iter().all(|e| e.result.is_err()));
    assert!(kernel.drain(10).is_empty());
    drop(kernel);
    let log = log.lock().unwrap();
    for name in ["alpha", "beta"] {
        let pos = |kind| {
            log.iter()
                .position(|s| s == &format!("{kind}:{name}"))
                .unwrap()
        };
        for other in ["alpha", "beta"] {
            let engine = log
                .iter()
                .position(|s| s == &format!("engine:{other}"))
                .unwrap();
            let plugin = log
                .iter()
                .position(|s| s == &format!("plugin:{other}"))
                .unwrap();
            assert!(pos("producer") < engine);
            assert!(pos("engine") < plugin);
        }
    }
}
#[test]
fn dependencies_fail_before_composition() {
    for (a, b, expected) in [
        (vec![Key::new("absent", "main")], vec![], "missing"),
        (
            vec![Key::new("beta", "main")],
            vec![Key::new("alpha", "main")],
            "cycle",
        ),
        (vec![Key::new("alpha", "main")], vec![], "cycle"),
        (
            vec![Key::new("beta", "main"); 2],
            vec![],
            "duplicate dependency",
        ),
    ] {
        let log = Log::default();
        let error = Kernel::build(
            "test",
            snapshots(a, b, false, false, &log),
            Arc::new(LocalExecutor),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains(expected), "{error}");
        assert!(
            !log.lock()
                .unwrap()
                .iter()
                .any(|s| s.starts_with("compose:"))
        );
    }
    let log = Log::default();
    let mut selected = snapshots(vec![], vec![], false, false, &log);
    selected.push(selected[0].clone());
    assert!(
        Kernel::build("test", selected, Arc::new(LocalExecutor))
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate snapshot owner")
    );
}
#[test]
fn compile_failure_never_starts_and_startup_rolls_back() {
    let log = Log::default();
    assert!(
        Kernel::build(
            "test",
            snapshots(vec![], vec![], true, false, &log),
            Arc::new(LocalExecutor)
        )
        .is_err()
    );
    assert!(!log.lock().unwrap().iter().any(|s| s.starts_with("start:")));
    log.lock().unwrap().clear();
    let mut kernel = Kernel::build(
        "test",
        snapshots(vec![], vec![], false, true, &log),
        Arc::new(LocalExecutor),
    )
    .unwrap();
    assert!(!log.lock().unwrap().iter().any(|s| s.starts_with("start:")));
    let expected = phenix_kernel::StartError::Activation {
        contribution: Key::new("beta", "main"),
        cause: "startup failed".into(),
    };
    let error = kernel.start().unwrap_err();
    assert_eq!(error.to_string(), expected.to_string());
    assert_eq!(
        std::error::Error::source(&error).unwrap().to_string(),
        "startup failed"
    );
    assert!(kernel.drain(10).is_empty());
    assert!(kernel.fire("alpha", vec![Value::local(1_i64)]).is_err());
    assert!(!log.lock().unwrap().iter().any(|s| s.starts_with("engine:")));
    // Retrying runs factories again rather than retaining a partial activation.
    assert_eq!(
        kernel.start().unwrap_err().to_string(),
        expected.to_string()
    );
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "producer:alpha")
            .count(),
        2
    );
    drop(kernel);
    let log = log.lock().unwrap();
    assert!(log.contains(&"producer:alpha".into()));
    assert!(!log.contains(&"producer:beta".into()));
    assert!(!log.iter().any(|s| s.starts_with("execute:")));
    let producer = log.iter().position(|s| s == "producer:alpha").unwrap();
    for name in ["alpha", "beta"] {
        let engine = log
            .iter()
            .position(|s| s == &format!("engine:{name}"))
            .unwrap();
        assert!(producer < engine);
        for other in ["alpha", "beta"] {
            let plugin = log
                .iter()
                .position(|s| s == &format!("plugin:{other}"))
                .unwrap();
            assert!(engine < plugin);
        }
    }
}
struct FailingExecutor;
impl phenix_graph::Executor for FailingExecutor {
    fn dispatch(
        &self,
        _: &dyn phenix_graph::NodeAction,
        _: phenix_graph::InvocationCtx,
    ) -> Result<Vec<phenix_graph::Emission>, String> {
        Err("execution failed".into())
    }
}
#[test]
fn wave_failures_remain_reports_and_do_not_stop_draining() {
    let log = Log::default();
    let mut kernel = Kernel::build(
        "test",
        snapshots(vec![], vec![], false, false, &log),
        Arc::new(FailingExecutor),
    )
    .unwrap();
    kernel.start().unwrap();
    let events = kernel.drain(4);
    assert_eq!(events.len(), 4);
    assert!(!events[0].result.as_ref().unwrap().success());
    assert!(!events[1].result.as_ref().unwrap().success());
    assert!(events[2].result.is_err());
    assert!(events[3].result.is_err());
}

struct DriverPlugin {
    routes: Vec<String>,
    polls: Arc<std::sync::atomic::AtomicUsize>,
}
struct WakeSource {
    polls: Arc<std::sync::atomic::AtomicUsize>,
    wake: Option<phenix_kernel::WakeHandle>,
}
impl Producer for WakeSource {
    fn poll(&mut self) -> Result<Option<Notice>, phenix_kernel::CallbackError> {
        self.polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Notification during polling must not be lost before the driver waits.
        if let Some(wake) = self.wake.take() {
            wake.shutdown();
        }
        Ok(None)
    }
    fn set_waker(&mut self, wake: phenix_kernel::WakeHandle) -> bool {
        self.wake = Some(wake);
        true
    }
}
impl PluginInstance for DriverPlugin {
    fn contribute(&self, r: &mut Registrar) -> Result<(), Failure> {
        let polls = self.polls.clone();
        Contribution::new("driver", move |context| {
                let g = &mut *context.graph;
                let n = g.node("sink", |_: i64| ())?;
                let input = g.expose_input("in", n.inlet())?;
                g.trigger("run", [input])?;
                let polls = polls.clone();
                context.activate(move || {
                    Ok(Box::new(WakeSource {
                        polls: polls.clone(),
                        wake: None,
                    }))
                })
            })
.route(self.routes.clone())
.register(r)
    }
}
fn driver_kernel(
    routes: &[&str],
    polls: Arc<std::sync::atomic::AtomicUsize>,
) -> Result<Kernel, phenix_kernel::ResolutionError> {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("driver", |p: DriverPlugin| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(
            &key,
            DriverPlugin {
                routes: routes.iter().map(|s| s.to_string()).collect(),
                polls,
            },
        )
        .unwrap();
    Kernel::build("driver", plugins.snapshots(), Arc::new(LocalExecutor))
}
#[test]
fn routes_are_preflighted_without_activation() {
    for (routes, expected) in [
        (vec!["missing"], "unknown producer route"),
        (vec!["run", "run"], "duplicate producer route"),
    ] {
        let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let error = driver_kernel(&routes, polls.clone()).err().unwrap();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
#[test]
fn driver_handles_shutdown_during_poll_and_rejects_legacy_sources() {
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut kernel = driver_kernel(&["run"], polls.clone()).unwrap();
    let wake = phenix_kernel::WakeHandle::default();
    assert!(kernel.run_until(&wake, |_| {}).is_err());
    kernel.start().unwrap();
    kernel.run_until(&wake, |_| panic!("idle")).unwrap();
    assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let mut kernel = Kernel::build(
        "legacy",
        snapshots(vec![], vec![], false, false, &Log::default()),
        Arc::new(LocalExecutor),
    )
    .unwrap();
    kernel.start().unwrap();
    assert!(
        kernel
            .run_until(&phenix_kernel::WakeHandle::default(), |_| {})
            .unwrap_err()
            .to_string()
            .contains("does not support")
    );
    assert_eq!(kernel.drain(2).len(), 2);
}
#[test]
fn producer_errors_are_distinct_from_admission_and_wave_failures() {
    let mut kernel = Kernel::build(
        "errors",
        snapshots(vec![], vec![], false, false, &Log::default()),
        Arc::new(LocalExecutor),
    )
    .unwrap();
    kernel.start().unwrap();
    let events = kernel.drain(6);
    assert!(matches!(
        events[2].result,
        Err(phenix_kernel::ProducerError::UndeclaredRoute(_))
    ));
    assert!(matches!(
        events[4].result,
        Err(phenix_kernel::ProducerError::Poll(_))
    ));
}

#[test]
fn empty_driver_blocks_until_cross_thread_shutdown() {
    let mut kernel = Kernel::build("empty", vec![], Arc::new(LocalExecutor)).unwrap();
    kernel.start().unwrap();
    let wake = phenix_kernel::WakeHandle::default();
    let driver_wake = wake.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        kernel
            .run_until(&driver_wake, |_| panic!("no producers"))
            .unwrap();
        tx.send(()).unwrap();
    });
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(30))
            .is_err()
    );
    wake.shutdown();
    rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    thread.join().unwrap();
}

#[test]
fn permanent_poll_failure_is_reported_once_and_driver_remains_shutdown_capable() {
    struct Broken(Arc<std::sync::atomic::AtomicUsize>);
    impl Producer for Broken {
        fn poll(&mut self) -> Result<Option<Notice>, phenix_kernel::CallbackError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("permanent".into())
        }
        fn set_waker(&mut self, _: phenix_kernel::WakeHandle) -> bool {
            true
        }
    }
    impl PluginInstance for Broken {
        fn contribute(&self, r: &mut Registrar) -> Result<(), Failure> {
            let polls = self.0.clone();
            Contribution::new("broken", move |c| {
                    let polls = polls.clone();
                    c.activate(move || Ok(Box::new(Broken(polls.clone()))))
                })
.register(r)
        }
    }
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("broken", |p: Broken| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&key, Broken(polls.clone())).unwrap();
    let mut kernel = Kernel::build("broken", plugins.snapshots(), Arc::new(LocalExecutor)).unwrap();
    kernel.start().unwrap();
    assert_eq!(kernel.drain(100).len(), 1);
    assert!(kernel.drain(100).is_empty());
    assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 1);
    polls.store(0, std::sync::atomic::Ordering::SeqCst);
    let mut kernel = Kernel::build(
        "broken-driver",
        plugins.snapshots(),
        Arc::new(LocalExecutor),
    )
    .unwrap();
    kernel.start().unwrap();
    let wake = phenix_kernel::WakeHandle::default();
    let driver_wake = wake.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        kernel.run_until(&driver_wake, |event| {
            let _ = tx.send(event);
        })
    });
    let first = rx.recv_timeout(std::time::Duration::from_secs(2));
    let repeated = rx.recv_timeout(std::time::Duration::from_millis(30));
    wake.shutdown();
    thread.join().unwrap().unwrap();
    assert!(matches!(
        first.unwrap().result,
        Err(phenix_kernel::ProducerError::Poll(_))
    ));
    assert!(repeated.is_err(), "terminal failure must not hot spin");
    assert_eq!(polls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
