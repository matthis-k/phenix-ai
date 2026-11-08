//! One-value output routing. A plain `T` broadcasts; `Output<T>` carries
//! zero or one produced payload, together with its selected destinations.
//! A missing value is different from a produced value with zero recipients.
//! No selection directly invokes another node or modifies graph topology.
use crate::authoring::Input;
use crate::{EdgeId, Emission, InputAddr, SlotId, Value};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

/// One existing outgoing connection, supplied read-only to selection predicates.
#[derive(Clone, Debug)]
pub struct Destination {
    pub edge: EdgeId,
    pub node_path: String,
    pub port: String,
    pub slot: String,
    pub(crate) input: InputAddr,
}
impl Destination {
    pub fn path(&self) -> &str {
        &self.node_path
    }
    pub fn edge(&self) -> EdgeId {
        self.edge
    }
    pub fn input(&self) -> InputAddr {
        self.input
    }
    pub fn port(&self) -> &str {
        &self.port
    }
    pub fn slot(&self) -> &str {
        &self.slot
    }
}

/// Symbolic typed destination; resolved against this output's compiled edges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DestinationTarget {
    Node {
        scope: u64,
        declaration_id: u64,
        name: String,
        port: String,
        slot: String,
    },
    Module {
        scope: u64,
        declaration_id: u64,
        name: String,
        port: String,
    },
}

#[derive(Clone)]
pub(crate) enum OutputSelection {
    All,
    Targets(Vec<DestinationTarget>),
    // The selection is carried in an emission and may cross an executor
    // boundary before it is evaluated. Keep Emission Send + Sync capable.
    Filter(Arc<dyn Fn(&Destination) -> bool + Send + Sync>),
}

/// Routing information is meaningful only when a node actually produces a value.
/// Fields are private so callers cannot forge output-routing capabilities.
#[doc(hidden)]
pub struct Produced<T: Send + Sync + 'static> {
    value: T,
    selection: OutputSelection,
    _local: PhantomData<Rc<()>>,
}

/// The result of a node activation: either no value was produced, or one
/// value was produced with zero or more selected outgoing connections.
///
/// This is deliberately different from `Option<T>`: returning `Option<T>`
/// normally broadcasts that Option *as data*. Returning `Output::none()`
/// produces no value, while `Output::to_all([], value)` produces a value
/// without delivering it to any connected node.
///
/// A typed output port always exists, even when one invocation produces no
/// payload. `Output::none()` does not require constructing a dummy `T`.
///
/// `Output` is intentionally `!Send` and lowered before the value enters
/// the engine's delivery buffers. Constructing one does not dispatch eagerly.
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use phenix_graph::Output;
/// fn run() {
///     Output::all(42_i64); // Must be returned from the node function.
/// }
/// fn main() { run(); }
/// ```
#[must_use = "return this value from the node handler; Output does not dispatch eagerly"]
pub enum Output<T: Send + Sync + 'static = ()> {
    /// The invocation succeeded without producing a value.
    None,
    /// The invocation produced a value; recipients may be empty.
    Some(Produced<T>),
}
impl<T: Send + Sync + 'static> Output<T> {
    fn new(value: T, selection: OutputSelection) -> Self {
        Self::Some(Produced {
            value,
            selection,
            _local: PhantomData,
        })
    }
    /// Produce no value. This is NOT equivalent to a value with no recipients.
    /// The generic output type is inferred from the node/return context; for
    /// a completely unconstrained closure annotate it, e.g. `Output::<()>::none()`.
    pub fn none() -> Self {
        Self::None
    }
    /// Produce a value and broadcast to every existing outgoing connection.
    pub fn all(value: T) -> Self {
        Self::new(value, OutputSelection::All)
    }
    /// Send to one typed input, provided this output is connected to it.
    ///
    /// ```compile_fail
    /// use phenix_graph::{GraphBuilder, Output};
    /// let mut graph = GraphBuilder::new("types");
    /// let sink = graph.node("sink", |x: String| x).unwrap();
    /// let target = sink.inlet();
    /// graph.node("source", move |x: i64| Output::to(&target, x)).unwrap();
    /// ```
    pub fn to(target: &Input<T>, value: T) -> Self {
        Self::new(
            value,
            OutputSelection::Targets(vec![target.dispatch_target()]),
        )
    }
    /// Send one identical value to a selection of existing connected inputs.
    pub fn to_all<'a>(targets: impl IntoIterator<Item = &'a Input<T>>, value: T) -> Self
    where
        T: 'a,
    {
        Self::new(
            value,
            OutputSelection::Targets(targets.into_iter().map(Input::dispatch_target).collect()),
        )
    }
    /// Route only to connected node paths in this prefix. A prefix matches
    /// whole segments: `workers` includes `workers/a` but not `workers-old`.
    /// Empty or slash-only prefixes match nothing; use `all` to broadcast.
    pub fn to_prefix(prefix: impl Into<String>, value: T) -> Self {
        let prefix = prefix.into().trim_end_matches('/').to_owned();
        Self::to_filter(
            move |dest| {
                !prefix.is_empty()
                    && (dest.path() == prefix
                        || dest
                            .path()
                            .strip_prefix(prefix.as_str())
                            .is_some_and(|tail| tail.starts_with('/')))
            },
            value,
        )
    }
    /// Choose among only the existing outgoing connections. The callback
    /// may capture intermediate computations. Captured state must be Send +
    /// Sync so an emission can cross an external executor completion boundary.
    pub fn to_filter(
        filter: impl Fn(&Destination) -> bool + Send + Sync + 'static,
        value: T,
    ) -> Self {
        Self::new(value, OutputSelection::Filter(Arc::new(filter)))
    }
    pub(crate) fn into_emission(self) -> Option<Emission> {
        match self {
            Self::None => None,
            Self::Some(produced) => Some(Emission {
                slot: SlotId(0),
                value: Value::local(produced.value),
                selection: Some(produced.selection),
            }),
        }
    }
}
