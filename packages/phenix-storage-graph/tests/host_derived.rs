mod support;
use phenix_graph::{
    network::NetworkBuilder,
    process::{
        ActivationJob, DispatchResult, LocalProcessDispatcher, ProcessDispatcher,
        ProcessExecutionGuard,
    },
};
use phenix_kernel::{CandidateArtifacts, Key, host::LifecycleError};
use phenix_plugin::PluginCatalog;
use phenix_storage_graph::derived::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;

fn reader(
    f: &Fixture,
    starts: Arc<AtomicUsize>,
    nested: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Plugin {
    let (base, _) = sum(f, true);
    let (a, b) = (f.a.clone(), f.b.clone());
    Plugin {
        author: Arc::new(move || {
            let mut g = NetworkBuilder::new();
            let nested = nested.clone();
            let sum = g.derive("sum", (&a, &b), move |a: Number, b: Number| {
                if let Some(nested) = &nested {
                    nested();
                }
                a.0 + b.0
            })?;
            let spec = g.query_spec("query", &sum)?;
            let observation = sum.observation(1);
            let mut artifacts = CandidateArtifacts {
                patch: Some(patch(g.build()?)),
                ..Default::default()
            };
            artifacts.export(spec)?;
            let starts = starts.clone();
            artifacts.activate(move || {
                starts.fetch_add(1, Ordering::SeqCst);
                observation.activate().map_err(Into::into)
            })?;
            Ok(artifacts)
        }),
        ..base
    }
}

#[test]
fn host_binding_precedes_activation_failed_candidate_is_atomic_and_old_generations_stay_pinned() {
    let first = Fixture::new(true);
    first.put(&first.a, 3);
    first.put(&first.b, 5);
    let second = Fixture::new(true);
    second.put(&second.a, 10);
    second.put(&second.b, 20);
    let starts = Arc::new(AtomicUsize::new(0));
    let old_plugin = reader(&first, starts.clone(), None);
    let config = options(&old_plugin);
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
    let mut host = DerivedHost::new(catalog.instance());
    let key = Key::new("domain", "derive");
    let mut old_spec = None;
    let candidate = host
        .prepare(
            vec![host.plugins().prepare_load(&kind, old_plugin).unwrap()],
            phenix_kernel::BuildOptions {
                composition: config.composition.clone(),
                ..Default::default()
            },
            Arc::new(LocalProcessDispatcher),
            "old",
            |binding| {
                assert_eq!(starts.load(Ordering::SeqCst), 0);
                let spec = binding.export::<QuerySpec<i64>>(&key).unwrap();
                spec.validate_binding(binding)?;
                old_spec = Some(spec.clone());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let old = host.publish(candidate).unwrap();
    let old_spec = old_spec.unwrap();
    assert_eq!(*old.query(&old_spec).unwrap(), 8);
    let failed = host.prepare(
        vec![
            host.plugins()
                .prepare_reload(&kind, reader(&second, starts.clone(), None))
                .unwrap(),
        ],
        phenix_kernel::BuildOptions {
            composition: config.composition.clone(),
            ..Default::default()
        },
        Arc::new(LocalProcessDispatcher),
        "failed",
        |_| Err("binding rejected".into()),
    );
    assert!(matches!(failed, Err(LifecycleError::Binding(_))));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(host.plugins().snapshot(&kind).unwrap().revision(), 1);
    let mut new_spec = None;
    let candidate = host
        .prepare(
            vec![
                host.plugins()
                    .prepare_reload(&kind, reader(&second, starts.clone(), None))
                    .unwrap(),
            ],
            config,
            Arc::new(LocalProcessDispatcher),
            "new",
            |binding| {
                let spec = binding.export::<QuerySpec<i64>>(&key).unwrap();
                spec.validate_binding(binding)?;
                assert!(old_spec.validate_binding(binding).is_err());
                new_spec = Some(spec.clone());
                Ok(())
            },
        )
        .unwrap();
    let new = host.publish(candidate).unwrap();
    let new_spec = new_spec.unwrap();
    assert_eq!(*new.query(&new_spec).unwrap(), 30);
    assert!(new.query(&old_spec).is_err());
    assert!(old.query(&new_spec).is_err());
    assert!(old.drain(10).unwrap().is_empty());
    assert_eq!(*old.query(&old_spec).unwrap(), 8);
    first.put(&first.b, 7);
    assert_eq!(*old.query(&old_spec).unwrap(), 10);
    drop(host);
    assert_eq!(*new.query(&new_spec).unwrap(), 30);
}

struct Offload;
impl ProcessDispatcher for Offload {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        DispatchResult::Completed(std::thread::spawn(move || job.run()).join().unwrap())
    }
}
struct ApplicationGuard(Arc<AtomicUsize>);
impl ProcessExecutionGuard for ApplicationGuard {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(()))
    }
}

#[test]
fn host_guard_runs_on_actual_local_and_offloaded_workers_and_preserves_application_guard() {
    for offload in [false, true] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (target, spec) = sum(&f, false);
        let mut target = local(target);
        target.start().unwrap();
        let target = Arc::new(target);
        let nested = target.clone();
        let nested_spec = spec.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let plugin = reader(
            &f,
            Arc::default(),
            Some(Arc::new(move || {
                assert!(matches!(
                    nested.bind_query(&nested_spec).unwrap().query(),
                    Err(Error::NestedQuery)
                ));
                observed.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let mut options = options(&plugin);
        let entered = Arc::new(AtomicUsize::new(0));
        options.execution.execution_guard = Some(Arc::new(ApplicationGuard(entered.clone())));
        options.execution.capture_capacity = capacity();
        options.execution.max_requests = capacity();
        let mut catalog = PluginCatalog::new();
        let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
        let mut host = DerivedHost::new(catalog.instance());
        let dispatcher: Arc<dyn ProcessDispatcher> = if offload {
            Arc::new(Offload)
        } else {
            Arc::new(LocalProcessDispatcher)
        };
        let candidate = host
            .prepare(
                vec![host.plugins().prepare_load(&kind, plugin).unwrap()],
                options,
                dispatcher,
                "guarded",
                |binding| {
                    binding
                        .export::<QuerySpec<i64>>(&Key::new("domain", "derive"))
                        .unwrap()
                        .validate_binding(binding)?;
                    Ok(())
                },
            )
            .unwrap();
        let generation = host.publish(candidate).unwrap();
        let spec = generation.with_kernel(|k| {
            k.export::<QuerySpec<i64>>(&Key::new("domain", "derive"))
                .unwrap()
                .clone()
        });
        assert_eq!(*generation.query(&spec).unwrap(), 8);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(entered.load(Ordering::SeqCst) >= 6);
        generation.drain(1).unwrap();
        generation.run_steps(128).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            *target
                .bind_query(&nested_spec_for(&target))
                .unwrap()
                .query()
                .unwrap(),
            8
        );
    }
}
fn nested_spec_for(runtime: &DerivedRuntime) -> QuerySpec<i64> {
    runtime
        .kernel()
        .export::<QuerySpec<i64>>(&Key::new("domain", "derive"))
        .unwrap()
        .clone()
}

#[test]
fn host_passes_request_capacity_to_the_native_session() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let plugin = reader(&f, Arc::default(), None);
    let mut options = options(&plugin);
    options.execution.max_requests = capacity();
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
    let mut host = DerivedHost::new(catalog.instance());
    let candidate = host
        .prepare(
            vec![host.plugins().prepare_load(&kind, plugin).unwrap()],
            options,
            Arc::new(LocalProcessDispatcher),
            "bounded",
            |_| Ok(()),
        )
        .unwrap();
    let generation = host.publish(candidate).unwrap();
    generation
        .with_session(|session| {
            let id = session.begin_request(None).unwrap();
            assert!(matches!(
                session.begin_request(None),
                Err(phenix_graph::runtime::RequestError::Full)
            ));
            session.close_request(id).unwrap();
            session.release_request(id).unwrap();
        })
        .unwrap();
}

#[test]
fn prepare_rejects_redirected_terminal_and_missing_source_captures_before_activation() {
    use phenix_graph::{
        compose::{Edit, NetworkPatch},
        network::PortPath,
    };
    for output in ["query", "sum.sources.snapshot", "sum.sources.errors"] {
        let f = Fixture::new(true);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut plugin = reader(&f, starts.clone(), None);
        let config = options(&plugin);
        let author = plugin.author.clone();
        plugin.author = Arc::new(move || {
            let mut artifacts = author()?;
            let patch = artifacts.patch.take().unwrap();
            artifacts.patch = Some(NetworkPatch::new(patch.edits().iter().filter_map(|edit| {
                if matches!(edit, Edit::ExposeOutput { name, .. } if name == output) {
                    if output == "query" {
                        Some(Edit::ExposeOutput {
                            name: output.into(),
                            from: PortPath {
                                process: "sum".into(),
                                port: "value".into(),
                            },
                        })
                    } else {
                        None
                    }
                } else {
                    Some(edit.clone())
                }
            })));
            Ok(artifacts)
        });
        let mut catalog = PluginCatalog::new();
        let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
        let host = DerivedHost::new(catalog.instance());
        let result = host.prepare(
            vec![host.plugins().prepare_load(&kind, plugin).unwrap()],
            config,
            Arc::new(LocalProcessDispatcher),
            "invalid-capture",
            |binding| {
                let spec = binding
                    .export::<QuerySpec<i64>>(&Key::new("domain", "derive"))
                    .unwrap();
                assert!(matches!(
                    spec.validate_binding(binding),
                    Err(Error::InvalidPublication)
                ));
                spec.validate_binding(binding).map_err(Into::into)
            },
        );
        assert!(
            matches!(result, Err(LifecycleError::Binding(_))),
            "{output}"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert!(host.active().is_none());
    }
}

#[test]
fn host_queries_restart_only_the_failed_request_and_keep_committed_effects() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let effects = Arc::new(AtomicUsize::new(0));
    let committed = effects.clone();
    let plugin = reader(
        &f,
        Arc::default(),
        Some(Arc::new(move || {
            assert_ne!(
                committed.fetch_add(1, Ordering::SeqCst),
                0,
                "first demand fails"
            );
        })),
    );
    let config = options(&plugin);
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
    let mut host = DerivedHost::new(catalog.instance());
    let candidate = host
        .prepare(
            vec![host.plugins().prepare_load(&kind, plugin).unwrap()],
            config,
            Arc::new(LocalProcessDispatcher),
            "recoverable",
            |_| Ok(()),
        )
        .unwrap();
    let generation = host.publish(candidate).unwrap();
    let spec = generation.with_kernel(|k| {
        k.export::<QuerySpec<i64>>(&Key::new("domain", "derive"))
            .unwrap()
            .clone()
    });
    assert!(
        matches!(generation.query(&spec), Err(Error::Wave(e)) if e.join(" ").contains("panicked"))
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert_eq!(*generation.query(&spec).unwrap(), 8);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
}
