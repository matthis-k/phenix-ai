//! Local payload ownership and schema-identified portable value envelopes.
use phenix_schema::Schema;
use std::any::{Any, TypeId, type_name};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Local Rust identities never depend on `type_name` strings. Wire types
/// retain explicit versioned schema names and are never silently compatible
/// with local Rust values, even if the textual name happens to match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeIdentity {
    Local(TypeId),
    Wire(Arc<str>),
}
/// The wire format is schema identity plus canonical bytes for that schema.
/// Only a few codecs are included to keep this experiment self-contained.
/// Values are locally typed payloads or explicitly encoded wire envelopes.
/// A local payload is never encoded; fan-out clones only its Arc.
#[derive(Clone)]
pub struct Value {
    schema: Arc<str>,
    bytes: Vec<u8>,
    local: Option<Arc<dyn Any + Send + Sync>>,
    // Declaration order ensures payload destructors run before code/resources.
    leases: Vec<Arc<dyn Send + Sync>>,
}

/// Shared typed ownership that retains the value's code/resource leases.
/// There is deliberately no raw `T` or `Arc<T>` extraction or mutable access.
/// As with `Value::borrow`, explicitly cloning data through the borrowed `T`
/// is an application responsibility; this wrapper cannot constrain `T`'s APIs.
pub struct Leased<T> {
    value: Arc<T>,
    leases: Vec<Arc<dyn Send + Sync>>,
}
impl<T> Clone for Leased<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            leases: self.leases.clone(),
        }
    }
}
impl<T> Deref for Leased<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T: fmt::Debug> fmt::Debug for Leased<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Leased").field(&self.value).finish()
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Value")
            .field("schema", &self.schema)
            .field("wire_bytes", &self.bytes.len())
            .field("is_local", &self.local.is_some())
            .finish()
    }
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && match (&self.local, &other.local) {
                (None, None) => self.bytes == other.bytes,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}
impl Eq for Value {}

pub trait ContractValue: Sized + Send + Sync + 'static {
    /// Complete wire contract, independent of the implementing Rust type.
    fn schema() -> Schema;
    /// Owned canonical identity. Consumers can cheaply clone the returned Arc.
    fn wire_schema() -> Arc<str> {
        Self::schema().identity()
    }
    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self, String>;
}
impl ContractValue for i64 {
    fn schema() -> Schema {
        Schema {
            name: "example.i64".into(),
            revision: 1,
            structure: "i64".into(),
            codec: "i64-le-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let arr: [u8; 8] = bytes.try_into().map_err(|_| "bad i64 length".to_string())?;
        Ok(i64::from_le_bytes(arr))
    }
}
impl ContractValue for String {
    fn schema() -> Schema {
        Schema {
            name: "example.utf8".into(),
            revision: 1,
            structure: "string".into(),
            codec: "utf8-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
    }
}
macro_rules! integer_contract {
    ($($ty:ty => $schema:literal),* $(,)?) => {$ (
        impl ContractValue for $ty {
            fn schema() -> Schema {
                Schema { name: $schema.into(), revision: 1, structure: stringify!($ty).into(), codec: concat!(stringify!($ty), "-le-v1").into() }
            }
            fn encode(&self) -> Vec<u8> { self.to_le_bytes().to_vec() }
            fn decode(bytes: &[u8]) -> Result<Self, String> {
                let array: [u8; std::mem::size_of::<Self>()] = bytes.try_into()
                    .map_err(|_| format!("bad {} length", stringify!($ty)))?;
                Ok(<$ty>::from_le_bytes(array))
            }
        }
    )*};
}
integer_contract!(
    i8 => "example.i8", i16 => "example.i16", i32 => "example.i32",
    i128 => "example.i128", u8 => "example.u8", u16 => "example.u16",
    u32 => "example.u32", u64 => "example.u64", u128 => "example.u128",
    f32 => "example.f32", f64 => "example.f64",
);
impl ContractValue for bool {
    fn schema() -> Schema {
        Schema {
            name: "example.bool".into(),
            revision: 1,
            structure: "bool".into(),
            codec: "bool-u8-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        vec![u8::from(*self)]
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        match bytes {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err("bad bool encoding".into()),
        }
    }
}
impl ContractValue for () {
    fn schema() -> Schema {
        Schema {
            name: "example.unit".into(),
            revision: 1,
            structure: "unit".into(),
            codec: "empty-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            Ok(())
        } else {
            Err("unit value must be empty".into())
        }
    }
}
pub(crate) fn enc<T: ContractValue>(v: T) -> Value {
    Value {
        schema: T::schema().identity(),
        bytes: v.encode(),
        local: None,
        leases: Vec::new(),
    }
}
impl Value {
    /// Attach ownership of code/resources required by this payload and its clones.
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        if !self.leases.iter().any(|held| Arc::ptr_eq(held, &lease)) {
            self.leases.push(lease);
        }
        self
    }
    pub(crate) fn retain_leases_from(&mut self, source: &Self) {
        for lease in &source.leases {
            if !self.leases.iter().any(|held| Arc::ptr_eq(held, lease)) {
                self.leases.push(lease.clone());
            }
        }
    }
    /// Move a unique local leaf through reconstruction without detaching its
    /// leases. `self` pins them while `finish` runs, including error/unwind paths.
    pub(crate) fn map_local<T: Send + Sync + 'static>(
        mut self,
        finish: impl FnOnce(T) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let data = self.local.take().ok_or("value is encoded, not local")?;
        let typed = Arc::downcast::<T>(data)
            .map_err(|_| format!("local value type mismatch: expected {}", type_name::<T>()))?;
        let payload = Arc::try_unwrap(typed)
            .map_err(|_| "local input is shared; use an explicit Arc field".to_owned())?;
        let mut result = finish(payload)?;
        result.retain_leases_from(&self);
        Ok(result)
    }
    fn require_unleased(&self) -> Result<(), String> {
        if self.leases.is_empty() {
            Ok(())
        } else {
            Err(
                "leased value cannot yield bare ownership; use borrow, leased_shared, or decoded"
                    .into(),
            )
        }
    }
    pub fn of<T: ContractValue>(value: T) -> Self {
        enc(value)
    }
    /// Only a prototype wire envelope. No Rust layout or trait object crosses it.
    pub fn schema(&self) -> &Arc<str> {
        &self.schema
    }
    pub fn identity(&self) -> TypeIdentity {
        match &self.local {
            Some(value) => TypeIdentity::Local(value.as_ref().type_id()),
            None => TypeIdentity::Wire(self.schema.clone()),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn from_wire_parts(schema: Arc<str>, bytes: Vec<u8>) -> Self {
        Self {
            schema,
            bytes,
            local: None,
            leases: Vec::new(),
        }
    }
    /// Construct an in-process, non-serializing value. No codec trait required.
    pub fn local<T: Send + Sync + 'static>(value: T) -> Self {
        Self {
            schema: type_name::<T>().into(),
            bytes: Vec::new(),
            local: Some(Arc::new(value)),
            leases: Vec::new(),
        }
    }
    /// A typed view of local data; never copies or decodes the payload.
    /// The reference cannot outlive this Value. Explicitly cloning `T` or using
    /// its own extraction APIs is outside the lease guarantee and must be safe
    /// under the application's contract for `T`.
    pub fn borrow<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.local.as_ref()?.downcast_ref::<T>()
    }
    /// Shared ownership of locally stored data, including non-Clone types.
    /// Leased values reject bare extraction; use `leased_shared` instead.
    pub fn shared<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, String> {
        self.require_unleased()?;
        self.local_shared()
    }
    fn local_shared<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, String> {
        let data = self.local.as_ref().ok_or("value is encoded, not local")?;
        Arc::downcast::<T>(Arc::clone(data)).map_err(|_| {
            format!(
                "local value type mismatch: expected {}, found {}",
                type_name::<T>(),
                self.schema
            )
        })
    }
    /// Consume an unleased local envelope, returning shared payload ownership.
    /// Leased values reject bare extraction; use `leased_shared` instead.
    pub fn into_shared<T: Send + Sync + 'static>(self) -> Result<Arc<T>, String> {
        self.require_unleased()?;
        let data = self.local.ok_or("value is encoded, not local")?;
        Arc::downcast::<T>(data)
            .map_err(|_| format!("local value type mismatch: expected {}", type_name::<T>()))
    }
    /// Unwrap or clone an unleased local payload. Leased values return an error.
    pub fn into_owned<T: Clone + Send + Sync + 'static>(self) -> Result<T, String> {
        let shared = self.into_shared::<T>()?;
        Ok(Arc::try_unwrap(shared).unwrap_or_else(|value| (*value).clone()))
    }
    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }
    /// Decode an unleased wire payload. For leased values use `decoded` instead.
    pub fn get<T: ContractValue>(&self) -> Result<T, String> {
        self.require_unleased()?;
        self.decode()
    }
    /// Shared local data with all leases retained, including for non-Clone `T`.
    pub fn leased_shared<T: Send + Sync + 'static>(&self) -> Result<Leased<T>, String> {
        Ok(Leased {
            value: self.local_shared()?,
            leases: self.leases.clone(),
        })
    }
    /// Decode canonical wire data while retaining all code/resource leases.
    pub fn decoded<T: ContractValue>(&self) -> Result<Leased<T>, String> {
        Ok(Leased {
            value: Arc::new(self.decode()?),
            leases: self.leases.clone(),
        })
    }
    fn decode<T: ContractValue>(&self) -> Result<T, String> {
        let expected = T::schema().identity();
        if self.schema != expected {
            return Err(format!("expected {}, got {}", expected, self.schema));
        }
        if self.is_local() {
            return Err("local values are not encoded; use borrow/shared".into());
        }
        T::decode(&self.bytes)
    }
}
