//! Local payload ownership and schema-identified portable value envelopes.
use crate::{CallbackPhase, Failure, FailureKind};
use phenix_schema::Schema;
use std::any::{Any, TypeId, type_name};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Host-owned details for value access and canonical codec rejection.
#[derive(Clone, Debug)]
pub enum ValueError {
    EncodedNotLocal,
    LocalNotEncoded,
    LocalType {
        expected: &'static str,
        actual: TypeIdentity,
    },
    SharedInput,
    LeasedOwnership,
    Schema {
        expected: Arc<str>,
        actual: Arc<str>,
    },
    Length {
        value_type: &'static str,
        expected: usize,
        actual: usize,
        cause: Option<std::array::TryFromSliceError>,
    },
    InvalidBool {
        length: usize,
        tag: Option<u8>,
    },
    DestructorPanicked {
        actual: TypeIdentity,
    },
}
impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncodedNotLocal => f.write_str("value is encoded, not local"),
            Self::LocalNotEncoded => f.write_str("local values are not encoded; use borrow/shared"),
            Self::LocalType { expected, .. } => {
                write!(f, "local value type mismatch: expected {expected}")
            }
            Self::SharedInput => f.write_str("local input is shared; use an explicit Arc field"),
            Self::LeasedOwnership => f.write_str(
                "leased value cannot yield bare ownership; use borrow, leased_shared, or decoded",
            ),
            Self::Schema { expected, actual } => write!(f, "expected {expected}, got {actual}"),
            Self::Length { value_type, .. } => write!(f, "bad {value_type} length"),
            Self::InvalidBool { .. } => f.write_str("bad bool encoding"),
            Self::DestructorPanicked { .. } => f.write_str("value destructor panicked"),
        }
    }
}
impl std::error::Error for ValueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Length {
                cause: Some(cause), ..
            } => Some(cause),
            _ => None,
        }
    }
}
impl ValueError {
    pub fn failure(self) -> Failure {
        Failure::external(format!("{self}"), self).with_kind(FailureKind::Framework)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    OffsetOverflow {
        field: String,
        offset: usize,
        length: usize,
    },
    TruncatedHeader {
        field: String,
        offset: usize,
        remaining: usize,
    },
    LengthOverflow {
        field: String,
        length: u64,
        cause: std::num::TryFromIntError,
    },
    TruncatedPayload {
        field: String,
        offset: usize,
        length: usize,
        remaining: usize,
    },
    OptionTag {
        field: String,
        tag: Option<u8>,
        length: usize,
    },
    TrailingBytes {
        offset: usize,
        total: usize,
    },
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffsetOverflow { .. } => f.write_str("field offset or length overflow"),
            Self::TruncatedHeader { .. } => f.write_str("truncated field header"),
            Self::LengthOverflow { .. } => f.write_str("field length overflow"),
            Self::TruncatedPayload { .. } => f.write_str("truncated field payload"),
            Self::OptionTag { field, .. } => write!(f, "invalid Option encoding for {field}"),
            Self::TrailingBytes { .. } => f.write_str("trailing bytes in contract value"),
        }
    }
}
impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LengthOverflow { cause, .. } => Some(cause),
            _ => None,
        }
    }
}
impl CodecError {
    pub fn failure(self) -> Failure {
        let field = match &self {
            Self::OffsetOverflow { field, .. }
            | Self::TruncatedHeader { field, .. }
            | Self::LengthOverflow { field, .. }
            | Self::TruncatedPayload { field, .. }
            | Self::OptionTag { field, .. } => Some(field.clone()),
            Self::TrailingBytes { .. } => None,
        };
        Failure::external(format!("{self}"), self)
            .with_kind(FailureKind::Framework)
            .with_context(crate::FailureContext {
                field,
                phase: Some(CallbackPhase::Decode),
                ..Default::default()
            })
    }
}

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
    value: Option<Arc<T>>,
    // Install typed cleanup at construction without adding bounds to Drop/Clone.
    dispose: fn(Option<Arc<T>>) -> bool,
    leases: Vec<Arc<dyn Send + Sync>>,
}
impl<T> Clone for Leased<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            dispose: self.dispose,
            leases: self.leases.clone(),
        }
    }
}
impl<T> Deref for Leased<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_deref().expect("live leased value")
    }
}
impl<T: fmt::Debug> fmt::Debug for Leased<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Leased").field(&self.value).finish()
    }
}
impl<T> Leased<T> {
    pub fn dispose_catching(mut self) -> bool {
        self.dispose_contents()
    }
    fn dispose_contents(&mut self) -> bool {
        let mut panicked = (self.dispose)(self.value.take());
        while let Some(lease) = self.leases.pop() {
            panicked |= drop_catching(lease);
        }
        panicked
    }
}
impl<T> Drop for Leased<T> {
    fn drop(&mut self) {
        let _ = self.dispose_contents();
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
    fn decode(bytes: &[u8]) -> Result<Self, Failure>;
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
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let arr: [u8; 8] = bytes.try_into().map_err(|cause| {
            ValueError::Length {
                value_type: "i64",
                expected: 8,
                actual: bytes.len(),
                cause: Some(cause),
            }
            .failure()
        })?;
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
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        String::from_utf8(bytes.to_vec()).map_err(|e| {
            Failure::external("invalid UTF-8 value", e).with_kind(FailureKind::Framework)
        })
    }
}
macro_rules! integer_contract {
    ($($ty:ty => $schema:literal),* $(,)?) => {$ (
        impl ContractValue for $ty {
            fn schema() -> Schema {
                Schema { name: $schema.into(), revision: 1, structure: stringify!($ty).into(), codec: concat!(stringify!($ty), "-le-v1").into() }
            }
            fn encode(&self) -> Vec<u8> { self.to_le_bytes().to_vec() }
            fn decode(bytes: &[u8]) -> Result<Self, Failure> {
                let array: [u8; std::mem::size_of::<Self>()] = bytes.try_into()
                    .map_err(|cause| ValueError::Length { value_type: stringify!($ty), expected: std::mem::size_of::<Self>(), actual: bytes.len(), cause: Some(cause) }.failure())?;
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
/// Ordered length-framed sequence. Every item carries its element contract
/// through the sequence schema; even empty element encodings consume a header.
impl<T: ContractValue> ContractValue for Vec<T> {
    fn schema() -> Schema {
        Schema {
            name: "phenix.sequence".into(),
            revision: 1,
            structure: T::schema().canonical(),
            codec: "phenix-sequence-le64-v1".into(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for item in self {
            let item = item.encode();
            bytes.extend_from_slice(
                &u64::try_from(item.len()).expect("item too large").to_le_bytes(),
            );
            bytes.extend_from_slice(&item);
        }
        bytes
    }

    fn decode(mut bytes: &[u8]) -> Result<Self, Failure> {
        let mut values = Vec::new();
        while !bytes.is_empty() {
            let header = bytes
                .get(..8)
                .ok_or_else(|| Failure::application_message("truncated sequence header"))?;
            let length = usize::try_from(u64::from_le_bytes(header.try_into().unwrap()))
                .map_err(|_| Failure::application_message("sequence length overflow"))?;
            bytes = &bytes[8..];
            let item = bytes
                .get(..length)
                .ok_or_else(|| Failure::application_message("truncated sequence item"))?;
            values.push(T::decode(item)?);
            bytes = &bytes[length..];
        }
        Ok(values)
    }
}

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
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        match bytes {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err(ValueError::InvalidBool {
                length: bytes.len(),
                tag: bytes.first().copied(),
            }
            .failure()),
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
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.is_empty() {
            Ok(())
        } else {
            Err(ValueError::Length {
                value_type: "unit",
                expected: 0,
                actual: bytes.len(),
                cause: None,
            }
            .failure())
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
    pub(crate) fn known_leases(&self) -> &[Arc<dyn Send + Sync>] {
        &self.leases
    }

    /// Attach ownership of code/resources required by this payload and its clones.
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        if !self.leases.iter().any(|held| Arc::ptr_eq(held, &lease)) {
            self.leases.push(lease);
        }
        self
    }
    /// Copy known code/resource leases without retaining the source payload.
    pub fn retain_leases_from(&mut self, source: &Self) {
        self.retain_known_leases(&source.leases);
    }
    pub(crate) fn retain_known_leases(&mut self, leases: &[Arc<dyn Send + Sync>]) {
        for lease in leases {
            if !self.leases.iter().any(|held| Arc::ptr_eq(held, lease)) {
                self.leases.push(lease.clone());
            }
        }
    }
    /// Move a unique local leaf through reconstruction without detaching its
    /// leases. `self` pins them while `finish` runs, including error/unwind paths.
    pub(crate) fn map_local<T: Send + Sync + 'static>(
        mut self,
        finish: impl FnOnce(T) -> Result<Value, Failure>,
    ) -> Result<Value, Failure> {
        let actual = self.identity();
        let result = (|| {
            let data = self
                .local
                .take()
                .ok_or_else(|| ValueError::EncodedNotLocal.failure())?;
            let typed = Arc::downcast::<T>(data).map_err(|data| {
                let failure = ValueError::LocalType {
                    expected: type_name::<T>(),
                    actual: actual.clone(),
                }
                .failure();
                if drop_catching(data) {
                    failure.with_cleanup(
                        ValueError::DestructorPanicked { actual }
                            .failure()
                            .with_kind(FailureKind::Cleanup),
                    )
                } else {
                    failure
                }
            })?;
            let payload = Arc::try_unwrap(typed).map_err(|data| {
                let _ = drop_catching(data);
                ValueError::SharedInput.failure()
            })?;
            self.map_callback(|| finish(payload))
        })();
        result.map_err(|failure| failure.retain_leases(&self.leases))
    }
    pub(crate) fn map_callback(
        &self,
        finish: impl FnOnce() -> Result<Value, Failure>,
    ) -> Result<Value, Failure> {
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(finish)) {
            Ok(result) => result,
            Err(payload) => Err(crate::process::callback_panicked(
                CallbackPhase::Decode,
                payload,
            )),
        };
        result
            .map(|mut value| {
                value.retain_leases_from(self);
                value
            })
            .map_err(|failure| failure.retain_leases(&self.leases))
    }
    pub(crate) fn finish_owned(self, result: Result<Value, Failure>) -> Result<Value, Failure> {
        let actual = self.identity();
        let failures = self.dispose_with_cleanup(|| {
            ValueError::DestructorPanicked {
                actual: actual.clone(),
            }
            .failure()
            .with_kind(FailureKind::Cleanup)
        });
        let mut failures = failures.into_iter();
        let Some(failure) = failures.next() else {
            return result;
        };
        let failure = failures.fold(failure, Failure::with_cleanup);
        Err(match result {
            Err(primary) => primary.with_cleanup(failure),
            Ok(value) => {
                let actual = value.identity();
                value
                    .dispose_with_cleanup(|| {
                        ValueError::DestructorPanicked {
                            actual: actual.clone(),
                        }
                        .failure()
                        .with_kind(FailureKind::Cleanup)
                    })
                    .into_iter()
                    .fold(failure, Failure::with_cleanup)
            }
        })
    }
    fn require_unleased(&self) -> Result<(), Failure> {
        if self.leases.is_empty() {
            Ok(())
        } else {
            Err(ValueError::LeasedOwnership
                .failure()
                .retain_leases(&self.leases))
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
    pub fn shared<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, Failure> {
        self.require_unleased()?;
        self.local_shared()
    }
    fn local_shared<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, Failure> {
        let result = self
            .local
            .as_ref()
            .ok_or_else(|| ValueError::EncodedNotLocal.failure())
            .and_then(|data| {
                Arc::downcast::<T>(Arc::clone(data)).map_err(|_| {
                    ValueError::LocalType {
                        expected: type_name::<T>(),
                        actual: self.identity(),
                    }
                    .failure()
                })
            });
        result.map_err(|failure| failure.retain_leases(&self.leases))
    }
    /// Consume an unleased local envelope, returning shared payload ownership.
    /// Leased values reject bare extraction; use `leased_shared` instead.
    pub fn into_shared<T: Send + Sync + 'static>(mut self) -> Result<Arc<T>, Failure> {
        let result = self
            .require_unleased()
            .and_then(|()| self.local_shared::<T>());
        match result {
            Ok(value) => {
                let _ = drop_catching(self.local.take());
                Ok(value)
            }
            Err(failure) => {
                let actual = self.identity();
                Err(self
                    .dispose_with_cleanup(|| {
                        ValueError::DestructorPanicked {
                            actual: actual.clone(),
                        }
                        .failure()
                        .with_kind(FailureKind::Cleanup)
                    })
                    .into_iter()
                    .fold(failure, Failure::with_cleanup))
            }
        }
    }
    /// Unwrap or clone an unleased local payload. Leased values return an error.
    pub fn into_owned<T: Clone + Send + Sync + 'static>(self) -> Result<T, Failure> {
        let shared = self.into_shared::<T>()?;
        Ok(Arc::try_unwrap(shared).unwrap_or_else(|value| (*value).clone()))
    }
    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }
    /// Decode an unleased wire payload. For leased values use `decoded` instead.
    pub fn get<T: ContractValue>(&self) -> Result<T, Failure> {
        self.require_unleased()?;
        self.decode()
    }
    /// Shared local data with all leases retained, including for non-Clone `T`.
    pub fn leased_shared<T: Send + Sync + 'static>(&self) -> Result<Leased<T>, Failure> {
        Ok(Leased {
            value: Some(self.local_shared()?),
            dispose: |value| dispose_local(value.map(|value| value as Arc<dyn Any + Send + Sync>)),
            leases: self.leases.clone(),
        })
    }
    /// Decode canonical wire data while retaining all code/resource leases.
    pub fn decoded<T: ContractValue>(&self) -> Result<Leased<T>, Failure> {
        Ok(Leased {
            value: Some(Arc::new(self.decode()?)),
            dispose: |value| dispose_local(value.map(|value| value as Arc<dyn Any + Send + Sync>)),
            leases: self.leases.clone(),
        })
    }
    fn decode<T: ContractValue>(&self) -> Result<T, Failure> {
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let expected = T::schema().identity();
            if self.schema != expected {
                return Err(ValueError::Schema {
                    expected,
                    actual: self.schema.clone(),
                }
                .failure());
            }
            if self.is_local() {
                return Err(ValueError::LocalNotEncoded.failure());
            }
            T::decode(&self.bytes)
        })) {
            Ok(result) => result,
            Err(payload) => Err(crate::process::callback_panicked(
                CallbackPhase::Decode,
                payload,
            )),
        };
        result.map_err(|failure| {
            let mut context = failure.context().clone();
            context.phase.get_or_insert(CallbackPhase::Decode);
            failure.with_context(context).retain_leases(&self.leases)
        })
    }

    /// Explicit managed cleanup reports payload and lease destructor panics.
    pub fn dispose_catching(mut self) -> bool {
        self.dispose_contents()
    }

    pub(crate) fn dispose_with_cleanup(self, cleanup: impl Fn() -> Failure) -> Vec<Failure> {
        let mut leases = self.leases.clone();
        let mut failures = Vec::new();
        if self.dispose_catching() {
            failures.push(cleanup().retain_leases(&leases));
        }
        // Snapshots pin code through payload and caught panic-payload cleanup.
        // Their final releases must be individually contained as well.
        while let Some(lease) = leases.pop() {
            if drop_catching(lease) {
                failures.push(cleanup());
            }
        }
        failures
    }

    fn dispose_contents(&mut self) -> bool {
        let mut panicked = dispose_local(self.local.take());
        while let Some(lease) = self.leases.pop() {
            panicked |= drop_catching(lease);
        }
        panicked
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let _ = self.dispose_contents();
    }
}

fn dispose_local(local: Option<Arc<dyn Any + Send + Sync>>) -> bool {
    match local {
        Some(local) if local.is::<Failure>() => {
            Arc::into_inner(Arc::downcast::<Failure>(local).expect("checked Failure"))
                .is_some_and(Failure::dispose_catching)
        }
        local => drop_catching(local),
    }
}

fn drop_catching<T>(value: T) -> bool {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value))) {
        crate::discard_panic_payload(payload);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod sequence_codec_tests {
    use super::*;

    #[test]
    fn sequence_roundtrip_includes_empty_elements_and_rejects_invalid_frames() {
        let values = vec![String::new(), "hello".to_owned(), String::new()];
        let encoded = values.encode();
        assert_eq!(Vec::<String>::decode(&encoded).unwrap(), values);
        assert_ne!(Vec::<String>::schema(), Vec::<i64>::schema());
        assert_eq!(Vec::<()>::decode(&vec![(); 3].encode()).unwrap().len(), 3);
        assert!(Vec::<String>::decode(&[1, 2, 3]).is_err());
        assert!(Vec::<String>::decode(&u64::MAX.to_le_bytes()).is_err());
        assert!(Vec::<String>::decode(&encoded[..encoded.len() - 1]).is_err());
    }
}
