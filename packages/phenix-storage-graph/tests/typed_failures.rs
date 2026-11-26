use phenix_storage::AtomicReadMany;
mod support;

use phenix_graph::{
    Failure,
    process::{ActivationJob, DispatchResult, ProcessDispatcher, ProcessExecutionGuard},
};
use phenix_kernel::{DriverError, WakeHandle};
use phenix_storage_graph::derived::{Error as DerivedError, RefreshCause};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

struct NonCloneCause {
    lease: Weak<()>,
    callbacks: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl fmt::Display for NonCloneCause {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        panic!("external display")
    }
}
impl fmt::Debug for NonCloneCause {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        panic!("external debug")
    }
}
impl Error for NonCloneCause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.callbacks.fetch_add(1, Ordering::SeqCst);
        panic!("external source")
    }
}
impl Drop for NonCloneCause {
    fn drop(&mut self) {
        assert!(self.lease.upgrade().is_some());
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
struct Reject(Failure);
impl ProcessExecutionGuard for Reject {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, Failure> {
        Err(self.0.clone())
    }
}
struct Worker;
impl ProcessDispatcher for Worker {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        DispatchResult::Completed(std::thread::spawn(move || job.run()).join().unwrap())
    }
}

#[test]
fn query_and_driver_share_the_original_non_clone_cause_after_kernel_and_session_drop() {
    for queue in [
        phenix_graph::runtime::RequestQueuePolicy::SharedFifo,
        phenix_graph::runtime::RequestQueuePolicy::BoundedLanes,
    ] {
        let fixture = support::Fixture::new(false);
        fixture.put(&fixture.a, 3);
        fixture.put(&fixture.b, 5);
        let (plugin, spec) = support::sum(&fixture, false);
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let callbacks = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        // An application keeps an early clone here, so its extra dependency must
        // be explicitly attached before handing another clone to the framework.
        let original = Failure::external(
            "host summary",
            NonCloneCause {
                lease: weak.clone(),
                callbacks: callbacks.clone(),
                drops: drops.clone(),
            },
        )
        .with_lease(lease);
        let address = original
            .source()
            .unwrap()
            .downcast_ref::<NonCloneCause>()
            .unwrap() as *const NonCloneCause;
        let mut options = support::options(&plugin);
        options.execution.request_queue_policy = queue;
        options
            .execution
            .execution_guards
            .push(Arc::new(Reject(original.clone())));
        let mut runtime = support::configured(plugin, Arc::new(Worker), options);
        runtime.start().unwrap();
        let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
        let DerivedError::Execution(query_failure) = &error else {
            panic!("{error:?}");
        };
        assert!(std::ptr::eq(
            original.source().unwrap(),
            query_failure.source().unwrap()
        ));
        assert_eq!(query_failure.context().guard_index, Some(2));
        assert_eq!(
            query_failure.context().phase,
            Some(phenix_graph::CallbackPhase::GuardEnter)
        );
        let snapshot = runtime
            .kernel()
            .with_session(|session| {
                session
                    .explain_process(0, std::time::Instant::now())
                    .unwrap()
            })
            .unwrap();
        // An unscoped activation with the same guard makes this native session
        // terminal, exercising the driver's retained Failure source as well.
        let route = runtime.kernel().compiled().inputs().keys().next().unwrap();
        runtime
            .kernel()
            .inject(route, phenix_graph::Value::local(RefreshCause::Demand))
            .unwrap();
        runtime.kernel().run_steps(32).unwrap();
        let driver = runtime
            .kernel()
            .run_until(&WakeHandle::default(), |_| {})
            .unwrap_err();
        let DriverError::SessionStopped {
            cause: Some(driver_failure),
            ..
        } = &driver
        else {
            panic!("{driver:?}");
        };
        assert!(std::ptr::eq(
            query_failure.source().unwrap(),
            driver_failure.source().unwrap()
        ));
        for _ in 0..4 {
            let _ = format!("{error:?} {driver:?} {snapshot:#?}");
        }
        let escaped = query_failure.clone();
        drop(original);
        drop(runtime);
        drop(spec);
        drop(fixture);
        drop(error);
        assert!(weak.upgrade().is_some());
        assert_eq!(
            escaped
                .source()
                .unwrap()
                .downcast_ref::<NonCloneCause>()
                .unwrap() as *const NonCloneCause,
            address
        );
        drop(escaped);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(driver);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        assert!(
            weak.upgrade().is_none(),
            "the Q6 snapshot must not retain error resources"
        );
        let _ = format!("{snapshot:#?}");
    }
}

#[derive(Clone, Copy)]
enum StorageFault {
    Denied,
    Schema,
    Sql,
    Io,
}
fn actual_storage_error(fault: StorageFault) -> phenix_storage::Error {
    use phenix_storage::{Address, Grant, Operation, Registry, Scope};
    match fault {
        StorageFault::Denied => {
            let registry = Registry::new();
            registry.mount_memory("main").unwrap();
            let address = Address::parse("store://main/value").unwrap();
            registry
                .session([])
                .get(&address, "value", &support::Format)
                .unwrap_err()
        }
        StorageFault::Schema => {
            use phenix_storage::Codec;
            let registry = Registry::new();
            registry.mount_memory("main").unwrap();
            let address = Address::parse("store://main/value").unwrap();
            let mut schema = support::Format.schema();
            registry.register(&address, schema.clone()).unwrap();
            schema.revision += 1;
            registry
                .session([Grant::new(Scope::Mount("main".into()), [Operation::Read])])
                .get(
                    &address,
                    "value",
                    &phenix_storage::SchemaCodec {
                        codec: support::Format,
                        schema,
                    },
                )
                .unwrap_err()
        }
        StorageFault::Sql => {
            let registry = Registry::new();
            registry.mount_sqlite("main", ":memory:").unwrap();
            registry
                .session([Grant::new(
                    Scope::Mount("main".into()),
                    [Operation::SqlAdmin],
                )])
                .transaction_mount("main", |sql| {
                    sql.execute("SELECT * FROM q7_missing_table", &[])
                })
                .unwrap_err()
        }
        StorageFault::Io => {
            let error = std::fs::File::open(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/q7_missing_storage_file"),
            )
            .unwrap_err();
            phenix_storage::Error::Io(error)
        }
    }
}
struct FaultBackend {
    memory: phenix_storage::MemoryBackend,
    fault: StorageFault,
    fail: bool,
}
impl phenix_storage::Backend for FaultBackend {
    fn typed_kv(&mut self) -> Option<&mut dyn phenix_storage::TypedKv> {
        Some(self)
    }
}
impl phenix_storage::TypedKv for FaultBackend {
    fn get(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
        self.memory.get(dataset, key)
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> phenix_storage::Result<()> {
        self.memory.put(dataset, key, value)
    }
    fn delete(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<bool> {
        self.memory.delete(dataset, key)
    }
    fn atomic_read_many(&mut self) -> Option<&mut dyn AtomicReadMany> {
        Some(self)
    }
}
impl AtomicReadMany for FaultBackend {
    fn read_many(
        &mut self,
        keys: &[phenix_storage::snapshot::ReadKey<'_>],
    ) -> phenix_storage::Result<Vec<Option<phenix_storage::Revisioned<Vec<u8>>>>> {
        if std::mem::replace(&mut self.fail, false) {
            return Err(actual_storage_error(self.fault));
        }
        self.memory.read_many(keys)
    }
}
fn assert_storage_cause(failure: &Failure, fault: StorageFault) {
    assert_eq!(
        failure.context().phase,
        Some(phenix_graph::CallbackPhase::Read)
    );
    let error = failure
        .source()
        .unwrap()
        .downcast_ref::<phenix_storage::Error>()
        .unwrap();
    match fault {
        StorageFault::Denied => assert!(matches!(error, phenix_storage::Error::Denied)),
        StorageFault::Schema => assert!(matches!(error, phenix_storage::Error::SchemaMismatch)),
        StorageFault::Sql => {
            assert!(matches!(error, phenix_storage::Error::Sql(_)));
            assert!(error.source().is_some());
        }
        StorageFault::Io => {
            assert!(matches!(error, phenix_storage::Error::Io(_)));
            assert_eq!(
                error
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::NotFound
            );
        }
    }
}

#[test]
fn real_storage_denial_schema_sql_and_io_causes_survive_error_data_and_query_normalization() {
    for fault in [
        StorageFault::Denied,
        StorageFault::Schema,
        StorageFault::Sql,
        StorageFault::Io,
    ] {
        for data in [true, false] {
            let fixture = support::Fixture::with_backend(
                false,
                FaultBackend {
                    memory: Default::default(),
                    fault,
                    fail: true,
                },
            );
            fixture.put(&fixture.a, 3);
            fixture.put(&fixture.b, 5);
            let (plugin, spec) = support::sum(&fixture, false);
            let mut runtime = support::build(plugin, Arc::new(Worker));
            runtime.start().unwrap();
            let failure = if data {
                let route = runtime
                    .kernel()
                    .compiled()
                    .inputs()
                    .keys()
                    .next()
                    .unwrap()
                    .clone();
                let capture = runtime
                    .kernel()
                    .compiled()
                    .outputs()
                    .keys()
                    .find(|name| name.ends_with(".errors"))
                    .unwrap()
                    .clone();
                runtime
                    .kernel()
                    .with_session(|session| {
                        let request = session.begin_request(None).unwrap();
                        session
                            .inject_request(
                                request,
                                &route,
                                phenix_graph::Value::local(RefreshCause::Demand),
                            )
                            .unwrap();
                        session.close_request(request).unwrap();
                        session.run_until_idle(128);
                        assert!(
                            session.failure().is_none(),
                            "source ERROR remains ordinary DATA"
                        );
                        assert_ne!(
                            session.request_status(request).unwrap(),
                            phenix_graph::runtime::RequestStatus::Failed
                        );
                        let packet = session
                            .take_request_output(request, &capture)
                            .unwrap()
                            .unwrap();
                        let original = packet.value().borrow::<Failure>().unwrap();
                        let failure = Failure::from_value(packet.value()).unwrap();
                        assert!(std::ptr::eq(
                            original.source().unwrap(),
                            failure.source().unwrap()
                        ));
                        drop(packet);
                        session.release_request(request).unwrap();
                        failure
                    })
                    .unwrap()
            } else {
                let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
                let DerivedError::Execution(outer) = &error else {
                    panic!("{error:?}");
                };
                assert!(outer.context().request.is_some());
                let wave = outer
                    .source()
                    .unwrap()
                    .downcast_ref::<DerivedError>()
                    .unwrap();
                let DerivedError::Wave(members) = wave else {
                    panic!("{wave:?}");
                };
                assert_eq!(members.len(), 1);
                assert!(std::ptr::eq(
                    wave.source().unwrap().downcast_ref::<Failure>().unwrap(),
                    &members[0]
                ));
                let failure = members[0].clone();
                assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
                failure
            };
            drop(runtime);
            drop(spec);
            drop(fixture);
            assert_storage_cause(&failure, fault);
        }
    }
}
