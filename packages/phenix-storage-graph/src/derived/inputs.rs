use super::*;
use phenix_graph::{PortDef, Value};
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
pub trait DeriveInputs<F, O: Clone + Send + Sync + 'static, M> {
    fn lower(
        self,
        graph: &mut NetworkBuilder,
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
        graph: &mut NetworkBuilder,
        name: &str,
        function: F,
    ) -> Result<DerivedRef<O>, Error> {
        let definition = source::transform(move |values: I::Values| function.call(values))?;
        let group = graph.sources(&format!("{name}.sources"), self)?;
        let component = graph.add(name, definition)?;
        graph.connect(group.output(), component.input("run")?, capacity())?;
        Ok(DerivedRef {
            output: component.output("value")?,
            component,
            root: group.root,
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
            fn lower(self, graph: &mut NetworkBuilder, name: &str, function: F) -> Result<DerivedRef<O>, Error> {
                let root = self.0.root.clone();
                if $( !Arc::ptr_eq(&root, &self.$i.root) )||+ { return Err(Error::MultipleRefreshRoots); }
                $(self.$i.validate(graph)?;)+
                let definition = source::function(
                    [$(PortDef::local::<$T>(stringify!($i))),+], PortDef::local::<O>("value"),
                    move |ctx| Ok(Value::local(function($(
                        ctx.take(stringify!($i)).ok_or("missing input")?
                            .borrow::<$T>().ok_or("wrong input type")?.clone()
                    ),+))),
                )?;
                let component = graph.add(name, definition)?;
                $(graph.connect(self.$i.output(), component.input::<$T>(stringify!($i))?, capacity())?;)+
                Ok(DerivedRef { output: component.output("value")?, component, root })
            }
        }
    };
}
tuples!(A:0);
tuples!(A:0, B:1);
tuples!(A:0, B:1, C:2);
tuples!(A:0, B:1, C:2, D:3);
