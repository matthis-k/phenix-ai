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
    fn poll(&mut self) -> Result<Option<Notice>, String> {
        self.notices.pop_front().transpose()
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
        r.add(Contribution {
            id: "main".into(),
            requires: self.requires.clone(),
            compose: Box::new(move |g| {
                log.lock().unwrap().push(format!("compose:{name}"));
                if compile_fail {
                    *g = GraphBuilder::new("invalid").limits(CompileLimits {
                        max_work: 0,
                        ..Default::default()
                    });
                }
                let mark = Mark(format!("engine:{name}"), log.clone());
                let n = g
                    .node(&name, move |x: i64| {
                        mark.1.lock().unwrap().push(format!("execute:{}", mark.0));
                        x + 1
                    })
                    .map_err(|e| e.to_string())?;
                let input = g.expose_input(format!("input_{name}"), n.inlet())?;
                g.trigger(&name, [input])?;
                Ok(())
            }),
            activate: Some(Box::new(move || {
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
            })),
        })
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
    assert!(kernel.start().unwrap_err().contains("already started"));
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
        assert!(error.contains(expected), "{error}");
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
            .contains("duplicate contribution")
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
    assert_eq!(kernel.start().unwrap_err(), "startup failed");
    assert!(kernel.drain(10).is_empty());
    assert!(kernel.fire("alpha", vec![Value::local(1_i64)]).is_err());
    assert!(!log.lock().unwrap().iter().any(|s| s.starts_with("engine:")));
    // Retrying runs factories again rather than retaining a partial activation.
    assert_eq!(kernel.start().unwrap_err(), "startup failed");
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
