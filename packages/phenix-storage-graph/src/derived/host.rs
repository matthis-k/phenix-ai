use super::{DerivedRuntime, Error, QuerySpec};
use phenix_graph::process::ProcessDispatcher;
use phenix_kernel::{
    BuildOptions,
    host::{Binding, Candidate, Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{PluginSet, StagedPlugin};
use std::sync::Arc;

/// Supported host integration for derived queries. The owned host always builds
/// with the store's host-only execution guard, including delegated worker steps.
/// No storage dependency is required by the kernel.
/// The application execution guard is retained and entered on the same worker.
pub struct DerivedHost {
    host: KernelHost,
}

impl DerivedHost {
    pub fn new(plugins: PluginSet) -> Self {
        Self {
            host: KernelHost::new(plugins),
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
        mut options: BuildOptions,
        dispatcher: Arc<dyn ProcessDispatcher>,
        name: impl Into<String>,
        bind: impl FnOnce(&Binding<'_>) -> Result<(), phenix_kernel::CallbackError>,
    ) -> Result<Candidate, LifecycleError> {
        DerivedRuntime::guard_options(&mut options.execution);
        self.host.prepare(stages, options, dispatcher, name, bind)
    }

    pub fn publish(&mut self, candidate: Candidate) -> Result<DerivedGeneration, LifecycleError> {
        self.host.publish(candidate).map(DerivedGeneration)
    }
}

/// A published, guarded generation. Cloning pins this exact deployment, even
/// after replacement. Ordinary generation APIs remain available through Deref;
/// explicit native session access retains the same execution guard.
#[derive(Clone)]
pub struct DerivedGeneration(Arc<Generation>);

impl std::ops::Deref for DerivedGeneration {
    type Target = Generation;
    fn deref(&self) -> &Generation {
        &self.0
    }
}

impl DerivedGeneration {
    /// Validate against this exact published network and admit one native demand
    /// request. Nested calls from guarded process execution return NestedQuery.
    pub fn query<T: Clone + Send + Sync + 'static>(
        &self,
        spec: &QuerySpec<T>,
    ) -> Result<super::QueryValue<T>, Error> {
        self.with_kernel(|kernel| spec.bind_kernel(kernel)?.query())
    }
}
