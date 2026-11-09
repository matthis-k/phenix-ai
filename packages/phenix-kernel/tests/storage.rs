#[path = "../examples/storage.rs"]
mod example;
use phenix_graph::Value;
use phenix_kernel::{Contribution, Kernel, Key, Producer};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{Codec, Error, Grant, Notice, Operation, Registry, Scope, Subscription};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
struct SubscriptionProbe {
    markers: Arc<Mutex<Vec<Weak<()>>>>,
    drops: Arc<AtomicUsize>,
}

struct ProbedSubscription {
    subscription: Subscription,
    _marker: Arc<()>,
    drops: Arc<AtomicUsize>,
}

impl Drop for ProbedSubscription {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl Producer for ProbedSubscription {
    fn poll(&mut self) -> Result<Option<phenix_kernel::Notice>, phenix_kernel::CallbackError> {
        let notices = self.subscription.drain()?;
        assert!(notices.len() <= 1);
        Ok(notices
            .into_iter()
            .next()
            .map(|notice| phenix_kernel::Notice {
                trigger: "observed".into(),
                values: vec![Value::local(notice)],
            }))
    }
}

struct SubscriptionPlugin {
    counter: example::Counter,
    observed: example::Observed,
    probe: SubscriptionProbe,
    fail_first: bool,
}

impl PluginInstance for SubscriptionPlugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        for later in [false, true] {
            let counter = self.counter.clone();
            let observed = self.observed.clone();
            let probe = self.probe.clone();
            let fail_first = self.fail_first;
            Contribution::new(if later { "later" } else { "subscription" }, move |context| {
                    let graph = &mut *context.graph;
                    if !later {
                        let observed = observed.clone();
                        let sink = graph.node("observe", move |notice: Notice| {
                            observed.lock().unwrap().push(notice);
                        })?;
                        let input = graph.expose_input("notice", sink.inlet())?;
                        graph.trigger("observed", [input])?;
                    }
                    let attempts = AtomicUsize::new(0);
                    let counter = counter.clone();
                    let probe = probe.clone();
                    context.activate(move || {
                        if later && fail_first && attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                            return Err("later activation failed".into());
                        }
                        let subscription = counter
                            .session
                            .subscribe(Scope::Dataset(counter.address.clone()), 1)?;
                        let marker = Arc::new(());
                        probe.markers.lock().unwrap().push(Arc::downgrade(&marker));
                        Ok(Box::new(ProbedSubscription {
                            subscription,
                            _marker: marker,
                            drops: probe.drops.clone(),
                        }))
                    })
                })
.after(if later {
                    vec![Key::new("storage", "subscription")]
                } else {
                    vec![]
                })
.route(vec!["observed".into()])
.register(registrar)?;
        }
        Ok(())
    }
}

fn subscription_lifecycle(sqlite: bool, retry: bool) {
    let registry = Registry::new();
    if sqlite {
        registry.mount_sqlite("main", ":memory:").unwrap();
    } else {
        registry.mount_memory("main").unwrap();
    }
    let address = phenix_storage::Address::parse("store://main/counters").unwrap();
    registry
        .register(&address, example::CounterCodec(1).schema())
        .unwrap();
    let counter = example::Counter {
        session: registry.session([Grant::new(
            Scope::Mount("main".into()),
            [
                Operation::Read,
                Operation::Write,
                Operation::Subscribe,
                Operation::Sql,
            ],
        )]),
        address,
        sqlite,
    };
    if sqlite {
        counter
            .session
            .transaction(&counter.address, |sql| {
                sql.execute(
                    "CREATE TABLE counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL)",
                    &[],
                )?;
                Ok(())
            })
            .unwrap();
    }
    let observed = example::Observed::default();
    let probe = SubscriptionProbe::default();
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("storage", |plugin: SubscriptionPlugin| Ok(plugin))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(
            &key,
            SubscriptionPlugin {
                counter: counter.clone(),
                observed: observed.clone(),
                probe: probe.clone(),
                fail_first: retry,
            },
        )
        .unwrap();
    let mut kernel = Kernel::build(
        "subscription-lifecycle",
        plugins.snapshots(),
        Arc::new(phenix_graph::LocalExecutor),
    )
    .unwrap();
    assert!(probe.markers.lock().unwrap().is_empty());
    assert!(kernel.fire("observed", vec![]).is_err());
    counter.write(0).unwrap();
    if retry {
        assert!(
            matches!(kernel.start().unwrap_err(), phenix_kernel::StartError::Activation { cause, .. } if cause.to_string() == "later activation failed")
        );
        assert_eq!(probe.drops.load(Ordering::SeqCst), 1);
        let markers = probe.markers.lock().unwrap();
        assert_eq!(markers.len(), 1);
        assert!(markers[0].upgrade().is_none());
        drop(markers);
        assert!(kernel.fire("observed", vec![]).is_err());
        counter.write(1).unwrap();
        assert!(kernel.drain(8).is_empty());
        assert!(observed.lock().unwrap().is_empty());
    }
    kernel.start().unwrap();
    assert!(
        kernel.drain(8).is_empty(),
        "retry must create fresh subscriptions"
    );
    assert_eq!(
        probe
            .markers
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.upgrade().is_some())
            .count(),
        2
    );
    counter.write(2).unwrap();
    let events = kernel.drain(8);
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| event.result.as_ref().unwrap().success())
    );
    assert!(matches!(
        observed.lock().unwrap().as_slice(),
        [Notice::Changed(_), Notice::Changed(_)]
    ));
    // Leave queued effects at teardown while the registry and session remain alive.
    counter.write(3).unwrap();
    drop(kernel);
    assert_eq!(
        probe.drops.load(Ordering::SeqCst),
        if retry { 3 } else { 2 }
    );
    assert!(
        probe
            .markers
            .lock()
            .unwrap()
            .iter()
            .all(|m| m.upgrade().is_none())
    );
    for value in 4..8 {
        counter.write(value).unwrap();
        assert_eq!(counter.read().unwrap(), Some(value));
    }
    assert_eq!(observed.lock().unwrap().len(), 2);
}

#[test]
fn real_subscriptions_are_released_on_kernel_drop() {
    for sqlite in [false, true] {
        subscription_lifecycle(sqlite, false);
    }
}

#[test]
fn partial_activation_releases_real_subscriptions_and_retry_succeeds() {
    for sqlite in [false, true] {
        subscription_lifecycle(sqlite, true);
    }
}

#[test]
fn both_backends_route_real_effects_into_separate_bounded_waves() {
    for sqlite in [false, true] {
        let mut demo = example::build(sqlite, 8).unwrap();
        assert!(
            demo.kernel
                .fire("write_counter", vec![Value::local(1_i64)])
                .is_err()
        );
        assert!(demo.kernel.drain(8).is_empty());
        // A write before startup must not be queued by a build-time subscription.
        demo.counter.write(0).unwrap();
        demo.kernel.start().unwrap();
        assert!(demo.kernel.drain(8).is_empty());
        assert_eq!(demo.counter.read().unwrap(), Some(0));
        assert!(
            demo.kernel
                .fire("write_counter", vec![Value::local("wrong type".to_owned())])
                .is_err()
        );
        assert!(demo.kernel.fire("missing", vec![]).is_err());
        let mut waves = Vec::new();
        for value in [17_i64, 29, -4] {
            let report = demo
                .kernel
                .fire("write_counter", vec![Value::local(value)])
                .unwrap();
            assert!(report.success(), "{:?}", report.error_messages());
            waves.push(report.wave_id());
            assert_eq!(demo.counter.read().unwrap(), Some(value));
        }
        assert_eq!(
            *demo.reads.lock().unwrap(),
            vec![Ok(Some(17)), Ok(Some(29)), Ok(Some(-4))]
        );
        assert!(demo.observed.lock().unwrap().is_empty());
        assert!(demo.kernel.drain(0).is_empty());
        for count in 1..=3 {
            let events = demo.kernel.drain(1);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].producer, Key::new("observer", "observer"));
            let report = events[0].result.as_ref().unwrap();
            assert!(report.success());
            assert!(!waves.contains(&report.wave_id()));
            waves.push(report.wave_id());
            assert_eq!(demo.observed.lock().unwrap().len(), count);
        }
        let observed = demo.observed.lock().unwrap();
        let mut last = 0;
        for notice in observed.iter() {
            let Notice::Changed(change) = notice else {
                panic!("unexpected overflow")
            };
            assert!(change.sequence > last);
            last = change.sequence;
            assert_eq!(
                change.scope,
                if sqlite {
                    Scope::Mount("main".into())
                } else {
                    Scope::Dataset(demo.counter.address.clone())
                }
            );
            assert_eq!(
                change.key.as_deref(),
                if sqlite { None } else { Some("counter") }
            );
        }
        drop(observed);
        assert!(
            demo.kernel.drain(32).is_empty(),
            "observer sink must not write or feed back"
        );
        assert_eq!(demo.counter.read().unwrap(), Some(-4));
        assert_eq!(demo.reads.lock().unwrap().len(), 3);
    }
}

#[test]
fn schema_mismatch_has_no_effect_or_notice() {
    for sqlite in [false, true] {
        let mut demo = example::build(sqlite, 8).unwrap();
        demo.kernel.start().unwrap();
        let result = demo.counter.session.put(
            &demo.counter.address,
            "counter",
            &99,
            &example::CounterCodec(2),
        );
        assert!(matches!(result, Err(Error::SchemaMismatch)));
        assert!(matches!(
            demo.counter
                .session
                .get(&demo.counter.address, "counter", &example::CounterCodec(2)),
            Err(Error::SchemaMismatch)
        ));
        assert_eq!(demo.counter.read().unwrap(), None);
        assert!(demo.kernel.drain(8).is_empty());
    }
}

#[test]
fn overflow_is_a_typed_notice_and_polling_recovers() {
    for sqlite in [false, true] {
        let mut demo = example::build(sqlite, 1).unwrap();
        demo.kernel.start().unwrap();
        demo.counter.write(1).unwrap();
        demo.counter.write(2).unwrap();
        let events = demo.kernel.drain(1);
        assert_eq!(events.len(), 1);
        assert!(events[0].result.as_ref().unwrap().success());
        assert!(matches!(
            demo.observed.lock().unwrap().as_slice(),
            [Notice::Overflow { .. }]
        ));
        demo.counter.write(3).unwrap();
        assert_eq!(demo.kernel.drain(1).len(), 1);
        assert!(matches!(
            demo.observed.lock().unwrap().last(),
            Some(Notice::Changed(_))
        ));
        assert!(demo.kernel.drain(16).is_empty());
        assert_eq!(demo.counter.read().unwrap(), Some(3));
    }
}
