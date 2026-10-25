//! Typed M1 values. Locations identify cells; current host scope authorizes use.
use crate::Value as WireValue;
use crate::{
    abi::{Bytes as AbiBytes, StateHostApi},
    CallError, CallResult, Code, CompareExchange, Epoch, Failure, Revision, StateRequest,
    StaticSchema, ValueAccessError, ValueLocation,
};
pub use crate::{Operation, Portable};
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};
fn invalid() -> Failure {
    Failure::new(Code::SchemaRejected, "invalid typed live value")
}
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

/// A plugin-owned value update failed before publication, or publication
/// failed after the owner's candidate was constructed. In either case the
/// generated updater leaves the owner's Rust field unchanged.
#[derive(Debug)]
pub enum OwnedUpdateError<E> {
    Rejected(E),
    Publish(AccessError),
}

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
        let (status, value) =
            unsafe { callback(self.api.scope, request.opcode(), AbiBytes::new(&bytes)).consume() }?;
        match status {
            0 => Ok(value),
            1 => Err(AccessError::State(ValueAccessError::from_value(value)?)),
            2 => Err(AccessError::Kernel(Failure::from_value(value)?)),
            _ => Err(invalid().into()),
        }
    }
    fn raw(&self, slot: &str, key: &str, value: WireValue) -> Result<(u32, WireValue), Failure> {
        let bytes = value.encode()?;
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
            .consume()
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
    /// Enqueue a plugin-owned custom event in the current graph root.
    /// This does not synchronously recurse into another plugin or root.
    /// The kernel validates event ownership, payload schema and queue bounds.
    pub fn emit<T: Portable>(&self, event: &str, payload: &T) -> Result<(), Failure> {
        let request = WireValue::map([
            ("event", WireValue::String(event.into())),
            ("payload", payload.to_value()),
        ]);
        match self.raw("", crate::HOST_GRAPH_EMIT, request)? {
            (0, WireValue::Null) => Ok(()),
            (2, value) => Err(Failure::from_value(value)?),
            _ => Err(invalid()),
        }
    }

    pub fn slot_count(&self, slot: &str) -> Result<usize, Failure> {
        match self.raw(slot, "phenix.host.slots@1/count", WireValue::Null)? {
            (0, WireValue::U64(n)) => usize::try_from(n).map_err(|_| invalid()),
            (2, v) => Err(Failure::from_value(v)?),
            _ => Err(invalid()),
        }
    }
    /// The selection is a pinned provider identity, not a capability grant.
    /// Only a declared, actually bound import operation can be inspected.
    pub fn selected_provider<O: Operation>(&self, slot: &str) -> Result<String, Failure> {
        match self.raw(
            slot,
            "phenix.host.slots@1/selected",
            WireValue::String(O::KEY.into()),
        )? {
            (0, WireValue::String(provider)) => Ok(provider),
            (2, value) => Err(Failure::from_value(value)?),
            _ => Err(invalid()),
        }
    }
    /// Locate a selected property after its operation and resource checks.
    pub fn locate_property<T: Portable>(
        &self,
        slot: &str,
        key: &str,
    ) -> Result<ValueRef<T>, Failure> {
        match self.raw(
            slot,
            "phenix.host.property.v2",
            WireValue::String(key.into()),
        )? {
            (0, value) => ValueRef::from_value(value),
            (2, value) => Err(Failure::from_value(value)?),
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
    /// Publish a plugin-owned value to its selected observers.
    ///
    /// Read-only properties can only be published from the currently
    /// executing owner, under its admitted property resource. This uses the
    /// existing M1 compare-and-swap state request; it grants no external
    /// property `set` authority. The caller must commit its local Rust value
    /// only after this operation succeeds.
    pub fn publish<T: Portable>(
        &self,
        r: ValueRef<T>,
        expected: Revision,
        value: &T,
    ) -> AccessResult<Snapshot<T>> {
        self.compare_exchange(r, expected, value)
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
/// Generated synchronous handlers accept an ordinary value, domain Result or
/// CallResult. Contract types choose the conversion; aliases remain supported.
#[doc(hidden)]
pub trait HandlerResult<T, E> {
    fn into_call_result(self) -> CallResult<T, E>;
}
impl<T, E> HandlerResult<T, E> for T {
    fn into_call_result(self) -> CallResult<T, E> {
        Ok(self)
    }
}
impl<T, E> HandlerResult<T, E> for Result<T, E> {
    fn into_call_result(self) -> CallResult<T, E> {
        self.map_err(CallError::Domain)
    }
}
impl<T, E> HandlerResult<T, E> for CallResult<T, E> {
    fn into_call_result(self) -> CallResult<T, E> {
        self
    }
}
#[doc(hidden)]
pub fn handler_result<T, E, R: HandlerResult<T, E>>(result: R) -> CallResult<T, E> {
    result.into_call_result()
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
        Ok(v) => crate::abi::Reply::value(0, v.to_value()),
        Err(CallError::Domain(v)) => crate::abi::Reply::value(1, v.to_value()),
        Err(CallError::Kernel(v)) => crate::abi::Reply::value(2, v.to_value()),
    }
}

/// A native provider retains its own physical work until poll or cancellation drains it.
pub enum PendingResult<T, E> {
    Ready(CallResult<T, E>),
    Pending,
    QuiescentFailure(Failure),
}
#[doc(hidden)]
pub fn pending_reply<T: Portable, E: Portable>(result: PendingResult<T, E>) -> crate::abi::Reply {
    match result {
        PendingResult::Ready(result) => reply(result),
        PendingResult::Pending => crate::abi::Reply::value(3, WireValue::Null),
        PendingResult::QuiescentFailure(error) => crate::abi::Reply::value(5, error.to_value()),
    }
}
#[doc(hidden)]
pub fn boundary(f: impl FnOnce() -> crate::abi::Reply) -> crate::abi::Reply {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        crate::abi::Reply::value(
            2,
            Failure::new(Code::PluginFailed, "caught state plugin panic").to_value(),
        )
    })
}

/// Native declarations reject incomplete exports and unsupported field access.
/// ```compile_fail
/// use phenix_sdk::contract;
/// #[contract(id="test.native@1")] pub trait Api { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.missing",version="1.0.0",profile="state",define(Api),provide(Api,slot="service"))]
/// mod plugin { use super::*; #[derive(Default)] struct State {} }
/// ```
/// ```compile_fail
/// use phenix_sdk::contract;
/// #[contract(id="test.native@1")] pub trait Api { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.duplicate",version="1.0.0",profile="state",define(Api),provide(Api,slot="service"))]
/// mod plugin {
/// use super::*; #[derive(Default)] struct State {}
/// #[provide(Api::run)] fn a(_: &mut State,_: &phenix_sdk::live::Host<'_>,_:())->phenix_sdk::CallResult<u64,String>{Ok(1)}
/// #[provide(Api::run)] fn b(_: &mut State,_: &phenix_sdk::live::Host<'_>,_:())->phenix_sdk::CallResult<u64,String>{Ok(2)}
/// }
/// ```
/// ```compile_fail
/// #[phenix_sdk::plugin(id="test.fields",version="1.0.0",profile="state")]
/// mod plugin { #[derive(Default)] struct State { #[get(id="same")] a:u64, #[get(id="same")] b:u64 } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::plugin(id="test.pointer",version="1.0.0",profile="state")]
/// mod plugin { #[derive(Default)] struct State { #[get] pointer:*const u8 } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::property(owner="test.readonly",id="value",get)] type Value=u64;
/// fn change() { let _ = Value::set::KEY; }
/// ```
/// ```compile_fail
/// use phenix_sdk::contract;
/// #[contract(id="test.a@1")] pub trait A { fn run(&self)->Result<u64,String>; }
/// #[contract(id="test.b@1")] pub trait B { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.import",version="1.0.0",profile="state",require(A,slot="selected"))]
/// mod plugin {
/// use super::*; #[derive(Default)] struct State { #[get] value:u64 }
/// fn wrong(host:&phenix_sdk::live::Host<'_>) { let _ = imports::selected::invoke::<B::run>(host,&()); }
/// }
/// ```
/// ```compile_fail
/// use phenix_sdk::contract;
/// #[contract(id="test.a@1")] pub trait A { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.import",version="1.0.0",profile="state",require(A,slot="selected"))]
/// mod plugin {
/// use super::*; #[derive(Default)] struct State { #[get] value:u64 }
/// fn wrong(host:&phenix_sdk::live::Host<'_>) { let _ = imports::undeclared::invoke::<A::run>(host,&()); }
/// }
/// ```
/// ```compile_fail
/// use phenix_sdk::contract;
/// #[contract(id="test.native@1")] pub trait Api { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.mismatch",version="1.0.0",profile="state",define(Api),provide(Api,slot="service"))]
/// mod plugin {
/// use super::*; #[derive(Default)] struct State {}
/// #[provide(Api::run)] fn wrong(_: &mut State,_: &phenix_sdk::live::Host<'_>,_:())->phenix_sdk::CallResult<String,String>{Ok("wrong".into())}
/// }
/// ```
#[doc(hidden)]
pub struct UnsupportedNativeAuthoring;

/// Pending slot helpers retain the declared contract membership.
/// ```compile_fail
/// #[phenix_sdk::contract(id="test.a@1")] pub trait A { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::contract(id="test.b@1")] pub trait B { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.pending",require(A,slot="selected"))]
/// mod plugin { use super::*; fn wrong(host:&phenix_sdk::Host<'_>) { let _ = imports::selected::begin::<B::run>(host,&()); } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="test.a@1")] pub trait A { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::contract(id="test.b@1")] pub trait B { fn run(&self)->Result<u64,String>; }
/// #[phenix_sdk::plugin(id="test.pending",require(A,slot="selected",many(min=0)))]
/// mod plugin { use super::*; fn wrong(host:&phenix_sdk::Host<'_>,scope:&phenix_sdk::ScopedRequest) { let _ = imports::selected::begin_scoped_at::<B::run>(host,0,scope,&()); } }
/// ```
#[doc(hidden)]
pub struct UnsupportedPendingSlots;

/// Invalid concise signatures still fail against their declared contract.
/// ```compile_fail
/// #[phenix_sdk::contract(id="test.short@1")]
/// trait Api { fn call(&self, input:String)->Result<String,String>; }
/// #[phenix_sdk::plugin(id="test.short",define(Api),provide(Api,slot="api"))]
/// mod plugin { use super::*; #[provide(Api::call)] fn call()->String { String::new() } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="test.short@1")]
/// trait Api { fn call(&self, input:String)->Result<String,String>; }
/// #[phenix_sdk::plugin(id="test.short",define(Api),provide(Api,slot="api"))]
/// mod plugin { use super::*; #[provide(Api::call)] fn call(_:String,_:String)->String { String::new() } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="test.short@1")]
/// trait Api { fn call(&self, input:String)->Result<String,String>; }
/// #[phenix_sdk::plugin(id="test.short",define(Api),provide(Api,slot="api"))]
/// mod plugin { use super::*; #[provide(Api::call)] fn call(_:String)->u64 { 0 } }
/// ```
/// ```compile_fail
/// #[phenix_sdk::contract(id="test.short@1")]
/// trait Api { fn call(&self, input:String)->Result<String,String>; }
/// #[phenix_sdk::plugin(id="test.short",define(Api),provide(Api,slot="api"))]
/// mod plugin { use super::*; #[provide(Api::call)] fn call(_: &str)->String { String::new() } }
/// ```
#[doc(hidden)]
pub struct UnsupportedConciseAuthoring;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyError {
    Conflict,
    Denied,
    OwnerGone,
    SchemaMismatch,
    StaleReference,
}
impl Portable for PropertyError {
    const SCHEMA: StaticSchema = StaticSchema::Variants(&[
        "Conflict",
        "Denied",
        "OwnerGone",
        "SchemaMismatch",
        "StaleReference",
    ]);
    fn to_value(&self) -> WireValue {
        WireValue::String(
            match self {
                Self::Conflict => "Conflict",
                Self::Denied => "Denied",
                Self::OwnerGone => "OwnerGone",
                Self::SchemaMismatch => "SchemaMismatch",
                Self::StaleReference => "StaleReference",
            }
            .into(),
        )
    }
    fn from_value(value: WireValue) -> Result<Self, Failure> {
        let WireValue::String(value) = value else {
            return Err(invalid());
        };
        match value.as_str() {
            "Conflict" => Ok(Self::Conflict),
            "Denied" => Ok(Self::Denied),
            "OwnerGone" => Ok(Self::OwnerGone),
            "SchemaMismatch" => Ok(Self::SchemaMismatch),
            "StaleReference" => Ok(Self::StaleReference),
            _ => Err(invalid()),
        }
    }
}
pub struct PropertyUpdate<T> {
    pub expected: Revision,
    pub location: ValueRef<T>,
    pub value: T,
}
impl<T: Portable> Portable for PropertyUpdate<T> {
    const SCHEMA: StaticSchema = StaticSchema::Struct(&[
        crate::StaticField {
            name: "expected",
            schema: StaticSchema::U64,
        },
        crate::StaticField {
            name: "location",
            schema: ValueRef::<T>::SCHEMA,
        },
        crate::StaticField {
            name: "value",
            schema: T::SCHEMA,
        },
    ]);
    fn to_value(&self) -> WireValue {
        WireValue::map([
            ("expected", WireValue::U64(self.expected.0)),
            ("location", self.location.to_value()),
            ("value", self.value.to_value()),
        ])
    }
    fn from_value(value: WireValue) -> Result<Self, Failure> {
        let WireValue::Map(mut fields) = value else {
            return Err(invalid());
        };
        let result = Self {
            expected: Revision(u64::from_value(
                fields.remove("expected").ok_or_else(invalid)?,
            )?),
            location: ValueRef::from_value(fields.remove("location").ok_or_else(invalid)?)?,
            value: T::from_value(fields.remove("value").ok_or_else(invalid)?)?,
        };
        if !fields.is_empty() {
            return Err(invalid());
        }
        Ok(result)
    }
}

/// Unsupported source representations cannot become live contracts.
/// ```compile_fail
/// #[derive(phenix_sdk::Portable)] struct Pointer { address:*const u8 }
/// ```
/// ```compile_fail
/// #[derive(phenix_sdk::Portable)] struct Borrowed<'a> { value:&'a str }
/// ```
/// ```compile_fail
/// #[derive(phenix_sdk::Portable)] struct Tuple(String);
/// ```
/// ```compile_fail
/// fn modify(mut v:phenix_sdk::live::Value<Vec<String>>) { v.push("untracked".into()); }
/// ```
/// ```compile_fail
/// fn send<T:Send>() {} send::<phenix_sdk::live::Watch<String>>();
/// ```
#[doc(hidden)]
pub struct UnsupportedAuthoring;

/// A typed child remains owned by its parent ticket and selected import slot.
/// ```compile_fail
/// fn send<T:Send>(){} send::<phenix_sdk::PendingImport<O>>();
/// #[phenix_sdk::contract(id="test.pending@1")] trait Api { fn run(&self)->Result<String,String>; }
/// type O=Api::run;
/// ```
/// Opaque pending child tied to its named import and native parent.
/// It cannot be serialized as an authority-bearing value or sent to another thread.
/// ```compile_fail
/// use phenix_sdk::{Operation, PendingImport, Portable};
/// fn require_portable<T: Portable>() {}
/// fn check<O: Operation>() { require_portable::<PendingImport<O>>(); }
/// ```
/// ```compile_fail
/// use phenix_sdk::{Operation, PendingImport};
/// fn require_send<T: Send>() {}
/// fn check<O: Operation>() { require_send::<PendingImport<O>>(); }
/// ```
pub struct PendingImport<O: Operation> {
    slot: String,
    ticket: u64,
    settled: bool,
    _operation: PhantomData<O>,
    _serial: PhantomData<Rc<()>>,
}
pub enum ImportCall<O: Operation> {
    Ready(CallResult<O::Response, O::Error>),
    Pending(PendingImport<O>),
}
pub enum ImportPoll<T, E> {
    Pending,
    Ready(CallResult<T, E>),
    Cancelled,
    Draining,
}
fn import_result<O: Operation>(
    status: u32,
    value: WireValue,
) -> Result<CallResult<O::Response, O::Error>, Failure> {
    Ok(match status {
        0 => Ok(O::Response::from_value(value)?),
        1 => Err(CallError::Domain(O::Error::from_value(value)?)),
        2 => Err(CallError::Kernel(Failure::from_value(value)?)),
        _ => {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "invalid imported result",
            ))
        }
    })
}
impl Host<'_> {
    pub fn resource(&self) -> Result<Option<String>, Failure> {
        let (status, value) = self.raw("", crate::RESOURCE_CONTEXT, WireValue::Null)?;
        match (status, value) {
            (0, WireValue::Null) => Ok(None),
            (0, WireValue::String(id)) if crate::identifier(&id) => Ok(Some(id)),
            (2, value) => Err(Failure::from_value(value)?),
            _ => Err(invalid()),
        }
    }
    pub fn invoke_scoped<O: Operation>(
        &self,
        slot: &str,
        scope: &crate::ScopedRequest,
        request: &O::Request,
    ) -> CallResult<O::Response, O::Error> {
        let bytes = scope.encode(O::KEY, request.to_value().encode()?)?;
        let (status, value) = self.raw(slot, crate::SCOPED_IMPORT, WireValue::decode(&bytes)?)?;
        import_result::<O>(status, value)?
    }
    fn pending_command(
        &self,
        slot: &str,
        key: &str,
        value: WireValue,
    ) -> Result<(u32, WireValue), Failure> {
        let (status, value) = self.raw(slot, key, value)?;
        if status == 5 && value != WireValue::Null {
            return Err(Failure::from_value(value)?);
        }
        Ok((status, value))
    }
    pub fn begin<O: Operation>(
        &self,
        slot: &str,
        request: &O::Request,
    ) -> Result<ImportCall<O>, Failure> {
        self.begin_command::<O>(
            slot,
            "phenix.host.pending.begin.v2",
            WireValue::map([
                ("operation", WireValue::String(O::KEY.into())),
                ("request", WireValue::Bytes(request.to_value().encode()?)),
            ]),
        )
    }
    pub fn begin_scoped<O: Operation>(
        &self,
        slot: &str,
        scope: &crate::ScopedRequest,
        request: &O::Request,
    ) -> Result<ImportCall<O>, Failure> {
        self.begin_command::<O>(
            slot,
            "phenix.host.pending.scoped.v2",
            WireValue::decode(&scope.encode(O::KEY, request.to_value().encode()?)?)?,
        )
    }
    fn begin_command<O: Operation>(
        &self,
        slot: &str,
        key: &str,
        value: WireValue,
    ) -> Result<ImportCall<O>, Failure> {
        let (status, value) = self.pending_command(slot, key, value)?;
        match (status, value) {
            (3, WireValue::U64(ticket)) => Ok(ImportCall::Pending(PendingImport {
                slot: slot.into(),
                ticket,
                settled: false,
                _operation: PhantomData,
                _serial: PhantomData,
            })),
            (2, value) => Err(Failure::from_value(value)?),
            (status, value) => Ok(ImportCall::Ready(import_result::<O>(status, value)?)),
        }
    }
}
impl<O: Operation> PendingImport<O> {
    fn value(&self) -> Result<WireValue, Failure> {
        if self.settled {
            return Err(Failure::new(
                Code::ScopeRejected,
                "imported result already settled",
            ));
        }
        Ok(WireValue::map([
            ("operation", WireValue::String(O::KEY.into())),
            ("ticket", WireValue::U64(self.ticket)),
        ]))
    }
    pub fn poll(&mut self, host: &Host<'_>) -> Result<ImportPoll<O::Response, O::Error>, Failure> {
        match host.pending_command(&self.slot, "phenix.host.pending.poll.v2", self.value()?)? {
            (3, WireValue::Null) => Ok(ImportPoll::Pending),
            (5, WireValue::Null) => Ok(ImportPoll::Draining),
            (4, WireValue::Null) => {
                self.settled = true;
                Ok(ImportPoll::Cancelled)
            }
            (status @ 0..=2, value) => {
                self.settled = true;
                Ok(ImportPoll::Ready(import_result::<O>(status, value)?))
            }
            _ => Err(Failure::new(
                Code::AbiIncompatible,
                "invalid imported poll reply",
            )),
        }
    }
    pub fn cancel(&self, host: &Host<'_>) -> Result<bool, Failure> {
        match host.pending_command(&self.slot, "phenix.host.pending.cancel.v2", self.value()?)? {
            (0, WireValue::Bool(quiet)) => Ok(quiet),
            _ => Err(Failure::new(
                Code::AbiIncompatible,
                "invalid imported cancel reply",
            )),
        }
    }
}
