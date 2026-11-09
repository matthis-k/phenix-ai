use super::{DerivedRuntime, Error, QuerySpec};
use phenix_graph::Executor;
use phenix_kernel::{
    BuildOptions, WakeHandle,
    host::{Binding, Candidate, Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{PluginSet, StagedPlugin};
use std::sync::Arc;

/// Supported host integration for derived queries. The owned host always builds
/// with the store's host-only dispatch guard, including delegated worker actions.
/// No storage dependency is required by the kernel.
/// Configuration cannot install a per-node executor to bypass the guard.
///
/// ```compile_fail
/// use phenix_graph::Executor;
/// use phenix_kernel::BuildOptions;
/// use std::sync::Arc;
/// fn bypass(options: &mut BuildOptions, executor: Arc<dyn Executor>) {
///     options.execution.node_executor("reader/source", executor).unwrap();
/// }
/// ```
pub struct DerivedHost {
    host: KernelHost,
}

impl DerivedHost {
    pub fn new(plugins: PluginSet) -> Self {
        Self {
            host: KernelHost::new(plugins),
        }
    }

    /// Opt into wake-driven published generations while retaining the mandatory
    /// derived-query executor guard in prepare.
    pub fn with_wake(plugins: PluginSet, wake: WakeHandle) -> Self {
        Self {
            host: KernelHost::with_wake(plugins, wake),
        }
    }

    pub fn plugins(&self) -> &PluginSet {
        self.host.plugins()
    }

    pub fn active(&self) -> Option<DerivedGeneration> {
        self.host.active().map(DerivedGeneration)
    }

    /// Validate query specs and exports before any producer starts. Binding has
    /// no admission API. Arbitrary callback/export external effects are not
    /// transactional and remain the caller's responsibility.
    pub fn prepare(
        &self,
        stages: Vec<StagedPlugin>,
        options: BuildOptions,
        executor: Arc<dyn Executor>,
        name: impl Into<String>,
        bind: impl FnOnce(&Binding<'_>) -> Result<(), phenix_kernel::CallbackError>,
    ) -> Result<Candidate, LifecycleError> {
        self.host.prepare(
            stages,
            options,
            DerivedRuntime::guard_executor(executor),
            name,
            bind,
        )
    }

    pub fn publish(&mut self, candidate: Candidate) -> Result<DerivedGeneration, LifecycleError> {
        self.host.publish(candidate).map(DerivedGeneration)
    }
}

/// A published, guarded generation. Cloning pins this exact deployment, even
/// after replacement. Ordinary generation APIs remain available through Deref;
/// explicit low-level engine access retains its documented admission limitation.
#[derive(Clone)]
pub struct DerivedGeneration(Arc<Generation>);

impl std::ops::Deref for DerivedGeneration {
    type Target = Generation;
    fn deref(&self) -> &Generation {
        &self.0
    }
}

impl DerivedGeneration {
    /// Validate against this exact published engine and fire one normal kernel
    /// demand wave. Nested calls from guarded dispatch return NestedQuery.
    pub fn query<T: Clone + Send + Sync + 'static>(&self, spec: &QuerySpec<T>) -> Result<T, Error> {
        self.with_kernel(|kernel| {
            let mut handle = spec.bind_engine(kernel.engine())?;
            handle.kernel = Some(kernel);
            handle.query()
        })
    }
}
