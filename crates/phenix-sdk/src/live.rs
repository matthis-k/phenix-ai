//! Typed M1 values. Locations identify cells; current host scope authorizes use.
use crate::Value as WireValue;
use crate::{
    abi::{Bytes as AbiBytes, StateHostApi},
    CallError, CallResult, Code, CompareExchange, Epoch, Failure, Revision, StateRequest,
    StaticSchema, ValueAccessError, ValueLocation, WireProfile,
};
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};
pub trait Portable: Sized {
    const SCHEMA: StaticSchema;
    fn to_value(&self) -> WireValue;
    fn from_value(value: WireValue) -> Result<Self, Failure>;
}
fn invalid() -> Failure {
    Failure::new(Code::SchemaRejected, "invalid typed live value")
}
macro_rules! scalar {
    ($t:ty,$variant:ident,$schema:ident) => {
        impl Portable for $t {
            const SCHEMA: StaticSchema = StaticSchema::$schema;
            fn to_value(&self) -> WireValue {
                WireValue::$variant(self.clone())
            }
            fn from_value(value: WireValue) -> Result<Self, Failure> {
                if let WireValue::$variant(v) = value {
                    Ok(v)
                } else {
                    Err(invalid())
                }
            }
        }
    };
}
scalar!(bool, Bool, Bool);
scalar!(i64, I64, I64);
scalar!(u64, U64, U64);
scalar!(String, String, String);
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);
impl Portable for Bytes {
    const SCHEMA: StaticSchema = StaticSchema::Bytes;
    fn to_value(&self) -> WireValue {
        WireValue::Bytes(self.0.clone())
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        if let WireValue::Bytes(v) = v {
            Ok(Self(v))
        } else {
            Err(invalid())
        }
    }
}
impl Portable for () {
    const SCHEMA: StaticSchema = StaticSchema::Struct(&[]);
    fn to_value(&self) -> WireValue {
        WireValue::map([] as [(&str, WireValue); 0])
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        if matches!(v,WireValue::Map(ref m) if m.is_empty()) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
impl<T: Portable> Portable for Vec<T> {
    const SCHEMA: StaticSchema = StaticSchema::List(&T::SCHEMA);
    fn to_value(&self) -> WireValue {
        WireValue::List(self.iter().map(T::to_value).collect())
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        if let WireValue::List(items) = v {
            items.into_iter().map(T::from_value).collect()
        } else {
            Err(invalid())
        }
    }
}
impl<T: Portable> Portable for Option<T> {
    const SCHEMA: StaticSchema = StaticSchema::Option(&T::SCHEMA);
    fn to_value(&self) -> WireValue {
        self.as_ref()
            .map_or(WireValue::Null, |v| WireValue::List(vec![v.to_value()]))
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        match v {
            WireValue::Null => Ok(None),
            WireValue::List(mut v) if v.len() == 1 => Ok(Some(T::from_value(v.pop().unwrap())?)),
            _ => Err(invalid()),
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct Value<T>(Arc<T>);
impl<T> Clone for Value<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Deref for Value<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> AsRef<T> for Value<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}
impl<T> From<T> for Value<T> {
    fn from(value: T) -> Self {
        Self(Arc::new(value))
    }
}
impl<T: Portable> Portable for Value<T> {
    const SCHEMA: StaticSchema = T::SCHEMA;
    fn to_value(&self) -> WireValue {
        self.0.to_value()
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        Ok(T::from_value(v)?.into())
    }
}
/// A location has no dereference or mutable dereference implementation.
/// ```compile_fail
/// fn change(mut r:phenix_sdk::live::ValueRef<Vec<String>>) { r.push("untracked".into()); }
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ValueRef<T> {
    location: ValueLocation,
    _type: PhantomData<fn() -> T>,
}
impl<T> Copy for ValueRef<T> {}
impl<T> Clone for ValueRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Portable> ValueRef<T> {
    pub fn from_location(location: ValueLocation) -> Result<Self, Failure> {
        if location.schema != T::SCHEMA.id()? {
            return Err(invalid());
        }
        Ok(Self {
            location,
            _type: PhantomData,
        })
    }
    pub fn location(&self) -> ValueLocation {
        self.location
    }
    pub fn read(&self, host: &Host<'_>) -> AccessResult<Snapshot<T>> {
        host.read(*self)
    }
    pub fn edit(&self, host: &Host<'_>) -> AccessResult<Edit<T>>
    where
        T: Clone,
    {
        Ok(Edit {
            reference: *self,
            snapshot: self.read(host)?,
            draft: None,
        })
    }
    pub fn watch(&self, host: &Host<'_>) -> AccessResult<(Watch<T>, Snapshot<T>)> {
        host.subscribe(*self)
    }
}
impl<T: Portable> Portable for ValueRef<T> {
    const SCHEMA: StaticSchema = StaticSchema::Ref(&T::SCHEMA);
    fn to_value(&self) -> WireValue {
        WireValue::Ref(Box::new(self.location))
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        if let WireValue::Ref(r) = v {
            Self::from_location(*r)
        } else {
            Err(invalid())
        }
    }
}
#[derive(Debug)]
pub struct Snapshot<T> {
    pub value: Value<T>,
    pub revision: Revision,
    pub epoch: Epoch,
}
impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            revision: self.revision,
            epoch: self.epoch,
        }
    }
}
impl<T: Portable> Snapshot<T> {
    fn decode(v: WireValue) -> Result<Self, Failure> {
        let WireValue::List(v) = v else {
            return Err(invalid());
        };
        let [WireValue::U64(revision), WireValue::U64(epoch), value]: [WireValue; 3] =
            v.try_into().map_err(|_| invalid())?
        else {
            return Err(invalid());
        };
        Ok(Self {
            value: T::from_value(value)?.into(),
            revision: Revision(revision),
            epoch: Epoch(epoch),
        })
    }
}
/// A draft publishes only through commit. Dropping it has no host effect.
pub struct Edit<T> {
    reference: ValueRef<T>,
    snapshot: Snapshot<T>,
    draft: Option<T>,
}
impl<T> Deref for Edit<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.draft.as_ref().unwrap_or(&self.snapshot.value)
    }
}
impl<T: Clone> DerefMut for Edit<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.draft
            .get_or_insert_with(|| self.snapshot.value.as_ref().clone())
    }
}
impl<T: Portable> Edit<T> {
    pub fn commit(&self, host: &Host<'_>) -> AccessResult<Snapshot<T>> {
        host.compare_exchange(self.reference, self.snapshot.revision, self.deref())
    }
    pub fn batch_edit(&self) -> BatchEdit {
        BatchEdit(CompareExchange {
            location: self.reference.location,
            expected: self.snapshot.revision,
            value: self.deref().to_value(),
        })
    }
    pub fn expected(&self) -> Revision {
        self.snapshot.revision
    }
}
pub struct BatchEdit(CompareExchange);
pub struct BatchSnapshot {
    pub epoch: Epoch,
    snapshots: Vec<(ValueLocation, DynamicSnapshot)>,
}
impl BatchSnapshot {
    pub fn get<T: Portable>(&self, reference: ValueRef<T>) -> Result<Snapshot<T>, Failure> {
        let (_, snapshot) = self
            .snapshots
            .iter()
            .find(|(location, _)| *location == reference.location)
            .ok_or_else(invalid)?;
        Snapshot::decode(snapshot.0.clone())
    }
}
pub struct DynamicSnapshot(WireValue);
impl DynamicSnapshot {
    pub fn decode<T: Portable>(self) -> Result<Snapshot<T>, Failure> {
        Snapshot::decode(self.0)
    }
}
pub struct EpochSnapshot<T> {
    pub epoch: Epoch,
    pub snapshots: Vec<(ValueRef<T>, Snapshot<T>)>,
}
#[derive(Debug)]
pub enum AccessError {
    State(ValueAccessError),
    Kernel(Failure),
}
pub type AccessResult<T> = Result<T, AccessError>;
impl From<Failure> for AccessError {
    fn from(e: Failure) -> Self {
        Self::Kernel(e)
    }
}
impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for AccessError {}
/// Borrowed call scope. It cannot be sent to a worker.
/// ```compile_fail
/// fn send<T:Send>() {} send::<phenix_sdk::live::Host<'static>>();
/// ```
pub struct Host<'call> {
    api: &'call StateHostApi,
    _serial: PhantomData<Rc<()>>,
}
impl<'call> Host<'call> {
    /// # Safety
    /// The negotiated API must be issued by the kernel for this native entry.
    pub unsafe fn from_api(api: &'call StateHostApi) -> Self {
        Self {
            api,
            _serial: PhantomData,
        }
    }
    fn state(&self, request: StateRequest) -> AccessResult<WireValue> {
        let bytes = request.encode()?;
        let callback = self
            .api
            .state
            .ok_or_else(|| Failure::new(Code::AbiIncompatible, "missing state callback"))?;
        let (status, value) = unsafe {
            callback(self.api.scope, request.opcode(), AbiBytes::new(&bytes)).consume_state()
        }?;
        match status {
            0 => Ok(value),
            1 => Err(AccessError::State(ValueAccessError::from_value(value)?)),
            2 => Err(AccessError::Kernel(Failure::from_value(value)?)),
            _ => Err(invalid().into()),
        }
    }
    fn raw(&self, slot: &str, key: &str, value: WireValue) -> Result<(u32, WireValue), Failure> {
        let bytes = value.encode_profile(WireProfile::V2)?;
        let callback = self
            .api
            .invoke
            .ok_or_else(|| Failure::new(Code::AbiIncompatible, "missing import callback"))?;
        unsafe {
            callback(
                self.api.scope,
                AbiBytes::new(slot.as_bytes()),
                AbiBytes::new(key.as_bytes()),
                AbiBytes::new(&bytes),
            )
            .consume_state()
        }
    }
    pub fn invoke<O: Operation>(
        &self,
        slot: &str,
        request: &O::Request,
    ) -> CallResult<O::Response, O::Error> {
        let (status, value) = self.raw(slot, O::KEY, request.to_value())?;
        match status {
            0 => Ok(O::Response::from_value(value)?),
            1 => Err(CallError::Domain(O::Error::from_value(value)?)),
            2 => Err(CallError::Kernel(Failure::from_value(value)?)),
            _ => Err(CallError::Kernel(invalid())),
        }
    }
    pub fn slot_count(&self, slot: &str) -> Result<usize, Failure> {
        match self.raw(slot, "phenix.host.slots@1/count", WireValue::Null)? {
            (0, WireValue::U64(n)) => usize::try_from(n).map_err(|_| invalid()),
            (2, v) => Err(Failure::from_value(v)?),
            _ => Err(invalid()),
        }
    }
    pub fn create<T: Portable>(&self, property: &str, initial: &T) -> AccessResult<ValueRef<T>> {
        Ok(ValueRef::from_value(self.state(StateRequest::Create {
            property: property.into(),
            initial: initial.to_value(),
        })?)?)
    }
    pub fn read<T: Portable>(&self, r: ValueRef<T>) -> AccessResult<Snapshot<T>> {
        Ok(Snapshot::decode(
            self.state(StateRequest::Read(r.location))?,
        )?)
    }
    pub fn compare_exchange<T: Portable>(
        &self,
        r: ValueRef<T>,
        expected: Revision,
        value: &T,
    ) -> AccessResult<Snapshot<T>> {
        Ok(Snapshot::decode(self.state(
            StateRequest::CompareExchange(CompareExchange {
                location: r.location,
                expected,
                value: value.to_value(),
            }),
        )?)?)
    }
    pub fn batch(&self, edits: Vec<BatchEdit>) -> AccessResult<BatchSnapshot> {
        let mut edits: Vec<_> = edits.into_iter().map(|v| v.0).collect();
        edits.sort_by_key(|e| e.location);
        let locations: Vec<_> = edits.iter().map(|e| e.location).collect();
        let value = self.state(StateRequest::Batch(edits))?;
        let WireValue::List(value) = value else {
            return Err(invalid().into());
        };
        let [WireValue::U64(epoch), WireValue::List(snapshots)]: [WireValue; 2] =
            value.try_into().map_err(|_| invalid())?
        else {
            return Err(invalid().into());
        };
        if snapshots.len() != locations.len() {
            return Err(invalid().into());
        }
        Ok(BatchSnapshot {
            epoch: Epoch(epoch),
            snapshots: locations
                .into_iter()
                .zip(snapshots.into_iter().map(DynamicSnapshot))
                .collect(),
        })
    }
    /// Results follow canonical location order. Duplicate locations reject.
    pub fn snapshot<T: Portable>(&self, refs: &[ValueRef<T>]) -> AccessResult<EpochSnapshot<T>> {
        let mut refs = refs.to_vec();
        refs.sort_by_key(|r| r.location);
        let (status, value) = self.raw(
            "",
            "phenix.host.state@1/snapshot",
            WireValue::List(refs.iter().map(Portable::to_value).collect()),
        )?;
        if status == 2 {
            return Err(AccessError::Kernel(Failure::from_value(value)?));
        }
        if status == 1 {
            return Err(AccessError::State(ValueAccessError::from_value(value)?));
        }
        if status != 0 {
            return Err(invalid().into());
        }
        let WireValue::List(value) = value else {
            return Err(invalid().into());
        };
        let [WireValue::U64(epoch), WireValue::List(snapshots)]: [WireValue; 2] =
            value.try_into().map_err(|_| invalid())?
        else {
            return Err(invalid().into());
        };
        if refs.len() != snapshots.len() {
            return Err(invalid().into());
        }
        Ok(EpochSnapshot {
            epoch: Epoch(epoch),
            snapshots: refs
                .into_iter()
                .zip(snapshots)
                .map(|(r, v)| Ok((r, Snapshot::decode(v)?)))
                .collect::<Result<_, Failure>>()?,
        })
    }
    pub fn subscribe<T: Portable>(&self, r: ValueRef<T>) -> AccessResult<(Watch<T>, Snapshot<T>)> {
        let WireValue::List(v) = self.state(StateRequest::Subscribe(r.location))? else {
            return Err(invalid().into());
        };
        let [WireValue::U64(cursor), snapshot]: [WireValue; 2] =
            v.try_into().map_err(|_| invalid())?
        else {
            return Err(invalid().into());
        };
        if cursor == 0 {
            return Err(invalid().into());
        }
        Ok((
            Watch {
                cursor,
                closed: false,
                _type: PhantomData,
                _serial: PhantomData,
            },
            Snapshot::decode(snapshot)?,
        ))
    }
    pub fn wake_permit(&self) -> crate::WakePermit {
        crate::WakePermit {
            ticket: self.api.ticket,
            permit: self.api.permit,
            wake: self.api.wake.expect("negotiated state host wake"),
        }
    }
}
/// An M1 operation declaration. The current host scope supplies authority.
///
/// ```compile_fail
/// mod declarations {
/// #[phenix_sdk::contract(id="example.counter@1")]
/// trait Counter { fn read(&self) -> Result<u64, String>; }
/// #[phenix_sdk::plugin(id="example.bad",version="1.0.0",profile="definitions",define(Counter),define(Counter))]
/// mod bad { use super::*; }
/// }
/// ```
/// ```compile_fail
/// mod declarations {
/// #[phenix_sdk::contract(id="example.counter@1")]
/// trait Counter { fn read(&self) -> Result<u64, String>; }
/// #[phenix_sdk::plugin(id="example.bad",version="1.0.0",profile="definitions",require(Counter))]
/// mod bad { use super::*; }
/// }
/// ```
/// ```compile_fail
/// mod declarations {
/// #[phenix_sdk::contract(id="example.counter@1")]
/// trait Counter { fn read(&self) -> Result<u64, String>; }
/// #[phenix_sdk::plugin(id="example.bad",version="1.0.0",profile="definitions",define(Counter))]
/// mod bad { use super::*; struct State; }
/// }
/// ```
///
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { fn call(&self, input: &str) -> Result<String, String>; }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { fn call(&self) -> Result<*const u8, String>; }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { async fn call(&self) -> Result<String, String>; }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad<T> { fn call(&self, input: T) -> Result<String, String>; }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { fn call(&self, a: String, b: String) -> Result<String, String>; }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { fn call(&self) -> Result<String, String> { Ok(String::new()) } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { fn call(&self) -> String; }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="example.bad@1")]
/// trait Bad { fn call(&self) -> Result<String, String>; fn call(&self) -> Result<String, String>; }
/// ```
pub trait Operation {
    type Request: Portable;
    type Response: Portable;
    type Error: Portable;
    const KEY: &'static str;
}
/// A subscription is scoped to its admitted root and native ticket.
pub struct Watch<T> {
    cursor: u64,
    closed: bool,
    _type: PhantomData<fn() -> T>,
    _serial: PhantomData<Rc<()>>,
}
impl<T: Portable> Watch<T> {
    pub fn next(&mut self, host: &Host<'_>) -> AccessResult<Option<Snapshot<T>>> {
        if self.closed {
            return Err(Failure::new(Code::ScopeRejected, "watch is closed").into());
        }
        let v = host.state(StateRequest::Next(self.cursor))?;
        if v == WireValue::Null {
            Ok(None)
        } else {
            Ok(Some(Snapshot::decode(v)?))
        }
    }
    pub fn close(&mut self, host: &Host<'_>) -> AccessResult<()> {
        if self.closed {
            return Ok(());
        }
        if host.state(StateRequest::Unsubscribe(self.cursor))? != WireValue::Null {
            return Err(invalid().into());
        }
        self.closed = true;
        Ok(())
    }
}
/// Generated property storage has no direct mutable access to published T.
/// ```compile_fail
/// fn mutate(mut cell:phenix_sdk::live::Cell<Vec<String>>) { cell.push("untracked".into()); }
/// ```
pub struct Cell<T> {
    initial: Option<T>,
    reference: Option<ValueRef<T>>,
}
impl<T> From<T> for Cell<T> {
    fn from(v: T) -> Self {
        Self {
            initial: Some(v),
            reference: None,
        }
    }
}
impl<T: Default> Default for Cell<T> {
    fn default() -> Self {
        T::default().into()
    }
}
impl<T: Portable> Cell<T> {
    pub fn bind(&mut self, host: &Host<'_>, property: &str) -> AccessResult<()> {
        if self.reference.is_some() {
            return Err(Failure::new(Code::ScopeRejected, "property already bound").into());
        }
        let initial = self
            .initial
            .as_ref()
            .ok_or_else(|| Failure::new(Code::ScopeRejected, "property has no initial value"))?;
        self.reference = Some(host.create(property, initial)?);
        self.initial = None;
        Ok(())
    }
    pub fn reference(&self) -> Result<ValueRef<T>, Failure> {
        self.reference
            .ok_or_else(|| Failure::new(Code::ScopeRejected, "property is unbound"))
    }
    pub fn read(&self, host: &Host<'_>) -> AccessResult<Snapshot<T>> {
        self.reference()?.read(host)
    }
    pub fn edit(&self, host: &Host<'_>) -> AccessResult<Edit<T>>
    where
        T: Clone,
    {
        self.reference()?.edit(host)
    }
}
#[doc(hidden)]
pub fn reply<T: Portable, E: Portable>(result: CallResult<T, E>) -> crate::abi::Reply {
    match result {
        Ok(v) => crate::abi::Reply::state_value(0, v.to_value()),
        Err(CallError::Domain(v)) => crate::abi::Reply::state_value(1, v.to_value()),
        Err(CallError::Kernel(v)) => crate::abi::Reply::state_value(2, v.to_value()),
    }
}

/// Unsupported source representations cannot become live contracts.
/// ```compile_fail
/// #[derive(phenix_sdk::LivePortable)] struct Pointer { address:*const u8 }
/// ```
/// ```compile_fail
/// #[derive(phenix_sdk::LivePortable)] struct Borrowed<'a> { value:&'a str }
/// ```
/// ```compile_fail
/// #[derive(phenix_sdk::LivePortable)] struct Tuple(String);
/// ```
/// ```compile_fail
/// fn modify(mut v:phenix_sdk::live::Value<Vec<String>>) { v.push("untracked".into()); }
/// ```
/// ```compile_fail
/// fn send<T:Send>() {} send::<phenix_sdk::live::Watch<String>>();
/// ```
#[doc(hidden)]
pub struct UnsupportedAuthoring;
