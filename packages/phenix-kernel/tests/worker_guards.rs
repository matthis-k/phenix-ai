mod support;

use phenix_graph::{
    Interface, Value,
    component::ComponentDef,
    compose::{Edit, NetworkPatch},
    connection::ConnectionError,
    process::{
        ActivationJob, DispatchResult, LocalProcessDispatcher, Process, ProcessContext,
        ProcessDispatcher, ProcessExecutionGuard, Step,
    },
    runtime::SessionState,
};
use phenix_kernel::{
    CallbackError, CandidateArtifacts, Contribution, InputEvent, Kernel, Producer, ProducerError,
};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{
    any::Any,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, ThreadId},
    time::Duration,
};

type Attempts = Arc<Mutex<Vec<(ThreadId, Result<(), String>)>>>;

#[derive(Clone, Default)]
struct Probe {
    kernel: Arc<Mutex<Option<Weak<Kernel>>>>,
    attempts: Attempts,
    entries: Arc<Mutex<Vec<(&'static str, ThreadId)>>>,
}
impl Probe {
    fn attempt(&self) {
        let kernel = self
            .kernel
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap();
        let result = kernel.with_session(|_| ());
        self.attempts
            .lock()
            .unwrap()
            .push((thread::current().id(), result));
    }
}

struct ReenterOnDrop(Probe);
impl Drop for ReenterOnDrop {
    fn drop(&mut self) {
        self.0.attempt();
    }
}

struct Guard {
    probe: Probe,
    name: &'static str,
    reenter_on_drop: bool,
}
impl ProcessExecutionGuard for Guard {
    fn enter(&self) -> Result<Box<dyn Any>, String> {
        self.probe
            .entries
            .lock()
            .unwrap()
            .push((self.name, thread::current().id()));
        if self.reenter_on_drop {
            Ok(Box::new(ReenterOnDrop(self.probe.clone())))
        } else {
            Ok(Box::new(()))
        }
    }
}

struct JoinedWorker(Probe);
impl ProcessDispatcher for JoinedWorker {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        let job = job.with_guard(Arc::new(Guard {
            probe: self.0.clone(),
            name: "dispatcher",
            reenter_on_drop: false,
        }));
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            let _ = send.send(job.run());
        });
        // A missing identity guard must fail the test, not hang in a worker join.
        DispatchResult::Completed(
            receive
                .recv_timeout(Duration::from_secs(2))
                .expect("worker deadlocked during same-kernel reentry"),
        )
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Process,
    GuardDrop,
    PanicPayloadDrop,
}
struct Worker {
    probe: Probe,
    mode: Mode,
}
impl Process for Worker {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        match self.mode {
            Mode::Process => self.probe.attempt(),
            Mode::GuardDrop => (),
            Mode::PanicPayloadDrop => std::panic::panic_any(ReenterOnDrop(self.probe.clone())),
        }
        Ok(Step::Finished)
    }
}
struct WorkerPlugin {
    probe: Probe,
    mode: Mode,
}
impl PluginInstance for WorkerPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let probe = self.probe.clone();
        let mode = self.mode;
        into.add(support::contribution(
            "worker",
            NetworkPatch::new([
                Edit::Add {
                    path: "worker".into(),
                    definition: Arc::new(ComponentDef::primitive(
                        Arc::new(Interface::new([], []).unwrap()),
                        Arc::new(move || Worker {
                            probe: probe.clone(),
                            mode,
                        }),
                    )),
                },
                Edit::AddSource("worker".into()),
            ]),
        ))
    }
}

fn check_worker_reentry(mode: Mode) {
    let probe = Probe::default();
    let snapshot = support::snapshot(
        "worker",
        WorkerPlugin {
            probe: probe.clone(),
            mode,
        },
    );
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options.execution.execution_guards.push(Arc::new(Guard {
        probe: probe.clone(),
        name: "application",
        reenter_on_drop: matches!(mode, Mode::GuardDrop),
    }));
    let mut kernel = Kernel::build_with_options(
        "worker-guards",
        [snapshot],
        Arc::new(JoinedWorker(probe.clone())),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    let kernel = Arc::new(kernel);
    *probe.kernel.lock().unwrap() = Some(Arc::downgrade(&kernel));

    let driver = thread::current().id();
    let report = kernel.run_steps(1).unwrap();
    assert_eq!(report.steps, 1);
    let failure = kernel
        .with_session(|session| session.failure().map(str::to_owned))
        .unwrap();
    if matches!(mode, Mode::PanicPayloadDrop) {
        assert_eq!(report.state, SessionState::Failed);
        assert!(
            failure
                .as_deref()
                .unwrap()
                .contains("process step panicked")
        );
    } else {
        assert_ne!(report.state, SessionState::Failed, "{failure:?}");
        assert_eq!(failure, None);
    }

    let attempts = probe.attempts.lock().unwrap();
    assert_eq!(attempts.len(), 1, "expected exactly one reentry attempt");
    let (worker, result) = &attempts[0];
    assert_ne!(*worker, driver);
    assert_eq!(
        result.as_ref().map_err(String::as_str),
        Err("reentrant access to the same kernel session")
    );
    assert_eq!(
        *probe.entries.lock().unwrap(),
        [("application", *worker), ("dispatcher", *worker)],
    );
}

#[test]
fn dispatcher_guard_append_preserves_kernel_identity_on_joined_worker() {
    check_worker_reentry(Mode::Process);
}

#[test]
fn application_guard_drop_reentry_is_rejected_on_worker() {
    check_worker_reentry(Mode::GuardDrop);
}

#[test]
fn panic_payload_drop_reentry_is_rejected_and_session_failure_is_contained() {
    check_worker_reentry(Mode::PanicPayloadDrop);
}

#[derive(Clone, Default)]
struct Drops {
    application: Arc<AtomicUsize>,
    code: Arc<AtomicUsize>,
    payload: Arc<AtomicUsize>,
}
impl Drops {
    fn assert_leased(&self) {
        assert_eq!(
            self.application.load(Ordering::SeqCst),
            0,
            "application lease released early"
        );
        assert_eq!(
            self.code.load(Ordering::SeqCst),
            0,
            "plugin code released early"
        );
    }
}
struct ApplicationLease(Arc<AtomicUsize>);
impl Drop for ApplicationLease {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct PluginPayload(Drops);
impl Drop for PluginPayload {
    fn drop(&mut self) {
        self.0.assert_leased();
        self.0.payload.fetch_add(1, Ordering::SeqCst);
    }
}
struct WrongTypeProducer(Option<PluginPayload>);
impl Producer for WrongTypeProducer {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        Ok(self.0.take().map(|payload| InputEvent {
            input: "input".into(),
            value: Value::local(payload),
        }))
    }
}
struct AdmissionPlugin(Drops);
impl Drop for AdmissionPlugin {
    fn drop(&mut self) {
        self.0.code.fetch_add(1, Ordering::SeqCst);
    }
}
impl PluginInstance for AdmissionPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let drops = self.0.clone();
        into.add(Contribution {
            id: "producer".into(),
            requires: vec![],
            routes: vec!["input".into()],
            patch: NetworkPatch::new([
                Edit::Add {
                    path: "sink".into(),
                    definition: support::sink(|_| panic!("wrong-typed value reached the sink")),
                },
                Edit::ExposeInput {
                    name: "input".into(),
                    targets: vec![support::path("sink", "run")],
                    capacity: support::capacity(),
                },
            ]),
            prepare: Some(Box::new(move |_| {
                let drops = drops.clone();
                let mut artifacts = CandidateArtifacts::default();
                artifacts.activate(move || {
                    Ok(Box::new(WrongTypeProducer(Some(PluginPayload(
                        drops.clone(),
                    )))))
                })?;
                Ok(artifacts)
            })),
        })
    }
}

#[test]
fn retained_wrong_type_admission_error_and_value_pin_application_and_plugin_code() {
    let drops = Drops::default();
    let snapshot = support::snapshot("admission", AdmissionPlugin(drops.clone()));
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options.execution.lease = Some(Arc::new(ApplicationLease(drops.application.clone())));
    let mut kernel = Kernel::build_with_options(
        "admission-leases",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    let mut events = kernel.drain(1).unwrap();
    assert_eq!(events.len(), 1);
    let error = events.pop().unwrap().result.unwrap_err();
    drop(events);
    drop(kernel);

    drops.assert_leased();
    assert_eq!(drops.payload.load(Ordering::SeqCst), 0);
    let ProducerError::Admission(admission) = &error else {
        panic!("expected a typed route admission error, got {error:?}");
    };
    assert_eq!(admission.reason, ConnectionError::WrongType);
    assert!(admission.value.borrow::<PluginPayload>().is_some());
    let value = admission.value.clone();
    drop(error);
    drops.assert_leased();
    assert_eq!(drops.payload.load(Ordering::SeqCst), 0);
    drop(value);
    assert_eq!(drops.payload.load(Ordering::SeqCst), 1);
    assert_eq!(drops.application.load(Ordering::SeqCst), 1);
    assert_eq!(drops.code.load(Ordering::SeqCst), 1);
}
