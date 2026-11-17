//! Typed names, not authority or owner proofs. A key can be used with any erased
//! `ComponentRef`; normal lookup and builder validation still check the actual
//! interface, direction, identity, scope and revision.
//!
//! Payload mismatch is rejected before a graph can be constructed:
//! ```compile_fail,E0308
//! use phenix_graph::{ComponentRef, NetworkBuilder, InputKey, OutputKey};
//! fn connect(builder: &mut NetworkBuilder, a: &ComponentRef, b: &ComponentRef) {
//!     builder.connect(a.output_key(OutputKey::<i64>::new("out")).unwrap(),
//!         b.input_key(InputKey::<String>::new("in")).unwrap(),
//!         std::num::NonZeroUsize::new(1).unwrap()).unwrap();
//! }
//! ```
//! Direction is part of the key type:
//! ```compile_fail,E0308
//! use phenix_graph::{ComponentRef, OutputKey};
//! fn wrong(component: &ComponentRef) {
//!     component.input_key(OutputKey::<i64>::new("value"));
//! }
//! ```
//! Every field has exactly one direction:
//! ```compile_fail
//! use phenix_graph::Ports;
//! #[derive(Ports)]
//! struct Invalid { #[input] #[output] value: i64 }
//! ```
//! Uppercase constant collisions are diagnosed:
//! ```compile_fail
//! use phenix_graph::Ports;
//! #[derive(Ports)]
//! struct Invalid { #[input] value: i64, #[output] VALUE: i64 }
//! ```
//! Encoded fields require a codec, including on generic instantiations:
//! ```compile_fail
//! use phenix_graph::Ports;
//! #[derive(Ports)]
//! struct Encoded<T> { #[input(encoded)] value: T }
//! struct LocalOnly;
//! let _ = Encoded::<LocalOnly>::interface();
//! ```
use crate::network::{ComponentRef, InputRef, NetworkError, OutputRef};
use crate::{
    ContractValue, Failure, FailureKind, Interface, InterfaceError, PortDef, ProcessContext,
    TypeIdentity, Value,
};
use std::borrow::Cow;
use std::marker::PhantomData;

type Decoder<T> = fn(&Value) -> Result<T, Failure>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputError {
    Missing {
        field: String,
    },
    WrongType {
        field: String,
        expected: TypeIdentity,
        actual: TypeIdentity,
    },
}
impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "input reconstruction: {self:?}")
    }
}
impl std::error::Error for InputError {}

enum Mode<R> {
    Local,
    Encoded {
        lookup: fn(&ComponentRef, &str) -> Result<R, NetworkError>,
        definition: fn(&str) -> Result<PortDef, InterfaceError>,
    },
}
impl<R> Copy for Mode<R> {}
impl<R> Clone for Mode<R> {
    fn clone(&self) -> Self {
        *self
    }
}

/// A typed input name. `new` describes a local port; `encoded` uses the complete
/// `ContractValue::schema()` identity. Neither constructor grants access.
pub struct InputKey<T> {
    name: &'static str,
    mode: Mode<InputRef<T>>,
    decode: Option<Decoder<T>>,
    encode: Option<fn(T) -> Value>,
}
impl<T> Copy for InputKey<T> {}
impl<T> Clone for InputKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> InputKey<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            mode: Mode::Local,
            decode: None,
            encode: None,
        }
    }
    pub const fn name(&self) -> &'static str {
        self.name
    }
    pub fn definition(&self) -> Result<PortDef, InterfaceError> {
        match self.mode {
            Mode::Local => Ok(PortDef::local::<T>(self.name)),
            Mode::Encoded { definition, .. } => definition(self.name),
        }
    }
}
impl<T: ContractValue> InputKey<T> {
    pub const fn encoded(name: &'static str) -> Self {
        Self {
            name,
            mode: Mode::Encoded {
                lookup: ComponentRef::wire_input::<T>,
                definition: wire_definition::<T>,
            },
            decode: Some(|value| T::decode(value.bytes())),
            encode: Some(Value::of::<T>),
        }
    }
}

/// A typed output name, distinct from an input key even for the same payload.
pub struct OutputKey<T> {
    name: &'static str,
    mode: Mode<OutputRef<T>>,
}
impl<T> Copy for OutputKey<T> {}
impl<T> Clone for OutputKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> OutputKey<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            mode: Mode::Local,
        }
    }
    pub const fn name(&self) -> &'static str {
        self.name
    }
    pub fn definition(&self) -> Result<PortDef, InterfaceError> {
        match self.mode {
            Mode::Local => Ok(PortDef::local::<T>(self.name)),
            Mode::Encoded { definition, .. } => definition(self.name),
        }
    }
}
impl<T: ContractValue> OutputKey<T> {
    pub const fn encoded(name: &'static str) -> Self {
        Self {
            name,
            mode: Mode::Encoded {
                lookup: ComponentRef::wire_output::<T>,
                definition: wire_definition::<T>,
            },
        }
    }
}

impl ComponentRef {
    pub fn input_key<T: 'static>(&self, key: InputKey<T>) -> Result<InputRef<T>, NetworkError> {
        match key.mode {
            Mode::Local => self.input::<T>(key.name),
            Mode::Encoded { lookup, .. } => lookup(self, key.name),
        }
    }
    pub fn output_key<T: 'static>(&self, key: OutputKey<T>) -> Result<OutputRef<T>, NetworkError> {
        match key.mode {
            Mode::Local => self.output::<T>(key.name),
            Mode::Encoded { lookup, .. } => lookup(self, key.name),
        }
    }
}

fn wire_definition<T: ContractValue>(name: &str) -> Result<PortDef, InterfaceError> {
    PortDef::wire::<T>(name)
}

fn path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else {
        format!("{prefix}.{name}")
    }
}

/// An input-only record whose leaves are real named ports. Implementations must
/// use the same keys for metadata and reconstruction. Reconstruction consumes
/// only the named leaves from this reservation; it does not select queue slots.
/// Encoded and explicit Arc leaves copy their source's known code/resource leases
/// on both success and error, without retaining the source envelope.
/// Unique local leaves move through reconstruction while retaining their leases.
///
/// `map_inputs` is a scoped transformation: do not detach raw payloads through
/// side effects in the callback. The returned Value retains the source leases.
///
/// ```
/// # #[cfg(feature = "derive")] {
/// use phenix_graph::{Ports, ProcessContext, Value};
/// #[derive(Ports)]
/// struct Inner<T> { #[input(name = "value")] r#type: T }
/// #[derive(Ports)]
/// struct Request { #[input(flatten)] request: Inner<String> }
/// let mut context = ProcessContext::new(
///     [("request.value".into(), Value::local(String::from("hello")))].into(), vec![]);
/// let value = context.input_record::<Request>()?;
/// assert_eq!(value.borrow::<Request>().unwrap().request.r#type, "hello");
/// # }
/// # Ok::<(), phenix_graph::Failure>(())
/// ```
/// Reconstruction requires sendable records, not just named type descriptors:
/// ```compile_fail
/// use phenix_graph::{Ports, ProcessContext};
/// #[derive(Ports)]
/// struct Record<T> { #[input] value: T }
/// let mut context = ProcessContext::default();
/// context.input_record::<Record<std::rc::Rc<i64>>>();
/// ```
pub trait InputRecord: Sized + Send + Sync + 'static {
    fn input_ports() -> Result<Vec<PortDef>, InterfaceError>;
    fn map_inputs(
        context: &mut ProcessContext,
        prefix: &str,
        finish: impl FnOnce(&mut ProcessContext, Self) -> Result<Value, Failure>,
    ) -> Result<Value, Failure>;

    /// Assemble one record as a local envelope, retaining all consumed leases.
    /// Missing or shared move-only inputs return an error, never an implicit
    /// clone. Failure may consume preceding leaves, like other process errors.
    fn from_inputs(context: &mut ProcessContext) -> Result<Value, Failure> {
        // Validate duplicate flattened paths before consuming any inputs.
        Interface::new(
            Self::input_ports().map_err(|error| {
                Failure::external("invalid input record ports", error)
                    .with_kind(FailureKind::Framework)
            })?,
            [],
        )
        .map_err(|error| {
            Failure::external("invalid input record interface", error)
                .with_kind(FailureKind::Framework)
        })?;
        Self::map_inputs(context, "", |_, record| Ok(Value::local(record)))
    }
}

impl ProcessContext {
    pub fn input_record<T: InputRecord>(&mut self) -> Result<Value, Failure> {
        T::from_inputs(self)
    }
}

/// A namespace for a flattened input record, not a port carrying that record.
/// `group.input_key(&component, Inner::LEAF)` looks up a typed leaf using its
/// full path. Nested `group` composition preserves each leaf's local/wire mode.
pub struct InputGroup<T> {
    name: Cow<'static, str>,
    record: PhantomData<fn() -> T>,
}
impl<T> Clone for InputGroup<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            record: PhantomData,
        }
    }
}
impl<T: InputRecord> InputGroup<T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            record: PhantomData,
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn group<U: InputRecord>(&self, child: InputGroup<U>) -> InputGroup<U> {
        InputGroup {
            name: Cow::Owned(path(&self.name, &child.name)),
            record: PhantomData,
        }
    }
    pub fn input_key<U: 'static>(
        &self,
        component: &ComponentRef,
        key: InputKey<U>,
    ) -> Result<InputRef<U>, NetworkError> {
        let name = path(&self.name, key.name);
        match key.mode {
            Mode::Local => component.input::<U>(&name),
            Mode::Encoded { lookup, .. } => lookup(component, &name),
        }
    }
    pub fn ports(&self) -> Result<Vec<PortDef>, InterfaceError> {
        Ok(T::input_ports()?
            .into_iter()
            .map(|mut port| {
                port.name = path(&self.name, &port.name);
                port
            })
            .collect())
    }
    pub fn map_inputs(
        &self,
        context: &mut ProcessContext,
        prefix: &str,
        finish: impl FnOnce(&mut ProcessContext, T) -> Result<Value, Failure>,
    ) -> Result<Value, Failure> {
        T::map_inputs(context, &path(prefix, &self.name), finish)
    }
}

impl<T: Send + Sync + 'static> InputKey<T> {
    /// Construct a local value for `new`, or the canonical wire envelope for
    /// `encoded`. Local payloads require neither Clone nor a codec.
    ///
    /// Conversion runs before admission and may panic, block or reenter through
    /// application codec code. Rejection returns this Value, not the original T.
    /// Keys grant no authority and infer no code/resource lease. To attach one,
    /// use `key.value(payload).with_lease(lease)` with raw admission.
    /// ```
    /// use phenix_graph::InputKey;
    /// struct MoveOnly(String);
    /// let value = InputKey::<MoveOnly>::new("input").value(MoveOnly("hello".into()));
    /// assert_eq!(value.borrow::<MoveOnly>().unwrap().0, "hello");
    /// let wire = InputKey::<i64>::encoded("input").value(7);
    /// assert_eq!(wire.bytes(), 7_i64.to_le_bytes());
    /// ```
    pub fn value(&self, payload: T) -> Value {
        match self.encode {
            Some(encode) => encode(payload),
            None => Value::local(payload),
        }
    }

    fn take(&self, context: &mut ProcessContext, prefix: &str) -> Result<Value, Failure> {
        let name = path(prefix, self.name);
        let expected = self
            .definition()
            .map_err(|error| {
                Failure::external("invalid input definition", error)
                    .with_kind(FailureKind::Framework)
            })?
            .value;
        let value = context.input(&name).ok_or_else(|| {
            Failure::external(
                "missing input",
                InputError::Missing {
                    field: name.clone(),
                },
            )
            .with_kind(FailureKind::Framework)
            .with_context(crate::FailureContext {
                field: Some(name.clone()),
                ..Default::default()
            })
        })?;
        if value.identity() != expected {
            return Err(Failure::external(
                "incompatible input",
                InputError::WrongType {
                    field: name.clone(),
                    expected,
                    actual: value.identity(),
                },
            )
            .with_kind(FailureKind::Framework)
            .with_context(crate::FailureContext {
                field: Some(name),
                ..Default::default()
            })
            .retain_leases(value.known_leases()));
        }
        Ok(context.take(&name).expect("checked input"))
    }
    /// Scoped leaf transformation used by InputRecord implementations.
    pub fn map_input(
        &self,
        context: &mut ProcessContext,
        prefix: &str,
        finish: impl FnOnce(&mut ProcessContext, T) -> Result<Value, Failure>,
    ) -> Result<Value, Failure> {
        let source = self.take(context, prefix)?;
        let result = if let Some(decode) = self.decode {
            let result = source.map_callback(|| {
                let payload = decode(&source)?;
                finish(context, payload)
            });
            source.finish_owned(result)
        } else {
            source.map_local::<T>(|payload| finish(context, payload))
        };
        result.map_err(|failure| {
            let mut metadata = failure.context().clone();
            if metadata.field.is_none() {
                metadata.field = Some(path(prefix, self.name));
            }
            failure.with_context(metadata)
        })
    }
}

impl<T: Clone + Send + Sync + 'static> InputKey<T> {
    /// Explicit Arc fields permit fan-out; retain the source envelope's leases.
    pub fn map_shared_input(
        &self,
        context: &mut ProcessContext,
        prefix: &str,
        finish: impl FnOnce(&mut ProcessContext, T) -> Result<Value, Failure>,
    ) -> Result<Value, Failure> {
        let source = self.take(context, prefix)?;
        let result = source.map_callback(|| {
            let payload = source
                .borrow::<T>()
                .ok_or_else(|| {
                    crate::ValueError::LocalType {
                        expected: std::any::type_name::<T>(),
                        actual: source.identity(),
                    }
                    .failure()
                })?
                .clone();
            finish(context, payload)
        });
        source.finish_owned(result).map_err(|failure| {
            let mut metadata = failure.context().clone();
            if metadata.field.is_none() {
                metadata.field = Some(path(prefix, self.name));
            }
            failure.with_context(metadata)
        })
    }
}
