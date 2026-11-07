//! Staged plugin publication and pinned generation lifetimes.
//! This test mocks a graph compile boundary; the real graph planner is
//! intentionally NOT reimplemented by the generic plugin manager.
use phenix_contract::{Code, Failure};
use phenix_plugin::{Contribution, ContributionHandle, PluginCatalog, PluginInstance, Registrar};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct GraphPatch {
    id: String,
    version: i32,
}
impl Contribution for GraphPatch {
    const KIND: &'static str = "graph.patch";
    fn id(&self) -> &str {
        &self.id
    }
}
struct Example {
    version: i32,
    drops: Arc<AtomicUsize>,
}
impl Drop for Example {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl PluginInstance for Example {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(GraphPatch {
            id: "model".into(),
            version: self.version,
        })
    }
}
struct GraphGeneration {
    action: ContributionHandle<GraphPatch>,
}
impl GraphGeneration {
    fn compile(snapshot: &phenix_plugin::PluginSnapshot) -> Result<Self, Failure> {
        let action = snapshot
            .get::<GraphPatch>("model")
            .ok_or_else(|| Failure::new(Code::MissingBinding, "model action absent"))?;
        if action.version < 0 {
            return Err(Failure::new(
                Code::SchemaRejected,
                "invalid candidate graph",
            ));
        }
        Ok(Self { action })
    }
    fn run(&self) -> i32 {
        self.action.version
    }
}

#[test]
fn failed_compile_does_not_change_plugin_or_executable_generation() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut catalog = PluginCatalog::new();
    let counts = drops.clone();
    let key = catalog
        .register("model", move |version: i32| {
            Ok(Example {
                version,
                drops: counts.clone(),
            })
        })
        .unwrap();
    let mut plugins = catalog.instance();
    let old = plugins.load(&key, 7).unwrap();
    let generation = GraphGeneration::compile(&old).unwrap();
    drop(old);
    let failed = plugins.stage_reload(&key, -1).unwrap();
    assert!(GraphGeneration::compile(&failed.snapshot()).is_err());
    drop(failed);
    assert_eq!(plugins.snapshot(&key).unwrap().revision(), 1);
    assert_eq!(generation.run(), 7);
    assert_eq!(drops.load(Ordering::SeqCst), 1);

    let prepared = plugins.stage_reload(&key, 13).unwrap();
    let new_generation = GraphGeneration::compile(&prepared.snapshot()).unwrap();
    let selected = plugins.commit(prepared).unwrap();
    assert_eq!(selected.revision(), 2);
    plugins.unload(&key).unwrap();
    assert_eq!(generation.run(), 7);
    assert_eq!(new_generation.run(), 13);
    drop(selected);
    drop(generation);
    drop(new_generation);
    assert_eq!(drops.load(Ordering::SeqCst), 3);
}

#[test]
fn competing_candidates_and_unload_reload_cannot_resurrect_stale_code() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("model", |v: i32| {
            Ok(Example {
                version: v,
                drops: Arc::new(AtomicUsize::new(0)),
            })
        })
        .unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&key, 1).unwrap();
    let candidate_a = plugins.stage_reload(&key, 2).unwrap();
    let candidate_b = plugins.stage_reload(&key, 3).unwrap();
    assert_eq!(plugins.commit(candidate_a).unwrap().revision(), 2);
    assert!(plugins.commit(candidate_b).is_err());

    let stale = plugins.stage_reload(&key, 4).unwrap();
    plugins.unload(&key).unwrap();
    // Load a new revision after unload; revision history must not reset.
    assert_eq!(plugins.load(&key, 5).unwrap().revision(), 3);
    assert!(plugins.commit(stale).is_err());
    assert_eq!(
        plugins
            .snapshot(&key)
            .unwrap()
            .get::<GraphPatch>("model")
            .unwrap()
            .version,
        5
    );
}

#[test]
fn unrelated_plugins_can_commit_candidates_independently() {
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("a", |v: i32| {
            Ok(Example {
                version: v,
                drops: Arc::new(AtomicUsize::new(0)),
            })
        })
        .unwrap();
    let b = catalog
        .register("b", |v: i32| {
            Ok(Example {
                version: v,
                drops: Arc::new(AtomicUsize::new(0)),
            })
        })
        .unwrap();
    let mut plugins = catalog.instance();
    let staged_a = plugins.stage_load(&a, 4).unwrap();
    let staged_b = plugins.stage_load(&b, 8).unwrap();
    plugins.commit(staged_a).unwrap();
    plugins.commit(staged_b).unwrap();
    assert_eq!(plugins.snapshot(&a).unwrap().revision(), 1);
    assert_eq!(plugins.snapshot(&b).unwrap().revision(), 1);
}


// A batch containing one stale candidate must not publish even the valid ones.
#[test]
fn multi_plugin_commit_is_atomic_on_stale_candidate() {
    let mut catalog = PluginCatalog::new();
    let a = catalog.register("alpha", |version: i32| {
        Ok(Example {
            version,
            drops: Arc::new(AtomicUsize::new(0)),
        })
    }).unwrap();
    let b = catalog.register("beta", |version: i32| {
        Ok(Example {
            version,
            drops: Arc::new(AtomicUsize::new(0)),
        })
    }).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&a, 1).unwrap();
    plugins.load(&b, 10).unwrap();

    let valid = plugins.stage_reload(&a, 2).unwrap();
    let stale = plugins.stage_reload(&b, 20).unwrap();
    plugins.reload(&b, 30).unwrap();
    assert!(plugins.commit_batch(vec![valid, stale]).is_err());
    assert_eq!(plugins.snapshot(&a).unwrap().get::<GraphPatch>("model").unwrap().version, 1);
    assert_eq!(plugins.snapshot(&b).unwrap().get::<GraphPatch>("model").unwrap().version, 30);

    let valid_a = plugins.stage_reload(&a, 3).unwrap();
    let valid_b = plugins.stage_reload(&b, 40).unwrap();
    let committed = plugins.commit_batch(vec![valid_a, valid_b]).unwrap();
    assert_eq!(committed.iter().map(|s|s.revision()).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(plugins.snapshot(&a).unwrap().get::<GraphPatch>("model").unwrap().version, 3);
    assert_eq!(plugins.snapshot(&b).unwrap().get::<GraphPatch>("model").unwrap().version, 40);
}

#[test]
fn a_staged_candidate_is_bound_to_one_kernel_plugin_set() {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("model", |version: i32| {
        Ok(Example {
            version,
            drops: Arc::new(AtomicUsize::new(0)),
        })
    }).unwrap();
    let mut a = catalog.instance();
    let mut b = catalog.instance();
    let staged_a = a.stage_load(&key, 10).unwrap();
    assert!(b.commit(staged_a).is_err());
    assert!(b.snapshot(&key).is_err());
    assert!(a.snapshot(&key).is_err());
    // No state leaked between independently configured kernel instances.
    assert_eq!(a.load(&key, 11).unwrap().get::<GraphPatch>("model").unwrap().version, 11);
    assert_eq!(b.load(&key, 12).unwrap().get::<GraphPatch>("model").unwrap().version, 12);
}
