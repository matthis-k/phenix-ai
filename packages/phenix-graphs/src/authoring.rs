//! Inference-first, macro-optional graph authoring. This layer uses the same
//! ModuleDefBuilder / ModuleCatalog compiler and the same Engine as low-level graphs.
//! No second scheduler, contract system, or runtime component wrapper.
use super::*;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Choose how a domain input is presented as slots without changing its type.
pub struct Layout<T>(PhantomData<fn() -> T>);
/// A zero-sized choice that only affects the registered input-port layout.
pub struct LayoutMode<A, L: InputLayout<Value = A>>(PhantomData<fn() -> (A, L)>);

/// A node's execution/serialization semantics are selected at its declaration.
/// All variants enter the same `GraphBuilder::node` path and produce the same
/// `NodeTemplate` consumed by the existing compiler. No alternate node kind.
pub struct Node;
pub struct WireNode<F>(F);
pub struct LocalLayoutNode<F, L: InputLayout>(F, LayoutMode<L::Value, L>);
pub struct WireLayoutNode<F, L: InputLayout>(F, LayoutMode<L::Value, L>);

impl Node {
    /// One constructor for ordinary and directed closures. The return type
    /// determines whether the value is broadcast or carries an `Output` choice.
    // The identity constructor preserves existing ergonomic Node::new(handler) authoring.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F>(handler: F) -> F {
        handler
    }
    pub fn wire<F>(handler: F) -> WireNode<F> {
        WireNode(handler)
    }
    pub fn with_layout<F, L: InputLayout>(
        handler: F,
        layout: LayoutMode<L::Value, L>,
    ) -> LocalLayoutNode<F, L> {
        LocalLayoutNode(handler, layout)
    }
    pub fn wire_layout<F, L: InputLayout>(
        handler: F,
        layout: LayoutMode<L::Value, L>,
    ) -> WireLayoutNode<F, L> {
        WireLayoutNode(handler, layout)
    }
}

/// The sole typed node construction contract. Its generics terminate at
/// authoring; the physical graph remains heterogeneous and type-erased.
/// Implementing it is optional: regular closures use the local-value default.
/// Distinguishes ordinary and directed closure signatures at authoring only.
/// Neither marker is a runtime node kind.
pub struct BroadcastReturn;
pub struct DirectedReturn;

/// Authoring-only type identity: a local Rust type or a portable wire schema.
/// This metadata is never an authority grant or a graph execution policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredValue {
    pub schema: &'static str,
    pub local_type: Option<TypeId>,
}
impl DeclaredValue {
    pub fn local<T: 'static>() -> Self {
        Self {
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
        }
    }
    pub fn wire<T: ContractValue>() -> Self {
        Self {
            schema: T::wire_schema(),
            local_type: None,
        }
    }
}
/// One typed input and one typed output; no positional booleans or runtime variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeSignature {
    pub input: DeclaredValue,
    pub output: DeclaredValue,
}
pub trait NodeDefinition<A: Send + Sync + 'static, O: Send + Sync + 'static, Mode = BroadcastReturn>
{
    type Layout;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError>;
    fn signature() -> NodeSignature;
}
impl<A, O, F> NodeDefinition<A, O> for F
where
    A: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
    F: Fn(A) -> O + Send + Sync + 'static,
{
    type Layout = AtomicInput;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::local_function::<A, O, O, _>(
            move |value: A| (self.as_ref())(value),
        ))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<A>(),
            output: DeclaredValue::local::<O>(),
        }
    }
}
impl<A, O, F> NodeDefinition<A, O, DirectedReturn> for F
where
    A: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
    F: Fn(A) -> crate::output::Output<O> + Send + Sync + 'static,
{
    type Layout = AtomicInput;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::local_function::<
            A,
            O,
            crate::output::Output<O>,
            _,
        >(move |value: A| (self.as_ref())(value)))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<A>(),
            output: DeclaredValue::local::<O>(),
        }
    }
}
impl<A, O, F> NodeDefinition<A, O> for WireNode<F>
where
    A: ContractValue,
    O: ContractValue,
    F: Fn(A) -> O + Send + Sync + 'static,
{
    type Layout = AtomicInput;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::function(move |value: A| (self.0)(value)))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::wire::<A>(),
            output: DeclaredValue::wire::<O>(),
        }
    }
}
impl<L, O, F> NodeDefinition<L::Value, O> for LocalLayoutNode<F, L>
where
    L: LocalInputLayout,
    L::Value: Send + Sync + 'static,
    O: Send + Sync + 'static,
    F: Fn(L::Value) -> O + Send + Sync + 'static,
{
    type Layout = L;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<L, O, O, _>(move |value: L::Value| (self.0)(value))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<L::Value>(),
            output: DeclaredValue::local::<O>(),
        }
    }
}
// A layout only describes input preparation. Routing belongs to the return
// value, exactly as for an atomic-input node; no layout-specific selector.
impl<L, O, F> NodeDefinition<L::Value, O, DirectedReturn> for LocalLayoutNode<F, L>
where
    L: LocalInputLayout,
    L::Value: Send + Sync + 'static,
    O: Send + Sync + 'static,
    F: Fn(L::Value) -> crate::output::Output<O> + Send + Sync + 'static,
{
    type Layout = L;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<L, O, crate::output::Output<O>, _>(
            move |value: L::Value| (self.0)(value),
        )
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<L::Value>(),
            output: DeclaredValue::local::<O>(),
        }
    }
}
impl<L, O, F> NodeDefinition<L::Value, O> for WireLayoutNode<F, L>
where
    L: DecodedInputLayout,
    L::Value: Send + Sync + 'static,
    O: ContractValue,
    F: Fn(L::Value) -> O + Send + Sync + 'static,
{
    type Layout = L;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::with_input_layout::<L, O, _>("run", AllSameCohort, move |value: L::Value| {
            (self.0)(value)
        })
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue {
                schema: type_name::<L::Value>(),
                local_type: None,
            },
            output: DeclaredValue::wire::<O>(),
        }
    }
}
/// Reconfigure a typed node's input slot layout while preserving its handler.
/// Only local closures participate: a layout never chooses wire encoding or
/// reinterprets a node's return value. The compiler owns the final connections.
pub trait LocalLayoutRetarget<
    A: Send + Sync + 'static,
    O: Send + Sync + 'static,
    Mode,
    L: LocalInputLayout<Value = A>,
>: NodeDefinition<A, O, Mode>
{
    fn with_input_layout(self: Arc<Self>) -> Result<NodeTemplate, LayoutError>;
}
impl<A, O, F, L> LocalLayoutRetarget<A, O, BroadcastReturn, L> for F
where
    A: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
    L: LocalInputLayout<Value = A>,
    F: Fn(A) -> O + Send + Sync + 'static,
{
    fn with_input_layout(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<L, O, O, _>(move |value: A| (self.as_ref())(value))
    }
}
impl<A, O, F, L> LocalLayoutRetarget<A, O, DirectedReturn, L> for F
where
    A: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
    L: LocalInputLayout<Value = A>,
    F: Fn(A) -> crate::output::Output<O> + Send + Sync + 'static,
{
    fn with_input_layout(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<L, O, crate::output::Output<O>, _>(move |value: A| {
            (self.as_ref())(value)
        })
    }
}
// Layout-specific authoring helpers can be reconfigured through the same
// typed handle. They do not remain a second mutable declaration mechanism.
impl<A, O, F, Prior, L> LocalLayoutRetarget<A, O, BroadcastReturn, L> for LocalLayoutNode<F, Prior>
where
    A: Send + Sync + 'static,
    O: Send + Sync + 'static,
    Prior: LocalInputLayout<Value = A>,
    L: LocalInputLayout<Value = A>,
    F: Fn(A) -> O + Send + Sync + 'static,
{
    fn with_input_layout(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<L, O, O, _>(move |value: A| (self.0)(value))
    }
}
impl<A, O, F, Prior, L> LocalLayoutRetarget<A, O, DirectedReturn, L> for LocalLayoutNode<F, Prior>
where
    A: Send + Sync + 'static,
    O: Send + Sync + 'static,
    Prior: LocalInputLayout<Value = A>,
    L: LocalInputLayout<Value = A>,
    F: Fn(A) -> crate::output::Output<O> + Send + Sync + 'static,
{
    fn with_input_layout(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<L, O, crate::output::Output<O>, _>(move |value: A| {
            (self.0)(value)
        })
    }
}

impl<T> Layout<T> {
    /// Default graph authoring is local and has no serialization requirement.
    pub fn atomic() -> LayoutMode<T, LocalAtomic<T>>
    where
        T: Send + Sync + 'static,
    {
        LayoutMode(PhantomData)
    }
    pub fn fields() -> LayoutMode<T, LocalFlatten<T>>
    where
        T: LocalPortLayoutAt<1>,
    {
        LayoutMode(PhantomData)
    }
    pub fn flatten_to<const N: usize>() -> LayoutMode<T, LocalFlattenTo<T, N>>
    where
        T: LocalPortLayoutAt<N>,
    {
        LayoutMode(PhantomData)
    }
    pub fn deep() -> LayoutMode<T, LocalDeepFlatten<T>>
    where
        T: LocalPortLayout,
    {
        LayoutMode(PhantomData)
    }

    /// Wire-encoded layouts for values crossing serialization boundaries.
    pub fn wire_atomic() -> LayoutMode<T, Atomic<T>>
    where
        T: ContractValue,
    {
        LayoutMode(PhantomData)
    }
    pub fn wire_fields() -> LayoutMode<T, Flatten<T>>
    where
        T: PortLayoutAt<1>,
    {
        LayoutMode(PhantomData)
    }
    pub fn wire_flatten_to<const N: usize>() -> LayoutMode<T, FlattenTo<T, N>>
    where
        T: PortLayoutAt<N>,
    {
        LayoutMode(PhantomData)
    }
    pub fn wire_deep() -> LayoutMode<T, DeepFlatten<T>>
    where
        T: PortLayout,
    {
        LayoutMode(PhantomData)
    }
}

/// A typed key generated by the optional LocalPortLayout derive. Unlike a raw
/// string, the owner and destination value type must match at compile time.
#[derive(Clone, Copy)]
pub struct SlotField<Owner, T> {
    name: &'static str,
    _type: PhantomData<fn(Owner) -> T>,
}
impl<Owner, T> SlotField<Owner, T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _type: PhantomData,
        }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    /// Construct a nested selector while preserving the parent and leaf types.
    /// The compiler validates this path against the chosen layout on wiring.
    pub fn then<U>(self, child: SlotField<T, U>) -> SlotPath<Owner, U> {
        SlotPath {
            path: format!("{}.{}", self.name, child.name),
            _type: PhantomData,
        }
    }
}
/// Fully typed path through one or more derived structural fields.
#[derive(Clone, Debug)]
pub struct SlotPath<Owner, T> {
    path: String,
    _type: PhantomData<fn(Owner) -> T>,
}
impl<Owner, T> SlotPath<Owner, T> {
    pub fn name(&self) -> &str {
        &self.path
    }
    pub fn then<U>(self, child: SlotField<T, U>) -> SlotPath<Owner, U> {
        SlotPath {
            path: format!("{}.{}", self.path, child.name),
            _type: PhantomData,
        }
    }
}
/// A public, macro-independent path capability; an invalid manual path is
/// always rejected by graph verification before compilation.
pub trait FieldSelector<Owner, T> {
    fn path(&self) -> &str;
}
impl<Owner, T> FieldSelector<Owner, T> for SlotField<Owner, T> {
    fn path(&self) -> &str {
        self.name
    }
}
impl<Owner, T> FieldSelector<Owner, T> for SlotPath<Owner, T> {
    fn path(&self) -> &str {
        &self.path
    }
}

#[derive(Clone)]
enum Source {
    Node { node: String, output: String },
    Instance { name: String, port: String },
}
#[derive(Clone)]
enum Target {
    Node {
        node: String,
        port: String,
        slot: String,
    },
    Instance {
        name: String,
        port: String,
    },
}

/// A typed endpoint from a declaration. The compiler itself remains heterogeneous.
#[derive(Clone)]
pub struct Input<T: Send + Sync + 'static> {
    scope: u64,
    source: Target,
    schema: &'static str,
    local_type: Option<TypeId>,
    _type: PhantomData<fn(T)>,
}
#[derive(Clone)]
pub struct OutputRef<T: Send + Sync + 'static> {
    scope: u64,
    source: Source,
    schema: &'static str,
    local_type: Option<TypeId>,
    _type: PhantomData<fn() -> T>,
}

impl<T: Send + Sync + 'static> Input<T> {
    /// Only a node input or exported module input can be an explicit target.
    /// Resolution happens against *existing outgoing edges* at execution.
    pub(crate) fn dispatch_target(&self) -> crate::output::DestinationTarget {
        match &self.source {
            Target::Node { node, port, slot } => crate::output::DestinationTarget::Node {
                scope: self.scope,
                name: node.clone(),
                port: port.clone(),
                slot: slot.clone(),
            },
            Target::Instance { name, port } => crate::output::DestinationTarget::Module {
                scope: self.scope,
                name: name.clone(),
                port: port.clone(),
            },
        }
    }
}

/// The type parameters are carried only by authoring handles, never by Graph.
pub struct AtomicInput;

/// The layout marker prevents atomic-only helpers from fabricating a slot
/// that does not exist on a structured input port.
pub struct NodeHandle<
    A: Send + Sync + 'static,
    O: Send + Sync + 'static,
    L = AtomicInput,
    D = (),
    Mode = BroadcastReturn,
> {
    scope: u64,
    name: String,
    input_schema: &'static str,
    output_schema: &'static str,
    input_type: Option<TypeId>,
    output_type: Option<TypeId>,
    _type: PhantomData<fn(A) -> O>,
    _layout: PhantomData<L>,
    _mode: PhantomData<fn() -> Mode>,
    definition: Arc<D>,
    pending: Weak<RefCell<BTreeMap<String, NodeTemplate>>>,
}
impl<A: Send + Sync + 'static, O: Send + Sync + 'static, D, Mode>
    NodeHandle<A, O, AtomicInput, D, Mode>
{
    /// Typed atomic input endpoint. `input(layout)` is reserved for layout
    /// configuration; endpoint access is `inlet()` (or `field()` for fields).
    pub fn inlet(&self) -> Input<A> {
        self.atomic_inlet()
    }
}
// An explicitly selected atomic layout still exposes the same atomic input.
// Its actual layout type survives in the declaration handle.
impl<A: Send + Sync + 'static, O: Send + Sync + 'static, D, Mode>
    NodeHandle<A, O, LocalAtomic<A>, D, Mode>
{
    pub fn inlet(&self) -> Input<A> {
        self.atomic_inlet()
    }
}
impl<A: Send + Sync + 'static, O: Send + Sync + 'static, State, D, Mode>
    NodeHandle<A, O, State, D, Mode>
{
    fn atomic_inlet(&self) -> Input<A> {
        Input {
            scope: self.scope,
            source: Target::Node {
                node: self.name.clone(),
                port: "run".into(),
                slot: "value".into(),
            },
            schema: self.input_schema,
            local_type: self.input_type,
            _type: PhantomData,
        }
    }
    /// Configure the existing registered declaration before graph compilation.
    /// The input argument type is checked by Rust (`L::Value = A`).
    ///
    /// ```compile_fail
    /// use phenix_graph::{GraphBuilder, Layout, LocalPortLayout};
    /// #[derive(LocalPortLayout)] struct Other { value: String }
    /// let mut graph = GraphBuilder::new("wrong-layout");
    /// let node = graph.node("n", |x: i64| x + 1).unwrap();
    /// let _ = node.input(Layout::<Other>::fields());
    /// ```
    ///
    /// A structured layout intentionally has no atomic `inlet()`, because
    /// only its individually named fields exist as physical input slots.
    ///
    /// ```compile_fail
    /// use phenix_graph::{GraphBuilder, Layout, LocalPortLayout};
    /// #[derive(Clone, LocalPortLayout)]
    /// struct Pair { left: i64, right: i64 }
    /// let mut graph = GraphBuilder::new("not-atomic");
    /// let node = graph.node("sum", |x: Pair| x.left + x.right).unwrap()
    ///     .input(Layout::fields()).unwrap();
    /// let _ = node.inlet(); // Only field endpoints exist in the selected layout.
    /// ```
    ///
    /// Any edges made incompatible by this change are diagnosed at graph compilation.
    pub fn input<L>(
        self,
        _layout: LayoutMode<A, L>,
    ) -> Result<NodeHandle<A, O, L, D, Mode>, GraphBuildError>
    where
        L: LocalInputLayout<Value = A>,
        D: LocalLayoutRetarget<A, O, Mode, L>,
    {
        // Build and validate the replacement before modifying the declaration.
        let replacement = Arc::clone(&self.definition).with_input_layout()?;
        let pending = self.pending.upgrade().ok_or_else(|| {
            GraphBuildError::Configuration("graph declaration has already been consumed".into())
        })?;
        let mut pending = pending.borrow_mut();
        let slot = pending.get_mut(&self.name).ok_or_else(|| {
            GraphBuildError::Configuration("node declaration has been replaced or removed".into())
        })?;
        // Preserve physical declaration identity and output interface.
        let mut replacement = replacement;
        replacement.id = slot.id;
        replacement.name = slot.name.clone();
        replacement.declaring_scope = slot.declaring_scope;
        *slot = replacement;
        drop(pending);
        Ok(NodeHandle {
            scope: self.scope,
            name: self.name,
            input_schema: self.input_schema,
            output_schema: self.output_schema,
            input_type: self.input_type,
            output_type: self.output_type,
            _type: PhantomData,
            _layout: PhantomData,
            _mode: PhantomData,
            definition: self.definition,
            pending: self.pending,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn output(&self) -> OutputRef<O> {
        OutputRef {
            scope: self.scope,
            source: Source::Node {
                node: self.name.clone(),
                output: "value".into(),
            },
            schema: self.output_schema,
            local_type: self.output_type,
            _type: PhantomData,
        }
    }
    /// Manual typed path for layouts not using generated slot descriptors.
    /// The real declaration is validated during connect/expose, not assumed.
    pub fn slot<T: ContractValue>(&self, path: &str) -> Input<T> {
        Input {
            scope: self.scope,
            source: Target::Node {
                node: self.name.clone(),
                port: "run".into(),
                slot: path.into(),
            },
            schema: T::wire_schema(),
            local_type: None,
            _type: PhantomData,
        }
    }
    /// Typed path into a manually declared local (non-wire) multi-slot port.
    pub fn local_slot<T: Send + Sync + 'static>(&self, path: &str) -> Input<T> {
        Input {
            scope: self.scope,
            source: Target::Node {
                node: self.name.clone(),
                port: "run".into(),
                slot: path.into(),
            },
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        }
    }
    /// Type-checked named field. The derive generates SLOT_FIELD constants.
    pub fn field<T: Send + Sync + 'static, K: FieldSelector<A, T>>(&self, key: K) -> Input<T> {
        Input {
            scope: self.scope,
            source: Target::Node {
                node: self.name.clone(),
                port: "run".into(),
                slot: key.path().into(),
            },
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        }
    }
    /// Explicitly address a named output in a custom multi-output node.
    pub fn output_named<T: ContractValue>(&self, name: &str) -> OutputRef<T> {
        OutputRef {
            scope: self.scope,
            source: Source::Node {
                node: self.name.clone(),
                output: name.into(),
            },
            schema: T::wire_schema(),
            local_type: None,
            _type: PhantomData,
        }
    }
    /// Address any explicitly named local output. The graph validates that
    /// the named output actually has the requested type when it is connected
    /// or exported; a caller cannot forge a differently typed connection.
    pub fn local_output<T: Send + Sync + 'static>(&self, name: &str) -> OutputRef<T> {
        OutputRef {
            scope: self.scope,
            source: Source::Node {
                node: self.name.clone(),
                output: name.into(),
            },
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct PublicInput {
    scope: u64,
    id: u16,
    name: String,
    schema: &'static str,
    local_type: Option<TypeId>,
}
impl PublicInput {
    pub fn id(&self) -> u16 {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn schema(&self) -> &'static str {
        self.schema
    }
}

/// A compiled-module interface is the set of exposed named, typed endpoints.
/// Internal nodes and slots are not accessible from the instance handle.
pub struct ModuleHandle {
    scope: u64,
    name: String,
    inputs: BTreeMap<String, (&'static str, Option<TypeId>)>,
    outputs: BTreeMap<String, (&'static str, Option<TypeId>)>,
}
impl ModuleHandle {
    pub fn input<T: ContractValue>(&self, name: &str) -> Result<Input<T>, String> {
        let actual = self
            .inputs
            .get(name)
            .ok_or_else(|| format!("module input {name:?} not exported"))?;
        if *actual != (T::wire_schema(), None) {
            return Err(format!("module input {name:?} type mismatch"));
        }
        Ok(Input {
            scope: self.scope,
            source: Target::Instance {
                name: self.name.clone(),
                port: name.into(),
            },
            schema: T::wire_schema(),
            local_type: None,
            _type: PhantomData,
        })
    }
    pub fn output<T: ContractValue>(&self, name: &str) -> Result<OutputRef<T>, String> {
        let actual = self
            .outputs
            .get(name)
            .ok_or_else(|| format!("module output {name:?} not exported"))?;
        if *actual != (T::wire_schema(), None) {
            return Err(format!("module output {name:?} type mismatch"));
        }
        Ok(OutputRef {
            scope: self.scope,
            source: Source::Instance {
                name: self.name.clone(),
                port: name.into(),
            },
            schema: T::wire_schema(),
            local_type: None,
            _type: PhantomData,
        })
    }
    pub fn local_input<T: Send + Sync + 'static>(&self, name: &str) -> Result<Input<T>, String> {
        let actual = self
            .inputs
            .get(name)
            .ok_or_else(|| format!("module input {name:?} not exported"))?;
        if *actual != (type_name::<T>(), Some(TypeId::of::<T>())) {
            return Err(format!("module input {name:?} type mismatch"));
        }
        Ok(Input {
            scope: self.scope,
            source: Target::Instance {
                name: self.name.clone(),
                port: name.into(),
            },
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        })
    }
    pub fn local_output<T: Send + Sync + 'static>(
        &self,
        name: &str,
    ) -> Result<OutputRef<T>, String> {
        let actual = self
            .outputs
            .get(name)
            .ok_or_else(|| format!("module output {name:?} not exported"))?;
        if *actual != (type_name::<T>(), Some(TypeId::of::<T>())) {
            return Err(format!("module output {name:?} type mismatch"));
        }
        Ok(OutputRef {
            scope: self.scope,
            source: Source::Instance {
                name: self.name.clone(),
                port: name.into(),
            },
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        })
    }
}

/// A reusable module plus its transitive definitions. Its public type signature
/// is precisely its declared public ports, not a separate Contract object.
pub struct ModulePackage {
    root: ModuleEntry,
    dependencies: Vec<ModuleEntry>,
    identities: BTreeMap<String, u64>,
    compile_limits: CompileLimits,
}
impl ModulePackage {
    pub fn name(&self) -> &str {
        &self.root.definition.id
    }
    pub fn inputs(&self) -> &[ModuleInput] {
        &self.root.definition.inputs
    }
    pub fn outputs(&self) -> &[ModuleOutput] {
        &self.root.definition.outputs
    }
    pub fn compile(self) -> Result<Arc<CompiledGraph>, CompileDiagnostic> {
        let mut catalog = ModuleCatalog::new().limits(self.compile_limits);
        for entry in self.dependencies {
            let id = entry.definition.id.clone();
            catalog.add(entry).map_err(|message| CompileDiagnostic {
                phase: CompilePhase::Declaration,
                definition: id,
                message,
            })?;
        }
        let root = self.root.definition.id.clone();
        catalog
            .add(self.root)
            .map_err(|message| CompileDiagnostic {
                phase: CompilePhase::Declaration,
                definition: root.clone(),
                message,
            })?;
        catalog.compile(&root)
    }
}
fn copy_entry(entry: &ModuleEntry) -> ModuleEntry {
    ModuleEntry {
        definition: entry.definition.clone(),
        triggers: entry.triggers.clone(),
    }
}

/// One declarative builder for functions, nested modules and low-level nodes.
/// Internal IDs and macro-generated sugar remain optional implementation details.
pub struct GraphBuilder {
    module: ModuleDefBuilder,
    // Typed declaration handles stage validated replacement templates here.
    // GraphBuilder owns the strong reference. Handles only hold Weak access,
    // preventing edits after the builder has been consumed.
    pending: Rc<RefCell<BTreeMap<String, NodeTemplate>>>,
    dependencies: BTreeMap<String, ModuleEntry>,
    identities: BTreeMap<String, u64>,
    definition_id: u64,
    trigger_names: BTreeMap<String, u16>,
    compile_limits: CompileLimits,
}

impl Clone for GraphBuilder {
    fn clone(&self) -> Self {
        Self {
            module: self.module.clone(),
            // Patch transactions get an isolated snapshot, not a shared
            // mutable configuration map from the original graph builder.
            pending: Rc::new(RefCell::new(self.pending.borrow().clone())),
            dependencies: self.dependencies.clone(),
            identities: self.identities.clone(),
            definition_id: self.definition_id,
            trigger_names: self.trigger_names.clone(),
            compile_limits: self.compile_limits,
        }
    }
}

/// A replacement kind is used by the patch planner only for its write claim;
/// the graph authoring layer need not import or understand patch targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementKind {
    Node,
    Module,
}
mod replacement_private {
    use super::*;
    pub trait Sealed {}
    impl Sealed for NodeTemplate {}
    impl Sealed for ModulePackage {}
    impl Sealed for &ModulePackage {}
}
/// One replacement operation for either an individual node implementation or
/// a module instance. Interface compatibility is checked before committing.
/// The sealed implementations prevent an arbitrary replacement from editing
/// unrelated graph state and misreporting its patch conflict footprint.
#[allow(private_bounds)]
pub trait Replacement: replacement_private::Sealed {
    fn apply(self, graph: &mut GraphBuilder, name: &str) -> Result<(), String>;
    fn kind() -> ReplacementKind;
}
impl Replacement for NodeTemplate {
    fn apply(self, graph: &mut GraphBuilder, name: &str) -> Result<(), String> {
        graph.replace_node_impl(name, self)
    }
    fn kind() -> ReplacementKind {
        ReplacementKind::Node
    }
}
impl Replacement for ModulePackage {
    fn apply(self, graph: &mut GraphBuilder, name: &str) -> Result<(), String> {
        graph.replace_instance_impl(name, &self)
    }
    fn kind() -> ReplacementKind {
        ReplacementKind::Module
    }
}
impl Replacement for &ModulePackage {
    fn apply(self, graph: &mut GraphBuilder, name: &str) -> Result<(), String> {
        graph.replace_instance_impl(name, self)
    }
    fn kind() -> ReplacementKind {
        ReplacementKind::Module
    }
}
/// The one connection API accepts either typed endpoints or explicit symbolic
/// paths. Implementations are sealed to ensure callers cannot bypass the
/// graph's existing type and scope checks.
#[allow(private_bounds)] // Intentionally sealed: only validated endpoints can enter the graph.
pub trait ConnectEndpoints: private_endpoints::Sealed {
    fn connect(self, graph: &mut GraphBuilder) -> Result<&mut GraphBuilder, String>;
    fn disconnect(self, graph: &mut GraphBuilder) -> Result<&mut GraphBuilder, String>;
}
/// The one public-input API accepts a typed handle or a symbolic path.
#[allow(private_bounds)] // Intentionally sealed: only validated endpoints can enter the graph.
pub trait InputEndpoint: private_endpoints::Sealed {
    fn expose(self, graph: &mut GraphBuilder, name: String) -> Result<PublicInput, String>;
}
/// The one public-output API accepts a typed handle or a symbolic path.
#[allow(private_bounds)] // Intentionally sealed: only validated endpoints can enter the graph.
pub trait OutputEndpoint: private_endpoints::Sealed {
    fn expose(self, graph: &mut GraphBuilder, name: String) -> Result<(), String>;
}
/// The one trigger API accepts exported handles or their public names.
#[allow(private_bounds)] // Intentionally sealed: only validated endpoints can enter the graph.
pub trait TriggerEndpoint: private_endpoints::Sealed {
    fn resolve(self, graph: &GraphBuilder) -> Result<PublicInput, String>;
}
mod private_endpoints {
    use super::*;
    pub trait Sealed {}
    impl<T: Send + Sync + 'static> Sealed for (OutputRef<T>, Input<T>) {}
    impl Sealed for (&str, &str) {}
    impl<T: Send + Sync + 'static> Sealed for Input<T> {}
    impl<T: Send + Sync + 'static> Sealed for OutputRef<T> {}
    impl Sealed for &str {}
    impl Sealed for PublicInput {}
}
impl<T: Send + Sync + 'static> ConnectEndpoints for (OutputRef<T>, Input<T>) {
    fn connect(self, graph: &mut GraphBuilder) -> Result<&mut GraphBuilder, String> {
        graph.connect_typed(self.0, self.1)
    }
    fn disconnect(self, graph: &mut GraphBuilder) -> Result<&mut GraphBuilder, String> {
        graph.disconnect_typed(self.0, self.1)?;
        Ok(graph)
    }
}
impl ConnectEndpoints for (&str, &str) {
    fn connect(self, graph: &mut GraphBuilder) -> Result<&mut GraphBuilder, String> {
        graph.connect_symbolic(self.0, self.1)
    }
    fn disconnect(self, graph: &mut GraphBuilder) -> Result<&mut GraphBuilder, String> {
        graph.disconnect_symbolic(self.0, self.1)
    }
}
impl<T: Send + Sync + 'static> InputEndpoint for Input<T> {
    fn expose(self, graph: &mut GraphBuilder, name: String) -> Result<PublicInput, String> {
        graph.expose_input_typed(name, self)
    }
}
impl InputEndpoint for &str {
    fn expose(self, graph: &mut GraphBuilder, name: String) -> Result<PublicInput, String> {
        graph.expose_input_symbolic(&name, self)
    }
}
impl<T: Send + Sync + 'static> OutputEndpoint for OutputRef<T> {
    fn expose(self, graph: &mut GraphBuilder, name: String) -> Result<(), String> {
        graph.expose_output_typed(name, self)
    }
}
impl OutputEndpoint for &str {
    fn expose(self, graph: &mut GraphBuilder, name: String) -> Result<(), String> {
        graph.expose_output_symbolic(&name, self)
    }
}
impl TriggerEndpoint for PublicInput {
    fn resolve(self, _graph: &GraphBuilder) -> Result<PublicInput, String> {
        Ok(self)
    }
}
impl TriggerEndpoint for &str {
    fn resolve(self, graph: &GraphBuilder) -> Result<PublicInput, String> {
        graph.public_input(self)
    }
}

impl GraphBuilder {
    /// Sole connection verb. Typed endpoints are checked by Rust; symbolic
    /// addresses are checked at authoring time. Neither path bypasses validation.
    pub fn connect<A, B>(&mut self, from: A, to: B) -> Result<&mut Self, String>
    where
        (A, B): ConnectEndpoints,
    {
        (from, to).connect(self)
    }
    pub fn disconnect<A, B>(&mut self, from: A, to: B) -> Result<&mut Self, String>
    where
        (A, B): ConnectEndpoints,
    {
        (from, to).disconnect(self)
    }
    pub fn expose_input<E: InputEndpoint>(
        &mut self,
        name: impl Into<String>,
        to: E,
    ) -> Result<PublicInput, String> {
        to.expose(self, name.into())
    }
    pub fn expose_output<E: OutputEndpoint>(
        &mut self,
        name: impl Into<String>,
        from: E,
    ) -> Result<(), String> {
        from.expose(self, name.into())
    }
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            module: ModuleDefBuilder::new(name),
            pending: Rc::new(RefCell::new(BTreeMap::new())),
            dependencies: BTreeMap::new(),
            identities: BTreeMap::new(),
            definition_id: NEXT_DECLARATION_SCOPE.fetch_add(1, Ordering::Relaxed),
            trigger_names: BTreeMap::new(),
            compile_limits: CompileLimits::default(),
        }
    }
    pub fn limits(mut self, limits: CompileLimits) -> Self {
        self.compile_limits = limits;
        self
    }
    /// Canonical typed node declaration. The returned handle retains the
    /// definition only for type-checked *declaration-time* reconfiguration.
    /// The runtime graph never holds NodeHandle or its authoring generics.
    pub fn node<A, O, D, Mode>(
        &mut self,
        name: impl Into<String>,
        definition: D,
    ) -> Result<NodeHandle<A, O, D::Layout, D, Mode>, GraphBuildError>
    where
        A: Send + Sync + 'static,
        O: Send + Sync + 'static,
        D: NodeDefinition<A, O, Mode> + 'static,
    {
        let signature = D::signature();
        let definition = Arc::new(definition);
        let template = Arc::clone(&definition).template()?;
        let named = self.module.register(name, template)?;
        // Registration assigns the physical authoring ID and declaring scope.
        // The pending copy must retain those canonical identities.
        let registered = self
            .module
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(named.name.as_str()))
            .expect("successful named registration")
            .clone();
        self.pending
            .borrow_mut()
            .insert(named.name.clone(), registered);
        Ok(NodeHandle {
            scope: named.scope,
            name: named.name,
            input_schema: signature.input.schema,
            output_schema: signature.output.schema,
            input_type: signature.input.local_type,
            output_type: signature.output.local_type,
            _type: PhantomData,
            _layout: PhantomData,
            _mode: PhantomData,
            definition,
            pending: Rc::downgrade(&self.pending),
        })
    }
    /// Preserve low-level custom actions/policies and manually declared ports.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        template: NodeTemplate,
    ) -> Result<NamedNodeHandle, ModuleNameError> {
        self.module.register(name, template)
    }

    /// Discover a named local endpoint from a *different contribution*.
    /// This is the patching entry point: handles do not need to be shared
    /// between plugins. Child modules are addressable only through exports.
    pub fn local_input<T: Send + Sync + 'static>(
        &self,
        node_or_instance: &str,
        port: &str,
        slot: &str,
    ) -> Result<Input<T>, String> {
        let source = if self
            .module
            .instances
            .iter()
            .any(|i| i.name == node_or_instance)
        {
            if !slot.is_empty() {
                return Err(
                    "a module public input has no nested slot; pass an empty slot name".into(),
                );
            }
            Target::Instance {
                name: node_or_instance.into(),
                port: port.into(),
            }
        } else {
            Target::Node {
                node: node_or_instance.into(),
                port: port.into(),
                slot: slot.into(),
            }
        };
        let expected = identity(type_name::<T>(), Some(TypeId::of::<T>()));
        self.verify_target(&source, expected)?;
        Ok(Input {
            scope: self.module.scope,
            source,
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        })
    }
    pub fn local_output<T: Send + Sync + 'static>(
        &self,
        node_or_instance: &str,
        output: &str,
    ) -> Result<OutputRef<T>, String> {
        let source = if self
            .module
            .instances
            .iter()
            .any(|i| i.name == node_or_instance)
        {
            Source::Instance {
                name: node_or_instance.into(),
                port: output.into(),
            }
        } else {
            Source::Node {
                node: node_or_instance.into(),
                output: output.into(),
            }
        };
        let expected = identity(type_name::<T>(), Some(TypeId::of::<T>()));
        self.verify_source(&source, expected)?;
        Ok(OutputRef {
            scope: self.module.scope,
            source,
            schema: type_name::<T>(),
            local_type: Some(TypeId::of::<T>()),
            _type: PhantomData,
        })
    }
    /// Portable/wire endpoints use the same symbolic lookup, but are checked
    /// against their declared wire schema rather than a local Rust TypeId.
    pub fn wire_input<T: ContractValue>(
        &self,
        node_or_instance: &str,
        port: &str,
        slot: &str,
    ) -> Result<Input<T>, String> {
        let source = if self
            .module
            .instances
            .iter()
            .any(|i| i.name == node_or_instance)
        {
            if !slot.is_empty() {
                return Err(
                    "a module public input has no nested slot; pass an empty slot name".into(),
                );
            }
            Target::Instance {
                name: node_or_instance.into(),
                port: port.into(),
            }
        } else {
            Target::Node {
                node: node_or_instance.into(),
                port: port.into(),
                slot: slot.into(),
            }
        };
        self.verify_target(&source, TypeIdentity::Wire(T::wire_schema()))?;
        Ok(Input {
            scope: self.module.scope,
            source,
            schema: T::wire_schema(),
            local_type: None,
            _type: PhantomData,
        })
    }
    pub fn wire_output<T: ContractValue>(
        &self,
        node_or_instance: &str,
        output: &str,
    ) -> Result<OutputRef<T>, String> {
        let source = if self
            .module
            .instances
            .iter()
            .any(|i| i.name == node_or_instance)
        {
            Source::Instance {
                name: node_or_instance.into(),
                port: output.into(),
            }
        } else {
            Source::Node {
                node: node_or_instance.into(),
                output: output.into(),
            }
        };
        self.verify_source(&source, TypeIdentity::Wire(T::wire_schema()))?;
        Ok(OutputRef {
            scope: self.module.scope,
            source,
            schema: T::wire_schema(),
            local_type: None,
            _type: PhantomData,
        })
    }

    /// Remove exactly one existing typed connection by symbolic endpoints.
    /// The builder keeps graph topology mutable until the final compile.
    fn disconnect_typed<T: Send + Sync + 'static>(
        &mut self,
        from: OutputRef<T>,
        to: Input<T>,
    ) -> Result<(), String> {
        if from.scope != self.module.scope || to.scope != self.module.scope {
            return Err("connection endpoints belong to another graph builder".into());
        }
        let expected = identity(from.schema, from.local_type);
        if expected != identity(to.schema, to.local_type) {
            return Err("connection type identity mismatch".into());
        }
        let source = self.verify_source(&from.source, expected)?;
        let target = self.verify_target(&to.source, expected)?;
        let before = self.module.edges.len();
        self.module
            .edges
            .retain(|edge| edge.from != source || edge.to != target);
        if before == self.module.edges.len() {
            return Err("connection not found".into());
        }
        Ok(())
    }

    /// Replace one named node's behavior/readiness without changing the
    /// existing external port/slot identities. Edges and public exports remain
    /// anchored by their symbolic names, not compiled physical node IDs.
    pub fn replace<R: Replacement>(&mut self, name: &str, replacement: R) -> Result<(), String> {
        replacement.apply(self, name)
    }

    fn replace_node_impl(
        &mut self,
        name: &str,
        mut replacement: NodeTemplate,
    ) -> Result<(), String> {
        let idx = self
            .module
            .nodes
            .iter()
            .position(|n| n.name.as_deref() == Some(name))
            .ok_or_else(|| format!("unknown node {name:?}"))?;
        // Observe staged configuration without committing or invalidating
        // handles. A failed replacement must leave the graph unchanged.
        let staged = self.pending.borrow().get(name).cloned();
        let old = staged.as_ref().unwrap_or(&self.module.nodes[idx]);
        let port_signature = |node: &NodeTemplate| -> Vec<_> {
            node.ports
                .iter()
                .map(|p| {
                    (
                        p.id,
                        p.name.clone(),
                        p.slots
                            .iter()
                            .map(|s| (s.id, s.name.clone(), s.identity()))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let output_signature = |node: &NodeTemplate| -> Vec<_> {
            node.outputs
                .iter()
                .map(|p| (p.id, p.name.clone(), p.identity()))
                .collect::<Vec<_>>()
        };
        if port_signature(old) != port_signature(&replacement)
            || output_signature(old) != output_signature(&replacement)
        {
            return Err(format!(
                "replacement {name:?} changes its public input/output interface"
            ));
        }
        replacement.id = old.id;
        replacement.declaring_scope = old.declaring_scope;
        replacement.name = old.name.clone();
        self.module.nodes[idx] = replacement;
        // Only a successful replacement revokes earlier layout handles.
        self.pending.borrow_mut().remove(name);
        Ok(())
    }

    /// Rebind an instance to a separately constructed module with precisely
    /// the same named public input/output identities. Replacing internals
    /// therefore cannot silently break the containing graph's connections.
    fn replace_instance_impl(&mut self, name: &str, module: &ModulePackage) -> Result<(), String> {
        let idx = self
            .module
            .instances
            .iter()
            .position(|i| i.name == name)
            .ok_or_else(|| format!("unknown module instance {name:?}"))?;
        let existing = self.dependency_for_instance(name)?;
        let compatible = |a: &ModuleEntry, b: &ModuleEntry| {
            let inputs = |entry: &ModuleEntry| {
                entry
                    .definition
                    .inputs
                    .iter()
                    .map(|p| (p.name.clone(), p.identity()))
                    .collect::<BTreeMap<_, _>>()
            };
            let outputs = |entry: &ModuleEntry| {
                entry
                    .definition
                    .outputs
                    .iter()
                    .map(|p| (p.name.clone(), p.identity()))
                    .collect::<BTreeMap<_, _>>()
            };
            inputs(a) == inputs(b) && outputs(a) == outputs(b)
        };
        if !compatible(existing, &module.root) {
            return Err(format!("replacement module {name:?} changes public ports"));
        }
        for (definition, identity) in &module.identities {
            if self
                .identities
                .get(definition)
                .is_some_and(|old| old != identity)
            {
                return Err(format!("conflicting module definition {definition:?}"));
            }
        }
        self.module.instances[idx].definition = module.name().to_owned();
        for entry in module
            .dependencies
            .iter()
            .chain(std::iter::once(&module.root))
        {
            self.dependencies
                .entry(entry.definition.id.clone())
                .or_insert_with(|| copy_entry(entry));
        }
        self.identities
            .extend(module.identities.iter().map(|(id, key)| (id.clone(), *key)));
        Ok(())
    }

    fn verify_source(
        &self,
        src: &Source,
        expected: TypeIdentity,
    ) -> Result<ModuleOutputSource, String> {
        match src {
            Source::Node { node, output } => {
                let selected = self.pending.borrow().get(node).cloned();
                let n = selected
                    .as_ref()
                    .or_else(|| {
                        self.module
                            .nodes
                            .iter()
                            .find(|n| n.name.as_deref() == Some(node))
                    })
                    .ok_or_else(|| format!("unknown source node {node:?}"))?;
                let addr = n.output(output)?;
                let actual = n
                    .outputs
                    .iter()
                    .find(|s| s.id == addr.slot)
                    .unwrap()
                    .identity();
                if actual != expected {
                    return Err(format!(
                        "source {node}.{output} type mismatch: {actual:?} != {expected:?}"
                    ));
                }
                Ok(ModuleOutputSource::NodeNamed {
                    node: node.clone(),
                    output: output.clone(),
                })
            }
            Source::Instance { name, port } => {
                let entry = self.dependency_for_instance(name)?;
                let actual = entry
                    .definition
                    .outputs
                    .iter()
                    .find(|p| &p.name == port)
                    .ok_or_else(|| format!("unknown exported output {name}.{port}"))?
                    .identity();
                if actual != expected {
                    return Err(format!("module source {name}.{port} type mismatch"));
                }
                Ok(ModuleOutputSource::InstanceNamed {
                    name: name.clone(),
                    port: port.clone(),
                })
            }
        }
    }
    fn dependency_for_instance(&self, name: &str) -> Result<&ModuleEntry, String> {
        let instance = self
            .module
            .instances
            .iter()
            .find(|instance| instance.name == name)
            .ok_or_else(|| format!("unknown module instance {name:?}"))?;
        self.dependencies
            .get(&instance.definition)
            .ok_or_else(|| format!("unregistered module definition {:?}", instance.definition))
    }
    fn verify_target(
        &self,
        target: &Target,
        expected: TypeIdentity,
    ) -> Result<ModuleInputTarget, String> {
        match target {
            Target::Node { node, port, slot } => {
                let selected = self.pending.borrow().get(node).cloned();
                let n = selected
                    .as_ref()
                    .or_else(|| {
                        self.module
                            .nodes
                            .iter()
                            .find(|n| n.name.as_deref() == Some(node))
                    })
                    .ok_or_else(|| format!("unknown destination node {node:?}"))?;
                let addr = n.input(port, slot)?;
                let actual = n
                    .ports
                    .iter()
                    .find(|p| p.id == addr.port)
                    .unwrap()
                    .slots
                    .iter()
                    .find(|s| s.id == addr.slot)
                    .unwrap()
                    .identity();
                if actual != expected {
                    return Err(format!(
                        "target {node}.{port}.{slot} type mismatch: {actual:?} != {expected:?}"
                    ));
                }
                Ok(ModuleInputTarget::NodeNamed {
                    node: node.clone(),
                    port: port.clone(),
                    slot: slot.clone(),
                })
            }
            Target::Instance { name, port } => {
                let entry = self.dependency_for_instance(name)?;
                let actual = entry
                    .definition
                    .inputs
                    .iter()
                    .find(|p| &p.name == port)
                    .ok_or_else(|| format!("unknown exported input {name}.{port}"))?
                    .identity();
                if actual != expected {
                    return Err(format!("module target {name}.{port} type mismatch"));
                }
                Ok(ModuleInputTarget::InstanceNamed {
                    name: name.clone(),
                    port: port.clone(),
                })
            }
        }
    }
    /// A mismatched Rust endpoint type is rejected before graph compilation:
    ///
    /// ```compile_fail
    /// use phenix_graph::GraphBuilder;
    /// let mut g = GraphBuilder::new("different-types");
    /// let a = g.node("a", |x: i64| x).unwrap();
    /// let b = g.node("b", |x: String| x).unwrap();
    /// g.connect(a.output(), b.inlet()).unwrap();
    /// ```
    fn connect_typed<T: Send + Sync + 'static>(
        &mut self,
        from: OutputRef<T>,
        to: Input<T>,
    ) -> Result<&mut Self, String> {
        if from.scope != self.module.scope || to.scope != self.module.scope {
            return Err("connection endpoints belong to another graph builder".into());
        }
        let from_id = identity(from.schema, from.local_type);
        let to_id = identity(to.schema, to.local_type);
        if from_id != to_id {
            return Err("connection type identity mismatch".into());
        }
        let src = self.verify_source(&from.source, from_id)?;
        let dst = self.verify_target(&to.source, to_id)?;
        // The compiled graph has a single logical edge for each endpoint pair.
        // Report duplicate wiring at the authoring site, not at compile().
        if self
            .module
            .edges
            .iter()
            .any(|edge| edge.from == src && edge.to == dst)
        {
            return Err("duplicate connection".into());
        }
        self.module.connect(src, dst);
        Ok(self)
    }
    fn expose_input_typed<T: Send + Sync + 'static>(
        &mut self,
        name: impl Into<String>,
        to: Input<T>,
    ) -> Result<PublicInput, String> {
        if to.scope != self.module.scope {
            return Err("input belongs to another graph builder".into());
        }
        let target = self.verify_target(&to.source, identity(to.schema, to.local_type))?;
        let name = name.into();
        validate_name_segment(&name).map_err(|error| error.to_string())?;
        if self.module.inputs.iter().any(|x| x.name == name) {
            return Err("duplicate public input".into());
        }
        let id =
            u16::try_from(self.module.inputs.len()).map_err(|_| "public input limit exceeded")?;
        self.module
            .input_with_identity(id, &name, to.schema, to.local_type, vec![target]);
        Ok(PublicInput {
            scope: self.module.scope,
            id,
            name,
            schema: to.schema,
            local_type: to.local_type,
        })
    }
    fn expose_output_typed<T: Send + Sync + 'static>(
        &mut self,
        name: impl Into<String>,
        from: OutputRef<T>,
    ) -> Result<(), String> {
        if from.scope != self.module.scope {
            return Err("output belongs to another graph builder".into());
        }
        let source = self.verify_source(&from.source, identity(from.schema, from.local_type))?;
        let name = name.into();
        validate_name_segment(&name).map_err(|error| error.to_string())?;
        if self.module.outputs.iter().any(|x| x.name == name) {
            return Err("duplicate public output".into());
        }
        let id =
            u16::try_from(self.module.outputs.len()).map_err(|_| "public output limit exceeded")?;
        self.module
            .output_with_identity(id, name, from.schema, from.local_type, source);
        Ok(())
    }
    pub fn trigger<I, T>(&mut self, name: impl Into<String>, inputs: I) -> Result<TriggerId, String>
    where
        I: IntoIterator<Item = T>,
        T: TriggerEndpoint,
    {
        let name = name.into();
        validate_name_segment(&name).map_err(|error| error.to_string())?;
        if self.trigger_names.contains_key(&name) {
            return Err("duplicate trigger name".into());
        }
        let id =
            u16::try_from(self.trigger_names.len() + 1).map_err(|_| "trigger limit exceeded")?;
        let input_ports: Vec<PublicInput> = inputs
            .into_iter()
            .map(|input| input.resolve(self))
            .collect::<Result<_, _>>()?;
        let targets: Vec<_> = input_ports
            .into_iter()
            .map(|input| {
                if input.scope == self.module.scope
                    && self.module.inputs.iter().any(|port| {
                        port.id == input.id
                            && port.name == input.name
                            && port.identity() == identity(input.schema, input.local_type)
                    })
                {
                    Ok(ModuleTriggerTarget::PublicInput(input.id))
                } else {
                    Err("trigger input is not exported by this builder".to_string())
                }
            })
            .collect::<Result<_, _>>()?;
        if targets.is_empty() {
            return Err("trigger must have at least one input".into());
        }
        let unique: BTreeSet<_> = targets
            .iter()
            .map(|target| match target {
                ModuleTriggerTarget::PublicInput(id) => *id,
                _ => unreachable!("authoring triggers only contain exported inputs"),
            })
            .collect();
        if unique.len() != targets.len() {
            return Err("trigger repeats a public input".into());
        }
        self.module.trigger_named(id, name.clone(), targets);
        self.trigger_names.insert(name, id);
        Ok(TriggerId(id))
    }
    pub fn instance(
        &mut self,
        name: impl Into<String>,
        module: &ModulePackage,
    ) -> Result<ModuleHandle, String> {
        let name = name.into();
        if self.module.id == module.name() {
            return Err("recursive module definition".into());
        }
        // A name is not a definition identity. Independently built modules with
        // the same name are rejected even when their interface and node counts
        // happen to match. Reusing the *same* package is supported.
        for (definition, identity) in &module.identities {
            if let Some(existing) = self.identities.get(definition)
                && existing != identity
            {
                return Err(format!("conflicting module definition {definition:?}"));
            }
        }
        self.module
            .try_instance(&name, module.name())
            .map_err(|error| error.to_string())?;
        for entry in module
            .dependencies
            .iter()
            .chain(std::iter::once(&module.root))
        {
            self.dependencies
                .entry(entry.definition.id.clone())
                .or_insert_with(|| copy_entry(entry));
        }
        for (definition, identity) in &module.identities {
            self.identities.insert(definition.clone(), *identity);
        }
        Ok(ModuleHandle {
            scope: self.module.scope,
            name,
            inputs: module
                .inputs()
                .iter()
                .map(|p| (p.name.clone(), (p.schema, p.local_type)))
                .collect(),
            outputs: module
                .outputs()
                .iter()
                .map(|p| (p.name.clone(), (p.schema, p.local_type)))
                .collect(),
        })
    }
    fn apply_pending(&mut self) {
        let mut pending = self.pending.borrow_mut();
        for (name, template) in pending.iter() {
            if let Some(node) = self
                .module
                .nodes
                .iter_mut()
                .find(|n| n.name.as_deref() == Some(name))
            {
                *node = template.clone();
            }
        }
        pending.clear();
    }
    pub fn module(mut self) -> ModulePackage {
        self.apply_pending();
        let mut identities = self.identities;
        identities.insert(self.module.id.clone(), self.definition_id);
        ModulePackage {
            root: self.module.build(),
            dependencies: self.dependencies.into_values().collect(),
            identities,
            compile_limits: self.compile_limits,
        }
    }
    pub fn compile(self) -> Result<Arc<CompiledGraph>, CompileDiagnostic> {
        self.module().compile()
    }
}

/// A source-scoped edit transaction. This is an assembly/build concern, not
/// something the executor interprets: the compiled graph remains immutable.
/// The caller determines contribution order explicitly; plugin discovery,
/// priority and authority belong to higher layers.
///
/// Future patch engines can lower their resolved edits into `contribute`;
/// matching targets are stable node/instance names and typed public ports.
#[derive(Clone)]
pub struct GraphAssembly {
    builder: Option<GraphBuilder>,
    revisions: Vec<AssemblyRevision>,
    contributors: BTreeSet<String>,
}

/// Provenance for a successful pre-compilation contribution. Records are
/// assembly metadata, never execution or authorization grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssemblyRevision {
    pub contributor: String,
    /// Symbolic edit log, independent of flattened NodeId/EdgeId allocation.
    pub changes: Vec<AssemblyChange>,
    pub nodes_after: Vec<String>,
    pub instances_after: Vec<String>,
    pub connections_after: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssemblyChange {
    NodeAdded(String),
    NodeReplaced(String),
    InstanceAdded(String),
    InstanceRebound(String),
    ConnectionAdded(String),
    ConnectionRemoved(String),
    InputExposed(String),
    OutputExposed(String),
    TriggerAdded(String),
}
/// Stable symbolic edge key. Names use byte-length prefixes, so names
/// containing dots or colons cannot collide. Only the fallback for manually
/// authored non-symbolic low-level endpoints uses Debug text.
fn patch_connection_key(from: &ModuleOutputSource, to: &ModuleInputTarget) -> String {
    let component = |name: &str| format!("{}:{name}", name.len());
    let src = match from {
        ModuleOutputSource::NodeNamed { node, output } => {
            format!("node/{}/{}", component(node), component(output))
        }
        ModuleOutputSource::InstanceNamed { name, port } => {
            format!("instance/{}/{}", component(name), component(port))
        }
        other => format!("non_symbolic/{other:?}"),
    };
    let dst = match to {
        ModuleInputTarget::NodeNamed { node, port, slot } => format!(
            "node/{}/{}/{}",
            component(node),
            component(port),
            component(slot)
        ),
        ModuleInputTarget::InstanceNamed { name, port } => {
            format!("instance/{}/{}", component(name), component(port))
        }
        other => format!("non_symbolic/{other:?}"),
    };
    format!("{src}->{dst}")
}

fn assembly_changes(before: &GraphBuilder, after: &GraphBuilder) -> Vec<AssemblyChange> {
    let mut changes = Vec::new();
    for n in &after.module.nodes {
        let name = n.name.as_deref().unwrap_or("<unnamed>");
        match before.module.nodes.iter().find(|old| old.name == n.name) {
            None => changes.push(AssemblyChange::NodeAdded(name.into())),
            Some(old) if !Arc::ptr_eq(&old.logic_factory, &n.logic_factory) => {
                changes.push(AssemblyChange::NodeReplaced(name.into()))
            }
            _ => {}
        }
    }
    for n in &after.module.instances {
        match before
            .module
            .instances
            .iter()
            .find(|old| old.name == n.name)
        {
            None => changes.push(AssemblyChange::InstanceAdded(n.name.clone())),
            Some(old) if old.definition != n.definition => {
                changes.push(AssemblyChange::InstanceRebound(n.name.clone()))
            }
            _ => {}
        }
    }
    for edge in &after.module.edges {
        if !before
            .module
            .edges
            .iter()
            .any(|old| old.from == edge.from && old.to == edge.to)
        {
            changes.push(AssemblyChange::ConnectionAdded(patch_connection_key(
                &edge.from, &edge.to,
            )));
        }
    }
    for edge in &before.module.edges {
        if !after
            .module
            .edges
            .iter()
            .any(|new| new.from == edge.from && new.to == edge.to)
        {
            changes.push(AssemblyChange::ConnectionRemoved(patch_connection_key(
                &edge.from, &edge.to,
            )));
        }
    }
    for input in &after.module.inputs {
        if !before
            .module
            .inputs
            .iter()
            .any(|old| old.name == input.name)
        {
            changes.push(AssemblyChange::InputExposed(input.name.clone()));
        }
    }
    for output in &after.module.outputs {
        if !before
            .module
            .outputs
            .iter()
            .any(|old| old.name == output.name)
        {
            changes.push(AssemblyChange::OutputExposed(output.name.clone()));
        }
    }
    for name in after.trigger_names.keys() {
        if !before.trigger_names.contains_key(name) {
            changes.push(AssemblyChange::TriggerAdded(name.clone()));
        }
    }
    changes
}

pub struct AssembledGraph {
    pub graph: Arc<CompiledGraph>,
    pub revisions: Vec<AssemblyRevision>,
}

impl GraphAssembly {
    pub fn new(name: impl Into<String>) -> Self {
        Self::from_builder(GraphBuilder::new(name))
    }
    fn from_builder(builder: GraphBuilder) -> Self {
        Self {
            builder: Some(builder),
            revisions: Vec::new(),
            contributors: BTreeSet::new(),
        }
    }
    /// Apply one whole contribution or patch atomically. Failures roll back all
    /// of its node/edge/module edits and do not reserve the contributor name.
    /// Only successful contributions become part of the eventual compilation.
    pub(crate) fn contribute<F>(
        &mut self,
        contributor: impl Into<String>,
        edit: F,
    ) -> Result<(), String>
    where
        F: FnOnce(&mut GraphBuilder) -> Result<(), String>,
    {
        let contributor = contributor.into();
        validate_name_segment(&contributor).map_err(|e| e.to_string())?;
        if self.contributors.contains(&contributor) {
            return Err(format!("duplicate graph contributor {contributor:?}"));
        }
        let mut next = self
            .builder
            .as_ref()
            .expect("assembly not yet finished")
            .clone();
        edit(&mut next).map_err(|e| format!("contribution {contributor:?}: {e}"))?;
        let revision = AssemblyRevision {
            contributor: contributor.clone(),
            changes: assembly_changes(self.builder.as_ref().expect("valid assembly"), &next),
            nodes_after: next
                .module
                .nodes
                .iter()
                .filter_map(|n| n.name.clone())
                .collect(),
            instances_after: next
                .module
                .instances
                .iter()
                .map(|n| n.name.clone())
                .collect(),
            connections_after: next.module.edges.len(),
        };
        self.builder = Some(next);
        self.revisions.push(revision);
        self.contributors.insert(contributor);
        Ok(())
    }
    pub fn revisions(&self) -> &[AssemblyRevision] {
        &self.revisions
    }
    /// Compile exactly once after every contribution has been applied.
    /// No patch or contribution callback survives into graph execution.
    pub fn finish(self) -> Result<AssembledGraph, CompileDiagnostic> {
        let graph = self.builder.expect("assembly not yet finished").compile()?;
        Ok(AssembledGraph {
            graph,
            revisions: self.revisions,
        })
    }
}

mod addresses;
mod patching;
pub use patching::{
    ConnectionRoute, PatchAnchor, PatchBuilder, PatchDestination, PatchError, PatchPlanner,
    PatchResolution, PatchSource, PatchSpec, PatchTarget, PlannedSplice, PortSelector,
    SplicePosition, SupersededPatch,
};
