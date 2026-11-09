//! Graph-native, snapshot-based derived values. No cache or independent scheduler.
//!
//! Declare nodes in ordinary kernel contributions. [`DerivedRuntime::build`]
//! calls `Kernel::build` and binds queries by exact action identity in that final
//! deployment, including declarations composed outside the runtime builder.
//! Binding also requires the named refresh route to take one payload and target
//! only that root's refresh inlet. A parent module must explicitly export and
//! bind this route; a missing, redirected or fan-out route fails before any wave.
//! Reusing an action in multiple physical nodes is ambiguous and cannot be bound.
//! Deliberately redeploying the same compiled graph is supported; each bound query
//! borrows its owning engine. Queries are host-only: dispatch of any node through
//! a DerivedRuntime rejects nested queries, including queries of another runtime.
//! KernelHost generations use the same binding via [`QuerySpec::bind_engine`]
//! inside `Generation::with_kernel`, and must use [`DerivedRuntime::guard_executor`]
//! when preparing to enforce this rule on delegated execution threads too.
//! Stored and derived inputs cannot be mixed in a tuple: explicitly declare one
//! [`GraphDeriveExt::sources`] group and reuse it with `derive_from` instead.
//! All values are local Rust types with `Clone + Send + Sync + 'static` bounds.
//! Arity 1-4 is supported. Field/method-based closures may need explicit argument
//! annotations; the canonical arithmetic `|a, b| a + b` infers its types.
//!
//! ```ignore
//! // Inside Contribution::compose, with previously authorized cell handles:
//! let sum = graph.derive("sum", (&a, &b), |a, b| a + b)?;
//! let twice = graph.derive("twice", (&sum,), |sum| sum * 2)?;
//! let spec = graph.query_spec("sum.query", &sum)?;
//! // Store both sum and spec in plugin-owned state for the host.
//! let observation = sum.observation(16);
//! // Declare observation.route() in Contribution::routes, and call
//! // observation.activate() only in Contribution::activate.
//!
//! // Host: this calls the ordinary Kernel::build, including compile/preflight.
//! let mut runtime = DerivedRuntime::build("app", snapshots, executor)?;
//! runtime.start()?;
//! let query = runtime.bind_query(&spec)?;
//! let current = query.query()?; // Kernel::fire, one demand wave, full snapshot
//! ```
//!
//! Only same-mount atomic snapshots are accepted. Unrelated roots may perform
//! redundant reads; there is no automatic regrouping, memoization, materialization,
//! stale `latest()`, or dynamic dependency tracking. A query refreshes every sibling
//! on its root, including ordinary downstream effects, not just its selected node.
//! Native source errors cross the graph boundary as `source[MissingInput]` or
//! `source[Storage]` plus the native diagnostic (including `Codec`/`Denied`). Host
//! construction and activation preserve the typed storage error in [`Error::Storage`].
//!
//! Mixing stored and derived inputs requires an explicit unified source group:
//! ```compile_fail
//! use phenix_graph::GraphBuilder;
//! use phenix_storage::{scoped::{ReadCell, Stored}};
//! use phenix_storage_graph::derived::{DerivedRef, GraphDeriveExt};
//! fn mixed<T: Stored + Clone + Send + Sync + 'static>(
//!     graph: &mut GraphBuilder, a: &ReadCell<T>, b: &DerivedRef<T>
//! ) where T::Format: Send + Sync {
//!     graph.derive("mixed", (a, b), |_: T, _: T| ()).unwrap();
//! }
//! ```
//!
//! Derived outputs retain their Rust type when wired to ordinary consumers:
//! ```compile_fail
//! use phenix_graph::GraphBuilder;
//! use phenix_storage_graph::derived::DerivedRef;
//! fn wrong_consumer(graph: &mut GraphBuilder, value: &DerivedRef<i64>) {
//!     let text = graph.node("text", |value: String| value).unwrap();
//!     graph.connect(value.output(), text.inlet()).unwrap();
//! }
//! ```
//!
//! Handles are immutable outputs, created after their inputs, so a pure derived
//! cycle is not constructible. General graph edges remain unrestricted.
//! ```compile_fail
//! use phenix_graph::GraphBuilder;
//! use phenix_storage_graph::derived::GraphDeriveExt;
//! let mut graph = GraphBuilder::new("cycle");
//! let cycle = graph.derive("cycle", (&cycle,), |x: i64| x + 1);
//! ```
mod inputs;
mod producer;
mod query;
mod source;
pub use inputs::{CellInput, DeriveInputs, StoredInputs, TupleFunction};
pub use producer::Observation;
pub use query::{DerivedRuntime, QueryHandle, QuerySpec};

use phenix_graph::{GraphBuilder, OutputRef};
use phenix_storage::snapshot::ReadSource;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum RefreshCause {
    Demand,
    Invalidated(phenix_storage::Notice),
    Resynchronize,
}

#[derive(Debug)]
pub enum Error {
    Graph(String),
    Storage(phenix_storage::Error),
    MultipleRefreshRoots,
    WrongGeneration,
    InvalidRefreshRoute(String),
    NestedQuery,
    PublicationCount(usize),
    InvalidPublication,
    Wave(Vec<String>),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::Graph(e)
    }
}
impl From<phenix_storage::Error> for Error {
    fn from(e: phenix_storage::Error) -> Self {
        Self::Storage(e)
    }
}

pub(crate) struct Root {
    trigger: String,
    sources: Vec<ReadSource>,
    action: Arc<dyn phenix_graph::NodeAction>,
}

/// An authoring endpoint and refresh lineage, never a computed value cache.
#[derive(Clone)]
pub struct DerivedRef<T: Clone + Send + Sync + 'static> {
    pub(crate) output: OutputRef<T>,
    pub(crate) root: Arc<Root>,
}
impl<T: Clone + Send + Sync + 'static> DerivedRef<T> {
    pub fn output(&self) -> OutputRef<T> {
        self.output.clone()
    }
    pub fn refresh_route(&self) -> &str {
        &self.root.trigger
    }
    pub fn observation(&self, capacity: usize) -> Observation {
        Observation::new(self.root.clone(), capacity)
    }
}

pub trait GraphDeriveExt {
    fn sources<I: StoredInputs>(
        &mut self,
        name: &str,
        inputs: I,
    ) -> Result<DerivedRef<I::Values>, Error>;
    fn derive<I, F, O, M>(
        &mut self,
        name: &str,
        inputs: I,
        function: F,
    ) -> Result<DerivedRef<O>, Error>
    where
        I: DeriveInputs<F, O, M>,
        O: Clone + Send + Sync + 'static;
    fn derive_from<V, F, O>(
        &mut self,
        name: &str,
        group: &DerivedRef<V>,
        function: F,
    ) -> Result<DerivedRef<O>, Error>
    where
        V: Clone + Send + Sync + 'static,
        F: TupleFunction<V, O>,
        O: Clone + Send + Sync + 'static;
    fn query_spec<T: Clone + Send + Sync + 'static>(
        &mut self,
        name: &str,
        value: &DerivedRef<T>,
    ) -> Result<QuerySpec<T>, Error>;
}

// Never replace the original with a clone: that would invalidate user NodeHandle
// weak configuration references. Validate deterministically, then replay additions.
pub(crate) fn staged<T>(
    graph: &mut GraphBuilder,
    mut edit: impl FnMut(&mut GraphBuilder) -> Result<T, Error>,
) -> Result<T, Error> {
    edit(&mut graph.clone())?;
    edit(graph)
}

impl GraphDeriveExt for GraphBuilder {
    fn sources<I: StoredInputs>(
        &mut self,
        name: &str,
        inputs: I,
    ) -> Result<DerivedRef<I::Values>, Error> {
        let inputs = inputs.owned();
        let sources = I::sources(&inputs);
        let reader = Arc::new(phenix_storage::snapshot::SnapshotReader::new(
            sources.clone(),
            phenix_storage::snapshot::ReadConsistency::AtomicWithinMount,
        )?);
        let root = Arc::new(Root {
            trigger: format!("{name}.refresh"),
            sources,
            action: source::action::<I>(inputs, reader),
        });
        staged(self, |g| source::lower::<I>(g, name, root.clone()))
    }
    fn derive<I, F, O, M>(
        &mut self,
        name: &str,
        inputs: I,
        function: F,
    ) -> Result<DerivedRef<O>, Error>
    where
        I: DeriveInputs<F, O, M>,
        O: Clone + Send + Sync + 'static,
    {
        inputs.lower(self, name, function)
    }
    fn derive_from<V, F, O>(
        &mut self,
        name: &str,
        group: &DerivedRef<V>,
        function: F,
    ) -> Result<DerivedRef<O>, Error>
    where
        V: Clone + Send + Sync + 'static,
        F: TupleFunction<V, O>,
        O: Clone + Send + Sync + 'static,
    {
        let function = Arc::new(function);
        staged(self, |g| {
            let function = function.clone();
            let node = g
                .node(name, move |values: V| function.call(values))
                .map_err(|e| Error::Graph(e.to_string()))?;
            g.connect(group.output(), node.inlet())?;
            Ok(DerivedRef {
                output: node.output(),
                root: group.root.clone(),
            })
        })
    }
    fn query_spec<T: Clone + Send + Sync + 'static>(
        &mut self,
        name: &str,
        value: &DerivedRef<T>,
    ) -> Result<QuerySpec<T>, Error> {
        query::lower(self, name, value)
    }
}
