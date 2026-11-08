use super::*;
use phenix_graph::{
    Emission, InputSlot, InvocationCtx, NodeAction, NodeTemplate, OutputSlot, Port, emit_local,
};
use phenix_storage::{
    scoped::{OptionalReadCell, ReadCell, Stored},
    snapshot::{ReadSource, SnapshotEntry},
};

pub trait CellInput {
    type Value: Clone + Send + Sync + 'static;
    type Owned: Clone + Send + Sync + 'static;
    fn owned(&self) -> Self::Owned;
    fn source(value: &Self::Owned) -> ReadSource;
    fn decode(value: &Self::Owned, entry: &SnapshotEntry) -> phenix_storage::Result<Self::Value>;
}
impl<T> CellInput for &ReadCell<T>
where
    T: Stored + Clone + Send + Sync + 'static,
    T::Format: Send + Sync,
{
    type Value = T;
    type Owned = ReadCell<T>;
    fn owned(&self) -> Self::Owned {
        (*self).clone()
    }
    fn source(value: &Self::Owned) -> ReadSource {
        value.source()
    }
    fn decode(value: &Self::Owned, entry: &SnapshotEntry) -> phenix_storage::Result<T> {
        value.decode(entry).map(|v| v.value)
    }
}
impl<T> CellInput for &OptionalReadCell<T>
where
    T: Stored + Clone + Send + Sync + 'static,
    T::Format: Send + Sync,
{
    type Value = Option<T>;
    type Owned = OptionalReadCell<T>;
    fn owned(&self) -> Self::Owned {
        (*self).clone()
    }
    fn source(value: &Self::Owned) -> ReadSource {
        value.source()
    }
    fn decode(value: &Self::Owned, entry: &SnapshotEntry) -> phenix_storage::Result<Option<T>> {
        value.decode(entry).map(|v| v.map(|v| v.value))
    }
}
pub trait StoredInputs {
    type Values: Clone + Send + Sync + 'static;
    type Owned: Clone + Send + Sync + 'static;
    fn owned(&self) -> Self::Owned;
    fn sources(inputs: &Self::Owned) -> Vec<ReadSource>;
    fn decode(
        inputs: &Self::Owned,
        entries: &[SnapshotEntry],
    ) -> phenix_storage::Result<Self::Values>;
}
pub trait TupleFunction<V, O>: Send + Sync + 'static {
    fn call(&self, values: V) -> O;
}
pub struct StoredMode;
pub struct DerivedMode;
struct Join<F>(F);
impl<F> NodeAction for Join<F>
where
    F: Fn(InvocationCtx) -> Result<Vec<Emission>, String> + Send + Sync,
{
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        (self.0)(ctx)
    }
}
pub trait DeriveInputs<F, O: Clone + Send + Sync + 'static, M> {
    fn lower(
        self,
        graph: &mut GraphBuilder,
        name: &str,
        function: F,
    ) -> Result<DerivedRef<O>, Error>;
}
impl<I, F, O> DeriveInputs<F, O, StoredMode> for I
where
    I: StoredInputs,
    F: TupleFunction<I::Values, O>,
    O: Clone + Send + Sync + 'static,
{
    fn lower(
        self,
        graph: &mut GraphBuilder,
        name: &str,
        function: F,
    ) -> Result<DerivedRef<O>, Error> {
        let inputs = self.owned();
        let sources = I::sources(&inputs);
        let reader = Arc::new(phenix_storage::snapshot::SnapshotReader::new(
            sources.clone(),
            phenix_storage::snapshot::ReadConsistency::AtomicWithinMount,
        )?);
        let root = Arc::new(Root {
            trigger: format!("{name}.sources.refresh"),
            sources,
            action: source::action::<I>(inputs, reader),
        });
        let function = Arc::new(function);
        staged(graph, |g| {
            let group = source::lower::<I>(g, &format!("{name}.sources"), root.clone())?;
            let function = function.clone();
            let node = g
                .node(name, move |v: I::Values| function.call(v))
                .map_err(|e| Error::Graph(e.to_string()))?;
            g.connect(group.output(), node.inlet())?;
            Ok(DerivedRef {
                output: node.output(),
                root: root.clone(),
            })
        })
    }
}
macro_rules! tuples {
    ($($T:ident:$i:tt),+) => {
        impl<$($T: CellInput),+> StoredInputs for ($($T,)+) {
            type Values = ($($T::Value,)+);
            type Owned = ($($T::Owned,)+);
            fn owned(&self) -> Self::Owned { ($(self.$i.owned(),)+) }
            fn sources(v: &Self::Owned) -> Vec<ReadSource> { vec![$($T::source(&v.$i)),+] }
            fn decode(v: &Self::Owned, entries: &[SnapshotEntry]) -> phenix_storage::Result<Self::Values> { Ok(($($T::decode(&v.$i, &entries[$i])?,)+)) }
        }
        impl<F, O, $($T),+> TupleFunction<($($T,)+), O> for F
        where F: Fn($($T),+) -> O + Send + Sync + 'static {
            fn call(&self, values: ($($T,)+)) -> O { self($(values.$i),+) }
        }
        impl<F, O, $($T),+> DeriveInputs<F, O, DerivedMode> for ($(&DerivedRef<$T>,)+)
        where $($T: Clone + Send + Sync + 'static,)+ O: Clone + Send + Sync + 'static,
            F: Fn($($T),+) -> O + Send + Sync + 'static {
            fn lower(self, graph: &mut GraphBuilder, name: &str, function: F) -> Result<DerivedRef<O>, Error> {
                let root = self.0.root.clone();
                if $( !Arc::ptr_eq(&root, &self.$i.root) )||+ { return Err(Error::MultipleRefreshRoots); }
                let function = Arc::new(function);
                staged(graph, |g| {
                    let join = format!("{name}.join");
                    g.add(&join, NodeTemplate::with_action_factory(0,
                        vec![Port::all(0, vec![$(InputSlot::local::<$T>($i).named(stringify!($i))),+]).named("run")],
                        vec![OutputSlot::local::<($($T,)+)>(0).named("value")],
                        || Join(|mut ctx: InvocationCtx| Ok(vec![emit_local(0, ($(ctx.take_local::<$T>($i)?,)+))])),
                    )).map_err(|e| Error::Graph(e.to_string()))?;
                    $(g.connect(self.$i.output(), g.local_input::<$T>(&join, "run", stringify!($i))?)?;)+
                    let function = function.clone();
                    let node = g.node(name, move |values: ($($T,)+)| function($(values.$i),+))
                        .map_err(|e| Error::Graph(e.to_string()))?;
                    g.connect(g.local_output::<($($T,)+)>(&join, "value")?, node.inlet())?;
                    Ok(DerivedRef { output: node.output(), root: root.clone() })
                })
            }
        }
    };
}
tuples!(A:0);
tuples!(A:0, B:1);
tuples!(A:0, B:1, C:2);
tuples!(A:0, B:1, C:2, D:3);
