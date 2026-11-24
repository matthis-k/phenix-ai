#[path = "../benches/support/composition_cases.rs"]
mod cases;
#[path = "../benches/support/recovery_case.rs"]
mod recovery;
use phenix_graph::{
    compose::{ComposeError, PlanningLimits},
    process::LocalProcessDispatcher,
};
use phenix_kernel::host::{KernelHost, LifecycleError};
use phenix_kernel::{Kernel, ResolutionError};
use phenix_plugin::PluginCatalog;
use std::{
    error::Error,
    sync::{Arc, atomic::Ordering},
};

fn attempt(
    n: usize,
    previews: bool,
    limit: usize,
) -> (Result<Kernel, ResolutionError>, Arc<cases::Counts>) {
    let counts = Arc::new(cases::Counts::default());
    let snapshot = cases::snapshot(cases::Plugin {
        n,
        previews,
        dense: false,
        revision: 1,
        counts: counts.clone(),
    });
    let mut options = cases::options(n, previews, false);
    options.planning_limits = PlanningLimits { max_units: limit };
    (
        Kernel::build_with_options(
            "planning-limit",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options,
        ),
        counts,
    )
}

fn minimum(n: usize, previews: bool) -> usize {
    let (mut low, mut high) = (0, 1_000_000);
    assert!(attempt(n, previews, high).0.is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        match attempt(n, previews, middle).0 {
            Ok(_) => high = middle,
            Err(error) => {
                assert_eq!(
                    error.source().unwrap().downcast_ref::<ComposeError>(),
                    Some(&ComposeError::PlanningBudgetExceeded { limit: middle })
                );
                low = middle + 1;
            }
        }
    }
    low
}

#[test]
fn default_success_envelope_real_prefixes_previews_dense_priorities_and_rebuilds() {
    for n in [16, 32] {
        cases::build(n, false, false, |_| {});
        cases::build(n, true, false, |_| {});
        cases::build(n, false, true, |_| {});
    }
    cases::rebuild(16, true, |_| {});
    let preview32 = minimum(32, true);
    println!(
        "default success envelope: Kernel32 real-write prefixes+previews exact_planning_units={preview32}"
    );
    let (result, _) = attempt(64, true, 1_000_000);
    assert!(matches!(
        result
            .err()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<ComposeError>(),
        Some(ComposeError::PlanningBudgetExceeded { limit: 1_000_000 })
    ));
    assert!(attempt(64, true, 50_000_000).0.is_ok());
}

#[test]
fn cumulative_counter_covers_every_prefix_and_preview_with_exact_retry_and_cleanup() {
    let plain = minimum(4, false);
    let previews = minimum(4, true);
    assert!(previews > plain);
    let single = minimum(1, true);
    assert!(previews > 4 * single);
    for (preview, total) in [(false, plain), (true, previews)] {
        for limit in [0, single, total - 1] {
            let (result, counts) = attempt(4, preview, limit);
            let error = result.err().expect("whole build must exhaust");
            assert_eq!(
                error.source().unwrap().downcast_ref::<ComposeError>(),
                Some(&ComposeError::PlanningBudgetExceeded { limit })
            );
            assert_eq!(
                counts.dropped.load(Ordering::SeqCst),
                counts.prepared.load(Ordering::SeqCst)
            );
            assert_eq!(
                counts.factory_dropped.load(Ordering::SeqCst),
                counts.prepared.load(Ordering::SeqCst)
            );
            assert_eq!(counts.activated.load(Ordering::SeqCst), 0);
            if limit == 0 && preview {
                assert_eq!(counts.prepared.load(Ordering::SeqCst), 0);
            }
        }
        let (result, counts) = attempt(4, preview, total);
        drop(result.unwrap());
        assert_eq!(counts.prepared.load(Ordering::SeqCst), 4);
        assert_eq!(counts.dropped.load(Ordering::SeqCst), 4);
    }
    println!(
        "Kernel4 exact planning units: main_only={plain} with_previews={previews}; single_preview={single}"
    );
}

#[test]
fn failed_host_prepare_preserves_epoch_inventory_and_published_generation_then_retries() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("q3", |plugin: cases::Plugin| Ok(plugin))
        .unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let counts = Arc::new(cases::Counts::default());
    let plugin = |revision| cases::Plugin {
        n: 4,
        previews: true,
        dense: false,
        revision,
        counts: counts.clone(),
    };
    let stage = host.plugins().prepare_load(&key, plugin(1)).unwrap();
    let candidate = host
        .prepare(
            vec![stage],
            cases::options(4, true, false),
            Arc::new(LocalProcessDispatcher),
            "initial",
            |_| Ok(()),
        )
        .unwrap();
    let old = host.publish(candidate).unwrap();
    let epoch = old.epoch();
    let revision = host.plugins().snapshot(&key).unwrap().revision();
    let stage = host.plugins().prepare_reload(&key, plugin(2)).unwrap();
    let mut options = cases::options(4, true, false);
    options.planning_limits = PlanningLimits { max_units: 0 };
    assert!(matches!(
        host.prepare(
            vec![stage],
            options,
            Arc::new(LocalProcessDispatcher),
            "limited",
            |_| panic!("binding must not run")
        ),
        Err(LifecycleError::Build(_))
    ));
    assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
    assert_eq!(host.active().unwrap().epoch(), epoch);
    assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), revision);
    let stage = host.plugins().prepare_reload(&key, plugin(2)).unwrap();
    let candidate = host
        .prepare(
            vec![stage],
            cases::options(4, true, false),
            Arc::new(LocalProcessDispatcher),
            "retry",
            |_| Ok(()),
        )
        .unwrap();
    let current = host.publish(candidate).unwrap();
    assert_eq!(current.epoch(), epoch + 1);
    assert!(!Arc::ptr_eq(&old, &current));
    assert!(old.compiled().inspect("unit0000").is_ok());
}

#[test]
fn later_valid_batch_cannot_repair_a_rejected_kernel_prefix() {
    use phenix_graph::{
        ComponentDef, GraphOrigin, NetworkBuilder,
        compose::{Composer, Edit, NetworkPatch, Registration},
    };
    use phenix_kernel::{BuildOptions, CandidateArtifacts, Contribution, Key};
    use phenix_plugin::{Failure, PluginInstance, Registrar};
    use std::sync::atomic::AtomicUsize;
    fn patches() -> [NetworkPatch; 2] {
        [
            NetworkPatch::new([Edit::AddSource("future".into())]),
            NetworkPatch::new([Edit::Add {
                path: "future".into(),
                definition: Arc::new(ComponentDef::source(|| [1_i64].into_iter())),
            }]),
        ]
    }
    struct Prefix(Arc<AtomicUsize>);
    impl PluginInstance for Prefix {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            for (i, patch) in patches().into_iter().enumerate() {
                let called = self.0.clone();
                Contribution::new(format!("c{i}"), patch)
                    .prepare(move |_| {
                        called.fetch_add(1, Ordering::SeqCst);
                        Ok(CandidateArtifacts::default())
                    })
                    .register(registrar)?;
            }
            Ok(())
        }
    }
    let called = Arc::new(AtomicUsize::new(0));
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("prefix", |plugin: Prefix| Ok(plugin))
        .unwrap();
    let snapshot = catalog
        .instance()
        .load(&key, Prefix(called.clone()))
        .unwrap();
    let mut options = BuildOptions::default();
    for (i, patch) in patches().into_iter().enumerate() {
        options
            .composition
            .entry(Key::new("prefix", format!("c{i}")))
            .or_default()
            .grants = patch.footprints();
    }
    assert!(matches!(
        Kernel::build_with_options(
            "prefix",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options
        ),
        Err(ResolutionError::Compilation { .. })
    ));
    assert_eq!(called.load(Ordering::SeqCst), 1);
    let mut batch = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
    batch
        .register(patches().into_iter().enumerate().map(|(i, patch)| {
            Registration::new(
                i.to_string(),
                [],
                0,
                GraphOrigin::default(),
                patch.footprints(),
                patch,
            )
        }))
        .unwrap();
    assert_eq!(batch.current().compiled().processes().len(), 1);
}

#[test]
fn actual_kernel_recovers_suppressed_prerequisite_dependent_and_exact_owners() {
    for groups in [1, 4] {
        for previews in [false, true] {
            recovery::run(groups, previews, |_| {});
        }
    }
}

fn recovery_attempt(limit: usize) -> (Result<Kernel, ResolutionError>, Arc<recovery::Trace>) {
    let (snapshots, mut options, trace) = recovery::fixture(1, true);
    options.planning_limits = PlanningLimits { max_units: limit };
    (
        Kernel::build_with_options(
            "phase-limit",
            snapshots,
            Arc::new(LocalProcessDispatcher),
            options,
        ),
        trace,
    )
}

#[test]
fn later_preview_and_post_prepare_main_denials_preserve_exact_cleanup_order() {
    // Whole-build boundaries include five preceding main admissions. Both
    // failures belong to s, on opposite sides of its preparation callback.
    const PREVIEW_COMPLETE: usize = 8_714;
    const MAIN_COMPLETE: usize = 12_023;
    for (limit, prepared_count) in [
        (PREVIEW_COMPLETE - 1, 5),
        (PREVIEW_COMPLETE, 6),
        (MAIN_COMPLETE - 1, 6),
    ] {
        let (result, trace) = recovery_attempt(limit);
        let error = result.err().expect("cumulative scope must reject");
        assert!(
            matches!(&error, ResolutionError::Composition { contribution, .. } if *contribution == phenix_kernel::Key::new(recovery::owner(0, 5), "main")),
            "{error:?}"
        );
        assert_eq!(
            error.source().unwrap().downcast_ref::<ComposeError>(),
            Some(&ComposeError::PlanningBudgetExceeded { limit })
        );
        let prepared: Vec<_> = (0..prepared_count).map(|s| recovery::owner(0, s)).collect();
        assert_eq!(trace.owners("prepare"), prepared);
        assert!(trace.owners("activate").is_empty());
        assert!(trace.owners("effect").is_empty());
        assert!(trace.owners("static_effect").is_empty());
        trace.assert_released(1, &prepared);
        // A scalar planning error must not keep any owning revision/code alive.
        assert_eq!(trace.owners("revision").len(), 6);
        assert_eq!(trace.owners("code").len(), 6);
        drop(error);
    }
    for limit in [MAIN_COMPLETE, MAIN_COMPLETE + 1_000] {
        let (result, trace) = recovery_attempt(limit);
        drop(result.unwrap());
        let prepared: Vec<_> = (0..6).map(|s| recovery::owner(0, s)).collect();
        assert_eq!(trace.owners("prepare"), prepared);
        trace.assert_released(1, &prepared);
    }
}
