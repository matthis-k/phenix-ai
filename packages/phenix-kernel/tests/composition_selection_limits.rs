mod support;

use phenix_graph::{
    compose::{ComposeError, SelectionLimits},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, ContractDefinition, Contribution, InputEvent,
    Kernel, Key, Producer, ResolutionError,
};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Artifact(&'static str, Arc<Mutex<Vec<&'static str>>>);
impl Drop for Artifact {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(self.0);
    }
}

struct Idle;
impl Producer for Idle {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        Ok(None)
    }
}

#[derive(Default)]
struct Plugin {
    self_import: bool,
    prepared: Arc<AtomicUsize>,
    activated: Arc<AtomicUsize>,
    dropped: Arc<Mutex<Vec<&'static str>>>,
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let mut contribution = Contribution::new(
            "main",
            support::patch("unit", false, true, 1, "test/service@1"),
        );
        if self.self_import {
            let contract = Key::new(registrar.owner(), "service");
            registrar.add(ContractDefinition {
                id: "service".into(),
                contract: support::contract("test/service@1"),
            })?;
            contribution = contribution
                .offer("offer", "unit", contract.clone())
                .import("self", contract);
        }
        let self_import = self.self_import;
        let prepared = self.prepared.clone();
        let activated = self.activated.clone();
        let dropped = self.dropped.clone();
        contribution
            .prepare(move |context| {
                prepared.fetch_add(1, Ordering::SeqCst);
                if self_import {
                    assert_eq!(context.bind_import("self")?.path(), "unit");
                }
                let mut artifacts = CandidateArtifacts::default();
                artifacts.export(Artifact("export", dropped.clone()))?;
                let factory = Artifact("factory", dropped.clone());
                let activated = activated.clone();
                artifacts.activate(move || {
                    let _ = &factory;
                    activated.fetch_add(1, Ordering::SeqCst);
                    Ok(Box::new(Idle))
                })?;
                Ok(artifacts)
            })
            .register(registrar)
    }
}

fn assert_budget_error(error: &ResolutionError, limit: usize) {
    assert!(
        matches!(error, ResolutionError::Composition { contribution, .. }
            if *contribution == Key::new("plugin", "main")),
        "{error:?}"
    );
    assert_eq!(
        error.source().unwrap().downcast_ref::<ComposeError>(),
        Some(&ComposeError::SelectionBudgetExceeded { limit }),
    );
}

#[test]
fn main_selection_limits_preserve_typed_errors_and_release_prepared_artifacts() {
    for max_work in [0, 1] {
        let plugin = Plugin::default();
        let prepared = plugin.prepared.clone();
        let activated = plugin.activated.clone();
        let dropped = plugin.dropped.clone();
        let mut options = BuildOptions {
            selection_limits: SelectionLimits { max_work },
            ..Default::default()
        };
        support::grant(
            &mut options,
            Key::new("plugin", "main"),
            support::patch("unit", false, true, 1, "test/service@1").footprints(),
        );
        let error = Kernel::build_with_options(
            "main-limit",
            [support::snapshot("plugin", plugin)],
            options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
        )
        .err()
        .expect("selection must exhaust the host budget");
        assert_budget_error(&error, max_work);
        assert_eq!(prepared.load(Ordering::SeqCst), 1);
        assert_eq!(activated.load(Ordering::SeqCst), 0);
        let mut dropped = dropped.lock().unwrap().clone();
        dropped.sort_unstable();
        assert_eq!(dropped, ["export", "factory"]);
    }
}

#[test]
fn zero_budget_self_import_preview_fails_before_preparation() {
    let plugin = Plugin {
        self_import: true,
        ..Default::default()
    };
    let prepared = plugin.prepared.clone();
    let activated = plugin.activated.clone();
    let dropped = plugin.dropped.clone();
    let mut options = BuildOptions {
        selection_limits: SelectionLimits { max_work: 0 },
        ..Default::default()
    };
    support::grant(
        &mut options,
        Key::new("plugin", "main"),
        support::patch("unit", false, true, 1, "test/service@1").footprints(),
    );
    let error = Kernel::build_with_options(
        "preview-limit",
        [support::snapshot("plugin", plugin)],
        options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
    )
    .err()
    .expect("self-import preview must exhaust the host budget");
    assert_budget_error(&error, 0);
    assert_eq!(prepared.load(Ordering::SeqCst), 0);
    assert_eq!(activated.load(Ordering::SeqCst), 0);
    assert!(dropped.lock().unwrap().is_empty());
}

#[test]
fn default_limits_preserve_normal_and_self_import_builds() {
    assert_eq!(BuildOptions::default().selection_limits.max_work, 1_000_000);
    for self_import in [false, true] {
        let plugin = Plugin {
            self_import,
            ..Default::default()
        };
        let prepared = plugin.prepared.clone();
        let activated = plugin.activated.clone();
        let dropped = plugin.dropped.clone();
        let mut options = BuildOptions::default();
        support::grant(
            &mut options,
            Key::new("plugin", "main"),
            support::patch("unit", false, true, 1, "test/service@1").footprints(),
        );
        let mut kernel = Kernel::build_with_options(
            "default-limit",
            [support::snapshot("plugin", plugin)],
            options.with_dispatcher(Arc::new(LocalProcessDispatcher)),
        )
        .unwrap();
        assert_eq!(prepared.load(Ordering::SeqCst), 1);
        assert_eq!(activated.load(Ordering::SeqCst), 0);
        assert!(dropped.lock().unwrap().is_empty());
        assert!(
            kernel
                .export::<Artifact>(&Key::new("plugin", "main"))
                .is_some()
        );
        kernel.start().unwrap();
        assert_eq!(activated.load(Ordering::SeqCst), 1);
        drop(kernel);
        let mut dropped = dropped.lock().unwrap().clone();
        dropped.sort_unstable();
        assert_eq!(dropped, ["export", "factory"]);
    }
}
