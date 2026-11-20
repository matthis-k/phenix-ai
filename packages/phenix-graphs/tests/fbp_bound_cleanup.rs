//! Adversarial public-API cleanup and preflight regressions for bound configuration.
use phenix_graph::{
    BoundDeclaration, CompiledNetwork, Failure, InputKey, Interface, NetworkBuilder, NetworkError,
    OutputKey, PortDef, Process, ProcessContext, ProcessFactory, ResourceGrant, SessionOptions,
    Step, ValidatedResourceSet,
    runtime::{
        CleanupStage, Readiness, ReadinessPolicy, ReadinessSnapshot, RuntimeError, SessionState,
    },
};
use std::{
    error::Error,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

type Log = Arc<Mutex<Vec<(&'static str, &'static str)>>>;
type Owners = Vec<Weak<dyn Send + Sync>>;

struct PanicPayload {
    name: &'static str,
    log: Log,
    owners: Owners,
}
impl Drop for PanicPayload {
    fn drop(&mut self) {
        assert!(
            self.owners.iter().all(|owner| owner.upgrade().is_some()),
            "panic payload lost code owners"
        );
        self.log.lock().unwrap().push((self.name, "payload"));
        // Exercise bounded containment of a second, destructor-originated panic.
        panic!("panic payload destructor");
    }
}

struct Probe {
    name: &'static str,
    log: Log,
    owners: Owners,
    hostile: bool,
}
impl Drop for Probe {
    fn drop(&mut self) {
        assert!(
            self.owners.iter().all(|owner| owner.upgrade().is_some()),
            "value lost code owners"
        );
        self.log.lock().unwrap().push((self.name, "drop"));
        if self.hostile {
            std::panic::panic_any(PanicPayload {
                name: self.name,
                log: self.log.clone(),
                owners: self.owners.clone(),
            });
        }
    }
}

#[derive(Default)]
struct Calls {
    spawn: AtomicUsize,
    factory_readiness: AtomicUsize,
    correlation: AtomicUsize,
    notification: AtomicUsize,
    select: AtomicUsize,
    closure: AtomicUsize,
}
impl Calls {
    fn assert_zero(&self) {
        for counter in [
            &self.spawn,
            &self.factory_readiness,
            &self.correlation,
            &self.notification,
            &self.select,
            &self.closure,
        ] {
            assert_eq!(counter.load(Ordering::SeqCst), 0);
        }
    }
}
struct Policy {
    probe: Probe,
    calls: Arc<Calls>,
}
impl ReadinessPolicy for Policy {
    fn select(&self, _: &ReadinessSnapshot) -> Vec<String> {
        self.calls.select.fetch_add(1, Ordering::SeqCst);
        vec![]
    }
    fn closure_ports(&self, _: usize) -> Vec<String> {
        self.calls.closure.fetch_add(1, Ordering::SeqCst);
        vec![]
    }
    fn notification_driven(&self) -> bool {
        // Read the host-owned name without formatting or running the probe.
        assert!(!self.probe.name.is_empty());
        self.calls.notification.fetch_add(1, Ordering::SeqCst);
        true
    }
}
struct Idle;
impl Process for Idle {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        Ok(Step::Finished)
    }
}
struct Factory(Arc<Calls>);
impl ProcessFactory for Factory {
    fn spawn(&self) -> Box<dyn Process> {
        self.0.spawn.fetch_add(1, Ordering::SeqCst);
        Box::new(Idle)
    }
    fn readiness(&self) -> Readiness {
        self.0.factory_readiness.fetch_add(1, Ordering::SeqCst);
        Readiness::Any
    }
    fn correlation_lanes(&self) -> Option<NonZeroUsize> {
        self.0.correlation.fetch_add(1, Ordering::SeqCst);
        None
    }
}
fn network(calls: &Arc<Calls>) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let worker = builder
        .node(
            "worker",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("in")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(Factory(calls.clone())),
        )
        .unwrap();
    builder
        .expose_input(
            "input",
            worker.input::<i64>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("output", worker.output::<i64>("out").unwrap())
        .unwrap();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::default()),
            Arc::new(Factory(calls.clone())),
        )
        .unwrap();
    builder.start(&source).unwrap();
    builder.build().unwrap().compile().unwrap()
}
fn probe(name: &'static str, log: &Log, owners: &Owners, hostile: bool) -> Probe {
    Probe {
        name,
        log: log.clone(),
        owners: owners.clone(),
        hostile,
    }
}
fn policy(
    name: &'static str,
    log: &Log,
    owners: &Owners,
    calls: &Arc<Calls>,
    hostile: bool,
) -> Readiness {
    Readiness::Custom(Arc::new(Policy {
        probe: probe(name, log, owners, hostile),
        calls: calls.clone(),
    }))
}
fn resources(name: &'static str, log: &Log, owners: &Owners) -> ValidatedResourceSet {
    ValidatedResourceSet::try_new(vec![ResourceGrant::new(
        "probe",
        probe(name, log, owners, true),
    )])
    .unwrap()
}
fn declaration(compiled: &CompiledNetwork, path: &str, log: &Log) -> (BoundDeclaration, Owners) {
    let guard: Arc<dyn Send + Sync> = Arc::new(());
    let guard_owners = vec![Arc::downgrade(&guard)];
    let first: Arc<dyn Send + Sync> = Arc::new(probe("lease1", log, &guard_owners, true));
    let second: Arc<dyn Send + Sync> = Arc::new(probe("lease2", log, &guard_owners, true));
    let owners = vec![
        Arc::downgrade(&guard),
        Arc::downgrade(&first),
        Arc::downgrade(&second),
    ];
    (
        compiled
            .declaration(path)
            .unwrap()
            .with_lease(guard)
            .with_lease(first)
            .with_lease(second),
        owners,
    )
}
fn assert_cleaned(log: &Log, names: &[&'static str], owners: &Owners) {
    let events = log.lock().unwrap();
    for &name in names {
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == (name, "drop"))
                .count(),
            1,
            "{events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == (name, "payload"))
                .count(),
            1,
            "{events:?}"
        );
    }
    let lease2 = events
        .iter()
        .position(|event| *event == ("lease2", "drop"))
        .unwrap();
    let lease1 = events
        .iter()
        .position(|event| *event == ("lease1", "drop"))
        .unwrap();
    assert!(lease2 < lease1, "lease cleanup must be reverse: {events:?}");
    for &name in names.iter().filter(|name| !name.starts_with("lease")) {
        assert!(
            events
                .iter()
                .position(|event| *event == (name, "payload"))
                .unwrap()
                < lease2,
            "{events:?}"
        );
    }
    assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
}

#[test]
fn abandoned_and_cloned_public_options_keep_every_owner_through_hostile_cleanup() {
    for clone_first in [false, true] {
        let calls = Arc::new(Calls::default());
        let compiled = network(&calls);
        let log = Log::default();
        let (declaration, owners) = declaration(&compiled, "worker", &log);
        let mut options = SessionOptions::default();
        for name in ["policy1", "policy2"] {
            options
                .bound
                .readiness(&declaration, policy(name, &log, &owners, &calls, true));
        }
        for name in ["resource1", "resource2"] {
            options
                .bound
                .resources(&declaration, resources(name, &log, &owners));
        }
        let cloned = options.clone();
        drop((declaration, compiled));
        let (first, last) = if clone_first {
            (cloned, options)
        } else {
            (options, cloned)
        };
        assert!(catch_unwind(AssertUnwindSafe(|| drop(first))).is_ok());
        assert!(log.lock().unwrap().is_empty());
        assert!(catch_unwind(AssertUnwindSafe(|| drop(last))).is_ok());
        calls.assert_zero();
        assert_cleaned(
            &log,
            &[
                "policy1",
                "policy2",
                "resource1",
                "resource2",
                "lease1",
                "lease2",
            ],
            &owners,
        );
    }
}

#[test]
fn every_detached_handle_contains_each_final_hostile_lease_and_panic_payload() {
    for kind in [
        "declaration",
        "input",
        "output",
        "ingress",
        "capture",
        "source",
    ] {
        let compiled = network(&Arc::default());
        let log = Log::default();
        let path = if kind == "source" { "source" } else { "worker" };
        let (declaration, owners) = declaration(&compiled, path, &log);
        let handle: Box<dyn Send + Sync> = match kind {
            "declaration" => Box::new(declaration.clone()),
            "input" => Box::new(declaration.input::<i64>("in").unwrap()),
            "output" => Box::new(declaration.output::<i64>("out").unwrap()),
            "source" => Box::new(compiled.source(&declaration).unwrap()),
            _ => {
                // Public exports issue their own handles; attach the exact same
                // explicit owners without granting an internal endpoint privilege.
                let held: Vec<_> = owners
                    .iter()
                    .map(|owner| owner.upgrade().unwrap())
                    .collect();
                if kind == "ingress" {
                    let mut ingress = compiled
                        .bind_ingress("input", InputKey::<i64>::new("in"))
                        .unwrap();
                    for lease in held {
                        ingress = ingress.with_lease(lease);
                    }
                    Box::new(ingress)
                } else {
                    let mut capture = compiled
                        .bind_capture("output", OutputKey::<i64>::new("out"))
                        .unwrap();
                    for lease in held {
                        capture = capture.with_lease(lease);
                    }
                    Box::new(capture)
                }
            }
        };
        drop((declaration, compiled));
        assert!(
            catch_unwind(AssertUnwindSafe(|| drop(handle))).is_ok(),
            "{kind}"
        );
        assert_cleaned(&log, &["lease1", "lease2"], &owners);
    }
}

#[test]
fn successful_start_and_preparation_failure_keep_configured_code_until_final_cleanup() {
    for foreign in [false, true] {
        let calls = Arc::new(Calls::default());
        let compiled = network(&calls);
        let receiving = if foreign {
            network(&calls)
        } else {
            compiled.clone()
        };
        let log = Log::default();
        let (declaration, owners) = declaration(&compiled, "worker", &log);
        let mut options = SessionOptions::default();
        options
            .bound
            .readiness(&declaration, policy("policy", &log, &owners, &calls, true));
        options
            .bound
            .resources(&declaration, resources("resource", &log, &owners));
        drop((declaration, compiled));
        let session = receiving.start_with_options(options);
        if foreign {
            calls.assert_zero();
            assert_eq!(session.state(), SessionState::Failed);
            assert_eq!(
                session
                    .failure()
                    .unwrap()
                    .source()
                    .unwrap()
                    .downcast_ref::<NetworkError>(),
                Some(&NetworkError::ForeignScope)
            );
        } else {
            assert!(session.failure().is_none());
        }
        drop(receiving);
        assert!(catch_unwind(AssertUnwindSafe(|| drop(session))).is_ok());
        assert_cleaned(&log, &["policy", "resource", "lease1", "lease2"], &owners);
    }
}

#[test]
fn bound_overrides_report_actual_cleanup_preserve_first_failure_and_skip_all_callbacks() {
    let calls = Arc::new(Calls::default());
    let compiled = network(&calls);
    let log = Log::default();
    let (declaration, owners) = declaration(&compiled, "worker", &log);
    let mut options = SessionOptions::default();
    options.readiness.insert(
        "worker".into(),
        policy("dynamic", &log, &owners, &calls, true),
    );
    options.resources.insert(
        "worker".into(),
        resources("dynamic_resource", &log, &owners),
    );
    for name in ["shadow1", "shadow2"] {
        options
            .bound
            .readiness(&declaration, policy(name, &log, &owners, &calls, true));
    }
    options
        .bound
        .readiness(&declaration, policy("final", &log, &owners, &calls, false));
    options
        .bound
        .resources(&declaration, resources("shadow_resource", &log, &owners));
    options
        .bound
        .resources(&declaration, ValidatedResourceSet::default());
    drop(declaration);
    let session = catch_unwind(AssertUnwindSafe(|| compiled.start_with_options(options)))
        .expect("replacement cleanup must not unwind start");
    calls.assert_zero();
    assert_eq!(session.state(), SessionState::Failed);
    let failure = session.failure().unwrap();
    assert_eq!(
        failure.source().unwrap().downcast_ref::<RuntimeError>(),
        Some(&RuntimeError::DestructorPanicked {
            stage: CleanupStage::Configuration
        })
    );
    assert_eq!(failure.context().path.as_deref(), Some("worker"));
    assert_eq!(failure.context().field.as_deref(), Some("readiness"));
    // Two shadowed policies, the dynamic policy, and both replaced resource sets.
    assert_eq!(failure.cleanup().len(), 4);
    assert_eq!(
        failure
            .cleanup()
            .iter()
            .map(|f| f.context().field.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["readiness", "readiness", "resources", "resources"]
    );
    let escaped = failure.clone();
    drop((session, compiled));
    assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(escaped))).is_ok());
    assert_cleaned(
        &log,
        &[
            "shadow1",
            "shadow2",
            "dynamic",
            "shadow_resource",
            "dynamic_resource",
            "lease1",
            "lease2",
        ],
        &owners,
    );
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|event| **event == ("final", "drop"))
            .count(),
        1
    );
}

#[test]
fn foreign_selector_validates_whole_inventory_before_replacement_or_registration() {
    let calls = Arc::new(Calls::default());
    let compiled = network(&calls);
    let other = network(&calls);
    let log = Log::default();
    let (declaration, owners) = declaration(&compiled, "worker", &log);
    let mut options = SessionOptions::default();
    let dynamic = Arc::new(Policy {
        probe: probe("dynamic", &log, &owners, false),
        calls: calls.clone(),
    });
    options
        .readiness
        .insert("worker".into(), Readiness::Custom(dynamic.clone()));
    options
        .bound
        .readiness(&declaration, policy("bound", &log, &owners, &calls, true));
    options.bound.resources(
        &other.declaration("worker").unwrap(),
        ValidatedResourceSet::default(),
    );
    let session = compiled.start_with_options(options);
    calls.assert_zero();
    assert_eq!(
        session
            .failure()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<NetworkError>(),
        Some(&NetworkError::ForeignScope)
    );
    assert_eq!(
        Arc::strong_count(&dynamic),
        2,
        "dynamic value must not be replaced before all selectors validate"
    );
    drop(dynamic);
    drop((declaration, compiled, other));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(session))).is_ok());
    assert_cleaned(&log, &["bound", "lease1", "lease2"], &owners);
}

#[test]
fn detached_source_job_contains_final_handle_lease_releases() {
    let compiled = network(&Arc::default());
    let log = Log::default();
    let (declaration, owners) = declaration(&compiled, "source", &log);
    let source = compiled.source(&declaration).unwrap();
    let mut session = compiled.start_with_options(SessionOptions {
        auto_start_sources: false,
        ..Default::default()
    });
    let id = session.begin_bound_request(None).unwrap();
    session.start_bound_request_source(id, &source).unwrap();
    let (_, job) = session.next_job().unwrap();
    drop((source, declaration, compiled));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(session))).is_ok());
    assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(job))).is_ok());
    assert_cleaned(&log, &["lease1", "lease2"], &owners);
}

#[test]
fn shared_replacements_do_not_fabricate_cleanup_failures_and_last_bound_policy_wins() {
    let calls = Arc::new(Calls::default());
    let old_calls = Arc::new(Calls::default());
    let final_calls = Arc::new(Calls::default());
    let compiled = network(&calls);
    let log = Log::default();
    let (declaration, owners) = declaration(&compiled, "worker", &log);
    let shared_policy = policy("shared_policy", &log, &owners, &old_calls, true);
    let shared_resources = resources("shared_resource", &log, &owners);
    let mut retained = SessionOptions::default();
    retained
        .bound
        .readiness(&declaration, shared_policy.clone());
    retained
        .bound
        .resources(&declaration, shared_resources.clone());
    let mut options = SessionOptions::default();
    options
        .readiness
        .insert("worker".into(), shared_policy.clone());
    options.resources.insert("worker".into(), shared_resources);
    options.bound.readiness(&declaration, shared_policy);
    options.bound.readiness(
        &declaration,
        policy("final_policy", &log, &owners, &final_calls, false),
    );
    options
        .bound
        .resources(&declaration, ValidatedResourceSet::default());
    let session = compiled.start_with_options(options);
    assert!(session.failure().is_none());
    old_calls.assert_zero();
    assert_eq!(final_calls.notification.load(Ordering::SeqCst), 1);
    assert!(
        log.lock().unwrap().is_empty(),
        "nonfinal owners must not report actual cleanup"
    );
    drop((declaration, compiled));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(session))).is_ok());
    assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
    assert!(catch_unwind(AssertUnwindSafe(|| drop(retained))).is_ok());
    assert_cleaned(
        &log,
        &["shared_policy", "shared_resource", "lease1", "lease2"],
        &owners,
    );
}

fn execution_declaration(
    compiled: &CompiledNetwork,
    path: &str,
    log: &Log,
    base_hostile: bool,
    appended_hostile: bool,
) -> (Arc<dyn Send + Sync>, BoundDeclaration, Owners) {
    let base: Arc<dyn Send + Sync> = Arc::new(probe("base", log, &Owners::new(), base_hostile));
    let base_owners = vec![Arc::downgrade(&base)];
    let first: Arc<dyn Send + Sync> =
        Arc::new(probe("extra1", log, &base_owners, appended_hostile));
    let second: Arc<dyn Send + Sync> =
        Arc::new(probe("extra2", log, &base_owners, appended_hostile));
    let owners = vec![
        Arc::downgrade(&base),
        Arc::downgrade(&first),
        Arc::downgrade(&second),
    ];
    let declaration = compiled
        .declaration(path)
        .unwrap()
        .with_lease(first)
        .with_lease(second);
    (base, declaration, owners)
}

fn assert_base_released_last(
    log: &Log,
    owners: &Owners,
    base_hostile: bool,
    appended_hostile: bool,
    values: &[&'static str],
) {
    let events = log.lock().unwrap();
    for name in values.iter().copied().chain(["extra1", "extra2", "base"]) {
        let hostile = if name == "base" {
            base_hostile
        } else {
            appended_hostile
        };
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == (name, "drop"))
                .count(),
            1,
            "{events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == (name, "payload"))
                .count(),
            usize::from(hostile),
            "{events:?}"
        );
    }
    let base = events
        .iter()
        .position(|event| *event == ("base", "drop"))
        .unwrap();
    assert!(
        events.iter().rposition(|event| event.0 != "base").unwrap() < base,
        "base must outlive appended owners and panic payloads: {events:?}"
    );
    let second = events
        .iter()
        .position(|event| *event == ("extra2", "drop"))
        .unwrap();
    let first = events
        .iter()
        .position(|event| *event == ("extra1", "drop"))
        .unwrap();
    assert!(
        second < first,
        "appended cleanup must remain reverse: {events:?}"
    );
    assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
}

#[test]
fn source_jobs_and_completions_keep_base_execution_code_through_appended_owner_cleanup() {
    for base_hostile in [false, true] {
        for appended_hostile in [false, true] {
            for run in [false, true] {
                let compiled = network(&Arc::default());
                let log = Log::default();
                let (base, declaration, owners) = execution_declaration(
                    &compiled,
                    "source",
                    &log,
                    base_hostile,
                    appended_hostile,
                );
                let source = compiled.source(&declaration).unwrap();
                let mut session = compiled.start_with_options(SessionOptions {
                    lease: Some(base),
                    auto_start_sources: false,
                    ..Default::default()
                });
                let id = session.begin_bound_request(None).unwrap();
                session.start_bound_request_source(id, &source).unwrap();
                let (_, job) = session.next_job().unwrap();
                drop((source, declaration, compiled, session));
                assert!(log.lock().unwrap().is_empty());
                assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
                if run {
                    let completion = job.run();
                    assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
                    assert!(catch_unwind(AssertUnwindSafe(|| drop(completion))).is_ok());
                } else {
                    assert!(catch_unwind(AssertUnwindSafe(|| drop(job))).is_ok());
                }
                assert_base_released_last(&log, &owners, base_hostile, appended_hostile, &[]);
            }
        }
    }
}

#[test]
fn bound_configuration_and_startup_failures_keep_base_through_all_appended_cleanup() {
    for base_hostile in [false, true] {
        for appended_hostile in [false, true] {
            for compiled_snapshot in [false, true] {
                for mode in ["success", "failure", "escaped_failure"] {
                    let calls = Arc::new(Calls::default());
                    let compiled = network(&calls);
                    let log = Log::default();
                    let (base, declaration, owners) = execution_declaration(
                        &compiled,
                        "worker",
                        &log,
                        base_hostile,
                        appended_hostile,
                    );
                    let compiled = if compiled_snapshot {
                        compiled
                            .with_lease(owners[1].upgrade().unwrap())
                            .with_lease(owners[2].upgrade().unwrap())
                    } else {
                        compiled
                    };
                    let receiving = if mode == "success" {
                        compiled.clone()
                    } else {
                        network(&calls)
                    };
                    let mut options = SessionOptions {
                        lease: Some(base),
                        auto_start_sources: false,
                        ..Default::default()
                    };
                    options.bound.readiness(
                        &declaration,
                        policy("policy", &log, &owners, &calls, appended_hostile),
                    );
                    options.bound.resources(
                        &declaration,
                        ValidatedResourceSet::try_new(vec![ResourceGrant::new(
                            "probe",
                            probe("resource", &log, &owners, appended_hostile),
                        )])
                        .unwrap(),
                    );
                    drop(declaration);
                    let session = receiving.start_with_options(options);
                    drop((receiving, compiled));
                    if mode == "success" {
                        assert!(session.failure().is_none());
                    } else {
                        calls.assert_zero();
                        assert_eq!(session.state(), SessionState::Failed);
                        assert_eq!(
                            session
                                .failure()
                                .unwrap()
                                .source()
                                .unwrap()
                                .downcast_ref::<NetworkError>(),
                            Some(&NetworkError::ForeignScope)
                        );
                    }
                    let escaped =
                        (mode == "escaped_failure").then(|| session.failure().unwrap().clone());
                    assert!(catch_unwind(AssertUnwindSafe(|| drop(session))).is_ok());
                    if let Some(failure) = escaped {
                        assert!(owners.iter().all(|owner| owner.upgrade().is_some()));
                        assert!(catch_unwind(AssertUnwindSafe(|| drop(failure))).is_ok());
                    }
                    assert_base_released_last(
                        &log,
                        &owners,
                        base_hostile,
                        appended_hostile,
                        &["policy", "resource"],
                    );
                }
            }
        }
    }
}

#[test]
fn abandoned_bound_options_keep_session_base_alive_during_appended_cleanup() {
    for appended_hostile in [false, true] {
        let calls = Arc::new(Calls::default());
        let compiled = network(&calls);
        let log = Log::default();
        // Before start, SessionOptions itself still holds the ordinary base lease.
        let (base, declaration, owners) =
            execution_declaration(&compiled, "worker", &log, false, appended_hostile);
        let mut options = SessionOptions {
            lease: Some(base),
            ..Default::default()
        };
        options.bound.readiness(
            &declaration,
            policy("policy", &log, &owners, &calls, appended_hostile),
        );
        options.bound.resources(
            &declaration,
            ValidatedResourceSet::try_new(vec![ResourceGrant::new(
                "probe",
                probe("resource", &log, &owners, appended_hostile),
            )])
            .unwrap(),
        );
        drop((declaration, compiled));
        assert!(catch_unwind(AssertUnwindSafe(|| drop(options))).is_ok());
        calls.assert_zero();
        assert_base_released_last(
            &log,
            &owners,
            false,
            appended_hostile,
            &["policy", "resource"],
        );
    }
}
