//! Snapshot-derived values authored exclusively as native FBP components.
//! Build and binding perform no reads. Observation activation subscribes before
//! bootstrap; demand queries use bounded native request scopes, never a scheduler
//! or a cached latest value. All members of a derived join share one source root.
//!
//! Authoring additions are transactional. Failed demand requests restart only
//! affected processes; cancellation does not undo already committed effects.
//!
//! Stored and derived inputs cannot be mixed implicitly:
//! ```compile_fail
//! use phenix_graph::network::NetworkBuilder;
//! use phenix_storage::scoped::{ReadCell, Stored};
//! use phenix_storage_graph::derived::{DerivedRef, GraphDeriveExt};
//! fn mixed<T: Stored + Clone + Send + Sync + 'static>(
//!     graph: &mut NetworkBuilder, cell: &ReadCell<T>, value: &DerivedRef<T>,
//! ) where T::Format: Send + Sync {
//!     graph.derive("mixed", (cell, value), |_: T, _: T| ()).unwrap();
//! }
//! ```
//! Native endpoints retain Rust type identity:
//! ```compile_fail
//! use phenix_graph::network::{InputRef, NetworkBuilder};
//! use phenix_storage_graph::derived::DerivedRef;
//! fn wrong(graph: &mut NetworkBuilder, value: &DerivedRef<i64>, to: InputRef<String>) {
//!     graph.connect(value.output(), to, std::num::NonZeroUsize::new(1).unwrap()).unwrap();
//! }
//! ```
//! Owned native dispatchers cannot replay an old completion by cloning it:
//! ```compile_fail
//! use phenix_graph::process::CompletedStep;
//! fn replay(completed: &CompletedStep) -> CompletedStep { completed.clone() }
//! ```
mod host;
mod inputs;
mod producer;
mod query;
mod source;
pub use host::{DerivedGeneration, DerivedHost};
pub use inputs::{CellInput, DeriveInputs, StoredInputs, TupleFunction};
pub use producer::Observation;
pub use query::{DerivedRuntime, QueryHandle, QuerySpec, QueryValue};

use phenix_graph::{
    component::{ComponentDef, DefinitionKey},
    network::{ComponentRef, NetworkBuilder, NetworkError, OutputRef},
};
use phenix_storage::snapshot::ReadSource;
use std::{num::NonZeroUsize, sync::Arc};

pub(crate) fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

#[derive(Clone, Debug)]
pub enum RefreshCause {
    Demand,
    Invalidated(phenix_storage::Notice),
    Resynchronize,
}

#[derive(Debug)]
pub enum Error {
    Graph(String),
    Kernel(phenix_kernel::CallbackError),
    Storage(phenix_storage::Error),
    MultipleRefreshRoots,
    WrongGeneration,
    InvalidRefreshRoute(String),
    NestedQuery,
    PublicationCount(usize),
    InvalidPublication,
    Request(phenix_graph::runtime::RequestError),
    Unsettled,
    RequestInterrupted {
        cause: Box<Error>,
        cancelled_jobs: Vec<phenix_graph::process::StepTicket>,
        disposal: phenix_graph::runtime::RequestDisposal,
    },
    Wave(Vec<String>),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Kernel(e) => Some(e.as_ref()),
            Self::Storage(e) => Some(e),
            Self::Request(e) => Some(e),
            Self::RequestInterrupted { cause, .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}
impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::Graph(e)
    }
}
impl From<NetworkError> for Error {
    fn from(e: NetworkError) -> Self {
        Self::Graph(e.to_string())
    }
}
impl From<phenix_storage::Error> for Error {
    fn from(e: phenix_storage::Error) -> Self {
        Self::Storage(e)
    }
}
impl From<phenix_graph::runtime::RequestError> for Error {
    fn from(e: phenix_graph::runtime::RequestError) -> Self {
        Self::Request(e)
    }
}

pub(crate) struct Root {
    trigger: String,
    sources: Vec<ReadSource>,
    definition: Arc<ComponentDef>,
    capture: String,
    errors: String,
}

/// Typed native endpoint and source lineage, not a value cache.
#[derive(Clone)]
pub struct DerivedRef<T: Clone + Send + Sync + 'static> {
    pub(crate) output: OutputRef<T>,
    pub(crate) component: ComponentRef,
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
    pub(crate) fn validate(&self, graph: &NetworkBuilder) -> Result<(), Error> {
        graph.inspect(&self.component)?;
        Ok(())
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

impl GraphDeriveExt for NetworkBuilder {
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
            definition: source::definition::<I>(inputs, reader)?,
            capture: format!("{name}.snapshot"),
            errors: format!("{name}.errors"),
        });
        self.additions(|graph| source::lower::<I>(graph, name, root))
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
        self.additions(|graph| inputs.lower(graph, name, function))
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
        self.additions(|graph| {
            group.validate(graph)?;
            let definition = source::transform(move |v: V| function.call(v))?;
            let component = graph.add(name, definition)?;
            graph.connect(group.output(), component.input("run")?, capacity())?;
            Ok(DerivedRef {
                output: component.output("value")?,
                component,
                root: group.root.clone(),
            })
        })
    }
    fn query_spec<T: Clone + Send + Sync + 'static>(
        &mut self,
        name: &str,
        value: &DerivedRef<T>,
    ) -> Result<QuerySpec<T>, Error> {
        self.additions(|graph| query::lower(graph, name, value))
    }
}
