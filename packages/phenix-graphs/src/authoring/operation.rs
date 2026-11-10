//! Typed addressing only. Layouts supply the frontier; the existing graph
//! verifier and module compiler remain the authority for every projection.
use super::*;
use std::borrow::Cow;

/// Representation metadata for a typed selection, not a runtime conversion.
pub trait ValueMode<T: Send + Sync + 'static> {
    fn declared() -> DeclaredValue;
}
impl<T: Send + Sync + 'static> ValueMode<T> for LocalValue {
    fn declared() -> DeclaredValue {
        DeclaredValue::local::<T>()
    }
}
impl<T: ContractValue> ValueMode<T> for EncodedValue {
    fn declared() -> DeclaredValue {
        DeclaredValue::wire::<T>()
    }
}

/// A port belongs to one interface, and selects its domain value and layout.
pub struct InletPort<I, T, L = LocalAtomic<T>> {
    name: Cow<'static, str>,
    marker: PhantomData<fn(I) -> (T, L)>,
}
impl<I, T, L> Clone for InletPort<I, T, L> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            marker: PhantomData,
        }
    }
}
impl<I, T, L> InletPort<I, T, L> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            marker: PhantomData,
        }
    }
    pub fn named(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        validate_name_segment(&name).map_err(|error| error.to_string())?;
        Ok(Self {
            name: Cow::Owned(name),
            marker: PhantomData,
        })
    }
}

/// Statically one port. Only this interface exposes the `inlet()` shorthand.
pub struct SinglePort<T, L = LocalAtomic<T>>(PhantomData<fn() -> (T, L)>);
/// Zero-port operations intentionally have no `inlet()` shorthand.
///
/// ```compile_fail
/// use phenix_graph::{NoPorts, Operation};
/// fn wrong(operation: Operation<NoPorts, i64>) { operation.inlet(); }
/// ```
pub struct NoPorts;

/// Custom NodeDefinitions use `Ports<I>` for zero or multiple physical ports.
/// Their typed descriptors belong to I, not to an unrelated domain input type.
///
/// ```compile_fail
/// use phenix_graph::Operation;
/// struct Several;
/// fn wrong(operation: Operation<Several, i64>) { operation.inlet(); }
/// ```
pub struct Ports<I>(PhantomData<fn() -> I>);
pub trait NodeInputs<A> {
    type Interface;
    fn validate(node: &NodeTemplate, signature: &NodeSignature) -> Result<(), String>;
}
impl<A: 'static, L: InputLayout<Value = A>> NodeInputs<A> for L {
    type Interface = SinglePort<A, L>;
    fn validate(node: &NodeTemplate, signature: &NodeSignature) -> Result<(), String> {
        if signature
            .input
            .local_type
            .is_some_and(|ty| ty != TypeId::of::<A>())
        {
            return Err("node input signature does not match its domain type".into());
        }
        if node.ports.len() != 1 {
            return Err("input layout requires exactly one physical port".into());
        }
        validate_slots(&node.ports[0].slots)?;
        if let Some(expected) = L::atomic_identity() {
            if identity(&signature.input.schema, signature.input.local_type) != expected {
                return Err(
                    "atomic signature does not match its domain type or representation".into(),
                );
            }
            validate_atomic(&node.ports[0].slots, expected)?;
        }
        Ok(())
    }
}
impl<A, I: PortInterface> NodeInputs<A> for Ports<I> {
    type Interface = I;
    fn validate(node: &NodeTemplate, signature: &NodeSignature) -> Result<(), String> {
        I::validate_ports(node, signature)
    }
}

fn validate_slots(slots: &[InputSlot]) -> Result<(), String> {
    if slots.is_empty() {
        return Err("ports must contain at least one slot".into());
    }
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for slot in slots {
        let name = slot
            .name
            .as_deref()
            .ok_or("typed input slots must be named")?;
        validate_name_segment(name).map_err(|e| e.to_string())?;
        if !names.insert(name) || !ids.insert(slot.id) {
            return Err("duplicate input slot name or id".into());
        }
    }
    Ok(())
}
fn validate_atomic(slots: &[InputSlot], expected: TypeIdentity) -> Result<(), String> {
    if slots.len() != 1
        || slots[0].name.as_deref() != Some("value")
        || slots[0].identity() != expected
    {
        return Err(
            "atomic input requires exactly one value slot of its domain type and representation"
                .into(),
        );
    }
    Ok(())
}

pub(super) fn validate_endpoints(node: &NodeTemplate) -> Result<(), String> {
    let output = node
        .output
        .name
        .as_deref()
        .ok_or("typed output must be named")?;
    validate_name_segment(output).map_err(|e| e.to_string())?;
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for port in &node.ports {
        let name = port
            .name
            .as_deref()
            .ok_or("typed input ports must be named")?;
        validate_name_segment(name).map_err(|e| e.to_string())?;
        if !names.insert(name) || !ids.insert(port.id) {
            return Err("duplicate input port name or id".into());
        }
        validate_slots(&port.slots)?;
    }
    Ok(())
}

pub(super) fn validate_node<A, O, D, R, L>(
    node: &NodeTemplate,
    signature: &NodeSignature,
) -> Result<(), String>
where
    A: Send + Sync + 'static,
    O: Send + Sync + 'static,
    D: NodeDefinition<A, O, R>,
    L: NodeInputs<A>,
{
    validate_endpoints(node)?;
    L::validate(node, signature)?;
    let local = TypeId::of::<D::InputMode>() == TypeId::of::<LocalValue>();
    let encoded = TypeId::of::<D::InputMode>() == TypeId::of::<EncodedValue>();
    if (!local && !encoded)
        || signature.input.local_type.is_some() != local
        || node
            .ports
            .iter()
            .flat_map(|p| &p.slots)
            .any(|s| s.local_type.is_some() != local)
    {
        return Err("node input representation does not match its definition mode".into());
    }
    let output = D::OutputMode::declared();
    if identity(&output.schema, output.local_type)
        != identity(&signature.output.schema, signature.output.local_type)
        || node.output.identity() != identity(&output.schema, output.local_type)
    {
        return Err("node output does not match its domain type or representation".into());
    }
    Ok(())
}

/// Implement on a custom multi-port owner. This describes cardinality only;
/// physical Ports continue to own readiness and input preparation.
pub trait PortInterface {
    const PORT_COUNT: Option<usize> = None;
    fn validate_count(count: usize) -> Result<(), String> {
        if Self::PORT_COUNT.is_some_and(|expected| expected != count) {
            return Err("physical port count does not match the typed interface".into());
        }
        Ok(())
    }
    fn validate_layout<T: 'static, L: InputLayout<Value = T>>() -> Result<(), String> {
        Ok(())
    }
    fn validate_ports(node: &NodeTemplate, _: &NodeSignature) -> Result<(), String> {
        Self::validate_count(node.ports.len())
    }
}
impl PortInterface for NoPorts {
    const PORT_COUNT: Option<usize> = Some(0);
}
impl<T: 'static, L: InputLayout<Value = T>> PortInterface for SinglePort<T, L> {
    const PORT_COUNT: Option<usize> = Some(1);
    fn validate_layout<U: 'static, K: InputLayout<Value = U>>() -> Result<(), String> {
        if TypeId::of::<T>() != TypeId::of::<U>() || TypeId::of::<L>() != TypeId::of::<K>() {
            return Err(
                "public port domain type or layout does not match its single-port interface".into(),
            );
        }
        Ok(())
    }
    fn validate_ports(node: &NodeTemplate, signature: &NodeSignature) -> Result<(), String> {
        L::validate(node, signature)
    }
}

#[derive(Clone)]
struct Address {
    port: String,
    slot: String,
    value: DeclaredValue,
    target: Target,
}
#[derive(Clone)]
pub(super) struct AddressBook {
    scope: u64,
    declaration_id: u64,
    ports: Arc<Vec<String>>,
    inputs: Arc<Vec<Address>>,
    output: Option<(Source, DeclaredValue)>,
}
impl AddressBook {
    pub(super) fn output<T: Send + Sync + 'static, M>(&self) -> OutputRef<T, M> {
        let (source, value) = self
            .output
            .as_ref()
            .expect("typed operation has one output");
        OutputRef {
            scope: self.scope,
            declaration_id: self.declaration_id,
            source: source.clone(),
            schema: value.schema.clone(),
            local_type: value.local_type,
            _type: PhantomData,
        }
    }
    pub(super) fn node(scope: u64, node: &NodeTemplate) -> Self {
        let name = node.name.clone().expect("registered node has a name");
        Self {
            scope,
            declaration_id: node.declaration_id,
            ports: Arc::new(
                node.ports
                    .iter()
                    .map(|port| port.name.clone().expect("validated named port"))
                    .collect(),
            ),
            inputs: Arc::new(
                node.ports
                    .iter()
                    .flat_map(|port| {
                        let name = &name;
                        port.slots.iter().map(move |slot| Address {
                            port: port.name.clone().expect("validated named port"),
                            slot: slot.name.clone().expect("validated named slot"),
                            value: DeclaredValue {
                                schema: slot.schema.clone(),
                                local_type: slot.local_type,
                            },
                            target: Target::Node {
                                node: name.clone(),
                                port: port.name.clone().expect("validated named port"),
                                slot: slot.name.clone().expect("validated named slot"),
                            },
                        })
                    })
                    .collect(),
            ),
            output: Some((
                Source::Node {
                    node: name,
                    output: node.output.name.clone().expect("validated named output"),
                },
                DeclaredValue {
                    schema: node.output.schema.clone(),
                    local_type: node.output.local_type,
                },
            )),
        }
    }
    pub(super) fn instance(
        scope: u64,
        declaration_id: u64,
        name: &str,
        definition: &ModuleDef,
    ) -> Self {
        Self {
            scope,
            declaration_id,
            ports: Arc::new(
                definition
                    .inputs
                    .iter()
                    .map(|input| input.port.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            ),
            inputs: Arc::new(
                definition
                    .inputs
                    .iter()
                    .map(|input| Address {
                        port: input.port.clone(),
                        slot: input.slot.clone(),
                        value: DeclaredValue {
                            schema: input.schema.clone(),
                            local_type: input.local_type,
                        },
                        target: Target::Instance {
                            name: name.into(),
                            port: input.name.clone(),
                        },
                    })
                    .collect(),
            ),
            output: definition.output.as_ref().map(|output| {
                (
                    Source::Instance {
                        name: name.into(),
                        port: output.name.clone(),
                    },
                    DeclaredValue {
                        schema: output.schema.clone(),
                        local_type: output.local_type,
                    },
                )
            }),
        }
    }
    fn view<T, M>(&self, port: &str) -> InletView<T, M> {
        InletView {
            book: self.clone(),
            port: port.into(),
            path: String::new(),
            marker: PhantomData,
        }
    }
    fn single_port(&self) -> &str {
        let [port] = self.ports.as_slice() else {
            unreachable!("single-port declarations are validated at admission")
        };
        port
    }
}

/// Lightweight shared Node/Module operation handle. No execution state lives here.
pub struct Operation<I, O: Send + Sync + 'static, IM = LocalValue, OM = IM> {
    book: AddressBook,
    marker: PhantomData<fn(I) -> O>,
    modes: PhantomData<fn(IM) -> OM>,
}
impl<I, O: Send + Sync + 'static, IM, OM> Clone for Operation<I, O, IM, OM> {
    fn clone(&self) -> Self {
        Self {
            book: self.book.clone(),
            marker: PhantomData,
            modes: PhantomData,
        }
    }
}
impl<I, O: Send + Sync + 'static, IM, OM> Operation<I, O, IM, OM> {
    pub fn inlets(&self) -> Inlets<I, IM> {
        Inlets {
            book: self.book.clone(),
            marker: PhantomData,
        }
    }
    pub fn output(&self) -> OutputRef<O, OM> {
        self.book.output()
    }
}
/// Explicit activation of a portless operation, not an input endpoint or payload.
pub struct SourceStart {
    book: AddressBook,
}
impl<O: Send + Sync + 'static, IM, OM> Operation<NoPorts, O, IM, OM> {
    pub fn start(&self) -> SourceStart {
        SourceStart {
            book: self.book.clone(),
        }
    }
}
impl private_endpoints::Sealed for SourceStart {}
impl TriggerEndpoint for SourceStart {
    fn resolve(self, graph: &GraphBuilder) -> Result<ModuleTriggerTarget, String> {
        let (source, _) = self.book.output.as_ref().ok_or("source has no output")?;
        if self.book.scope != graph.module.scope
            || graph.source_declaration_id(source)? != self.book.declaration_id
        {
            return Err("source start belongs to another or stale declaration".into());
        }
        let name = match source {
            Source::Node { node, .. } => node,
            Source::Instance { name, .. } => name,
        };
        Ok(ModuleTriggerTarget::Source(name.clone()))
    }
}
impl<T, L, O, IM, OM> Operation<SinglePort<T, L>, O, IM, OM>
where
    T: Send + Sync + 'static,
    O: Send + Sync + 'static,
    L: InletLayout<T, IM>,
{
    pub fn inlet(&self) -> L::View {
        L::select(self.book.view(self.book.single_port()))
    }
}

pub struct Inlets<I, M> {
    book: AddressBook,
    marker: PhantomData<fn(I, M)>,
}
impl<I, M> Inlets<I, M> {
    /// Descriptors from another interface cannot be used here.
    ///
    /// ```compile_fail
    /// use phenix_graph::{Inlets, InletPort, LocalValue};
    /// struct This; struct Other;
    /// fn wrong(inlets: Inlets<This, LocalValue>) {
    ///     inlets.port(InletPort::<Other, i64>::new("run"));
    /// }
    /// ```
    pub fn port<T, L>(&self, port: InletPort<I, T, L>) -> InletView<T, M> {
        self.book.view(&port.name)
    }
}

/// Checked discovery keeps the operation boundary, rather than accepting a raw
/// node/port/slot triple for every endpoint. Type selection is validated at leaves.
pub struct DiscoveredOperation {
    book: AddressBook,
}

/// Retains the borrowed contract authority, not a validation snapshot. Leaves
/// resolve afresh after checking the complete effective interface on every access.
pub struct BoundOperation<'g> {
    bound: BoundAuthoringUnit<'g>,
}
impl<'g> BoundOperation<'g> {
    pub(super) fn new(bound: BoundAuthoringUnit<'g>) -> Self {
        Self { bound }
    }
    pub fn inlets<I, M>(&self) -> BoundInlets<'g, I, M> {
        BoundInlets {
            bound: self.bound.clone(),
            marker: PhantomData,
        }
    }
    pub fn output<T: Send + Sync + 'static, M: ValueMode<T>>(
        &self,
    ) -> Result<OutputRef<T, M>, InterfaceError> {
        let operation = self.bound.operation_snapshot()?;
        let name = operation
            .book
            .output
            .as_ref()
            .map(|(source, _)| match source {
                Source::Node { output, .. } => output.as_str(),
                Source::Instance { port, .. } => port.as_str(),
            });
        operation.output().map_err(|message| {
            let mut error = self.bound.lookup_error(message);
            error.endpoint = name.map(|name| format!("output {name}").into());
            error
        })
    }
}
pub struct BoundInlets<'g, I, M> {
    bound: BoundAuthoringUnit<'g>,
    marker: PhantomData<fn(I, M)>,
}
impl<'g, I, M> BoundInlets<'g, I, M> {
    pub fn port<T, L>(&self, port: InletPort<I, T, L>) -> BoundInletView<'g, T, M> {
        BoundInletView {
            bound: self.bound.clone(),
            port: port.name.into_owned(),
            path: String::new(),
            marker: PhantomData,
        }
    }
}
pub struct BoundInletView<'g, T, M> {
    bound: BoundAuthoringUnit<'g>,
    port: String,
    path: String,
    marker: PhantomData<fn(T, M)>,
}
impl<'g, T, M> BoundInletView<'g, T, M> {
    pub fn field<U>(&self, key: impl FieldSelector<T, U>) -> BoundInletView<'g, U, M> {
        BoundInletView {
            bound: self.bound.clone(),
            port: self.port.clone(),
            path: if self.path.is_empty() {
                key.path().into()
            } else {
                format!("{}.{}", self.path, key.path())
            },
            marker: PhantomData,
        }
    }
}
impl<T: Send + Sync + 'static, M: ValueMode<T>> BoundInletView<'_, T, M> {
    pub fn target(&self) -> Result<Input<T, M>, InterfaceError> {
        let mut view = self
            .bound
            .operation_snapshot()?
            .book
            .view::<T, M>(&self.port);
        view.path = self.path.clone();
        view.target()
            .map_err(|error| self.bound.lookup_error(error))
    }
}
impl DiscoveredOperation {
    pub(super) fn from_book(book: AddressBook) -> Result<Self, String> {
        if book.output.is_none() {
            return Err("operation has no exposed output".into());
        }
        Ok(Self { book })
    }

    pub fn inlets<I, M>(&self) -> Inlets<I, M> {
        Inlets {
            book: self.book.clone(),
            marker: PhantomData,
        }
    }
    pub fn output<T: Send + Sync + 'static, M: ValueMode<T>>(
        &self,
    ) -> Result<OutputRef<T, M>, String> {
        let (_, actual) = self
            .book
            .output
            .as_ref()
            .ok_or("operation has no exposed output")?;
        let expected = M::declared();
        if identity(&expected.schema, expected.local_type)
            != identity(&actual.schema, actual.local_type)
        {
            return Err("output type or representation mismatch".into());
        }
        Ok(Operation::<NoPorts, T, M> {
            book: self.book.clone(),
            marker: PhantomData,
            modes: PhantomData,
        }
        .output())
    }
    pub(super) fn restrict(mut self, shape: &InterfaceShape) -> Self {
        self.book.inputs = Arc::new(
            self.book
                .inputs
                .iter()
                .filter(|address| {
                    shape.inputs().any(|(port, slot, value)| {
                        port == address.port
                            && slot == address.slot
                            && identity(&value.schema, value.local_type)
                                == identity(&address.value.schema, address.value.local_type)
                    })
                })
                .cloned()
                .collect(),
        );
        if !shape.output_value().is_some_and(|(name, value)| {
            self.book.output.as_ref().is_some_and(|(source, actual)| {
                let actual_name = match source {
                    Source::Node { output, .. } => output,
                    Source::Instance { port, .. } => port,
                };
                name == Some(actual_name.as_str())
                    && identity(&value.schema, value.local_type)
                        == identity(&actual.schema, actual.local_type)
            })
        }) {
            self.book.output = None;
        }
        self
    }
}

/// A structural view is deliberately not a connection endpoint. The selected
/// layout may stop above or below this value. `target()` checks the actual frontier.
///
/// ```compile_fail
/// use phenix_graph::{GraphBuilder, Layout, LocalPortLayout};
/// #[derive(Clone, LocalPortLayout, phenix_graph::InputFields)] struct Pair { left: i64, right: i64 }
/// let mut g = GraphBuilder::new("nonleaf");
/// let source = g.node("source", |x: Pair| x).unwrap();
/// let sink = g.node("sink", |x: Pair| x.left + x.right).unwrap()
///     .input(Layout::fields()).unwrap();
/// g.connect(source.output(), sink.inlet()).unwrap();
/// ```
pub struct InletView<T, M = LocalValue> {
    book: AddressBook,
    port: String,
    path: String,
    marker: PhantomData<fn(T, M)>,
}
impl<T, M> Clone for InletView<T, M> {
    fn clone(&self) -> Self {
        Self {
            book: self.book.clone(),
            port: self.port.clone(),
            path: self.path.clone(),
            marker: PhantomData,
        }
    }
}
impl<T, M> InletView<T, M> {
    /// ```compile_fail
    /// use phenix_graph::{InletView, SlotField};
    /// struct This; struct Other;
    /// fn wrong(view: InletView<This>) {
    ///     view.field(SlotField::<Other, i64>::new("value"));
    /// }
    /// ```
    pub fn field<U>(&self, key: impl FieldSelector<T, U>) -> InletView<U, M> {
        InletView {
            book: self.book.clone(),
            port: self.port.clone(),
            path: if self.path.is_empty() {
                key.path().into()
            } else {
                format!("{}.{}", self.path, key.path())
            },
            marker: PhantomData,
        }
    }
}
impl<T: Send + Sync + 'static, M: ValueMode<T>> InletView<T, M> {
    pub fn target(&self) -> Result<Input<T, M>, String> {
        let path = if self.path.is_empty() {
            "value"
        } else {
            &self.path
        };
        let address = self
            .book
            .inputs
            .iter()
            .find(|a| a.port == self.port && a.slot == path)
            .ok_or_else(|| {
                format!(
                    "{}.{} is not a leaf of the declared input layout",
                    self.port, path
                )
            })?;
        let expected = M::declared();
        let actual = &address.value;
        if identity(&expected.schema, expected.local_type)
            != identity(&actual.schema, actual.local_type)
        {
            return Err(match (expected.local_type, actual.local_type) {
                (Some(_), Some(_)) => "input Rust TypeId mismatch".into(),
                (None, None) => "input canonical schema mismatch".into(),
                _ => "input local/encoded representation mismatch".into(),
            });
        }
        Ok(Input {
            scope: self.book.scope,
            declaration_id: self.book.declaration_id,
            source: address.target.clone(),
            schema: address.value.schema.clone(),
            local_type: address.value.local_type,
            _type: PhantomData,
        })
    }
}

/// Static atomic proof for built-in atomic layouts; custom layouts default to a
/// checked structural view by implementing this with `View = InletView<T, M>`.
pub trait InletLayout<T: Send + Sync + 'static, M> {
    type View;
    fn select(view: InletView<T, M>) -> Self::View;
}
impl<T: Send + Sync + 'static> InletLayout<T, LocalValue> for LocalAtomic<T> {
    type View = Input<T, LocalValue>;
    fn select(view: InletView<T, LocalValue>) -> Self::View {
        view.target()
            .expect("atomic declaration has one value slot")
    }
}
impl<T: ContractValue> InletLayout<T, EncodedValue> for Atomic<T> {
    type View = Input<T, EncodedValue>;
    fn select(view: InletView<T, EncodedValue>) -> Self::View {
        view.target()
            .expect("atomic declaration has one value slot")
    }
}
macro_rules! structured {
    ($($layout:ident),*) => { $(
        impl<T: Send + Sync + 'static, M> InletLayout<T, M> for $layout<T> {
            type View = InletView<T, M>;
            fn select(view: InletView<T, M>) -> Self::View { view }
        }
    )* };
}
structured!(LocalFlatten, LocalDeepFlatten, Flatten, DeepFlatten);
impl<T: Send + Sync + 'static, M, const N: usize> InletLayout<T, M> for LocalFlattenTo<T, N> {
    type View = InletView<T, M>;
    fn select(view: InletView<T, M>) -> Self::View {
        view
    }
}
impl<T: Send + Sync + 'static, M, const N: usize> InletLayout<T, M> for FlattenTo<T, N> {
    type View = InletView<T, M>;
    fn select(view: InletView<T, M>) -> Self::View {
        view
    }
}

impl<A, O, L, D, R> NodeHandle<A, O, L, D, R>
where
    A: Send + Sync + 'static,
    O: Send + Sync + 'static,
    D: NodeDefinition<A, O, R>,
    L: NodeInputs<A>,
{
    pub fn operation(&self) -> Operation<L::Interface, O, D::InputMode, D::OutputMode> {
        Operation {
            book: self.addressing.clone(),
            marker: PhantomData,
            modes: PhantomData,
        }
    }
    pub fn inlets(&self) -> Inlets<L::Interface, D::InputMode> {
        self.operation().inlets()
    }
}

/// Public input layout, declared independently of implementation children.
pub struct InputInterface<I, M = LocalValue> {
    ports: BTreeSet<String>,
    slots: Vec<(String, String, DeclaredValue)>,
    marker: PhantomData<fn(I, M)>,
}
impl<I: PortInterface, M> Default for InputInterface<I, M> {
    fn default() -> Self {
        Self::new()
    }
}
impl<I: PortInterface, M> InputInterface<I, M> {
    pub fn new() -> Self {
        Self {
            ports: BTreeSet::new(),
            slots: Vec::new(),
            marker: PhantomData,
        }
    }
    pub fn port<T: 'static, L: InputLayout<Value = T>>(
        mut self,
        port: InletPort<I, T, L>,
        _layout: LayoutMode<T, L>,
    ) -> Result<Self, String> {
        validate_name_segment(&port.name).map_err(|e| e.to_string())?;
        if I::PORT_COUNT.is_some_and(|count| self.ports.len() >= count) {
            return Err("interface cannot admit another public port".into());
        }
        if !self.ports.insert(port.name.to_string()) {
            return Err("duplicate interface port".into());
        }
        let slots = L::slots().map_err(|e| e.to_string())?;
        I::validate_layout::<T, L>()?;
        validate_slots(&slots)?;
        if let Some(expected) = L::atomic_identity() {
            validate_atomic(&slots, expected)?;
        }
        let mut names = BTreeSet::new();
        for slot in slots {
            let name = slot.name.ok_or("public layout slots must be named")?;
            validate_name_segment(&name).map_err(|e| e.to_string())?;
            if !names.insert(name.clone()) {
                return Err("duplicate public layout slot".into());
            }
            self.slots.push((
                port.name.to_string(),
                name,
                DeclaredValue {
                    schema: slot.schema,
                    local_type: slot.local_type,
                },
            ));
        }
        Ok(self)
    }
}
impl<T: Send + Sync + 'static, L: InputLayout<Value = T>, M> InputInterface<SinglePort<T, L>, M> {
    pub fn single(layout: LayoutMode<T, L>) -> Result<Self, String> {
        Self::new().port(InletPort::new("run"), layout)
    }
}

/// A public selector cannot be connected as if it were a physical input.
pub struct PublicInlets<I, M> {
    inlets: Inlets<I, M>,
}
pub struct PublicView<T, M> {
    view: InletView<T, M>,
}
impl<I, M> PublicInlets<I, M> {
    pub fn port<T, L>(&self, port: InletPort<I, T, L>) -> PublicView<T, M> {
        PublicView {
            view: self.inlets.port(port),
        }
    }
}
impl<T, L, M> PublicInlets<SinglePort<T, L>, M> {
    pub fn inlet(&self) -> PublicView<T, M> {
        PublicView {
            view: self.inlets.book.view(self.inlets.book.single_port()),
        }
    }
}
impl<T, M> PublicView<T, M> {
    pub fn field<U>(&self, key: impl FieldSelector<T, U>) -> PublicView<U, M> {
        PublicView {
            view: self.view.field(key),
        }
    }
}

/// Authoring facade over the ordinary GraphBuilder, not a runtime module node.
pub struct Module<I, M = LocalValue> {
    graph: GraphBuilder,
    public: PublicInlets<I, M>,
}
impl<I, M> std::ops::Deref for Module<I, M> {
    type Target = GraphBuilder;
    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}
impl<I, M> std::ops::DerefMut for Module<I, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.graph
    }
}
impl<I, M> Module<I, M> {
    pub fn new(name: impl Into<String>, interface: InputInterface<I, M>) -> Result<Self, String>
    where
        I: PortInterface,
        M: 'static,
    {
        I::validate_count(interface.ports.len())?;
        let local = TypeId::of::<M>() == TypeId::of::<LocalValue>();
        let encoded = TypeId::of::<M>() == TypeId::of::<EncodedValue>();
        if (!local && !encoded)
            || interface
                .slots
                .iter()
                .any(|(_, _, value)| value.local_type.is_some() != local)
        {
            return Err("public input representation does not match its interface mode".into());
        }
        if interface
            .ports
            .iter()
            .any(|port| !interface.slots.iter().any(|(p, _, _)| p == port))
        {
            return Err("public port has no layout slots".into());
        }
        let graph = GraphBuilder::new(name);
        let book = AddressBook {
            scope: graph.module.scope,
            declaration_id: graph.definition_id,
            ports: Arc::new(interface.ports.into_iter().collect()),
            inputs: Arc::new(
                interface
                    .slots
                    .into_iter()
                    .map(|(port, slot, value)| Address {
                        target: Target::Instance {
                            name: String::new(),
                            port: module_input_slot_name(&port, &slot),
                        },
                        port,
                        slot,
                        value,
                    })
                    .collect(),
            ),
            output: None,
        };
        Ok(Self {
            graph,
            public: PublicInlets {
                inlets: Inlets {
                    book,
                    marker: PhantomData,
                },
            },
        })
    }
    pub fn inlets(&self) -> PublicInlets<I, M> {
        PublicInlets {
            inlets: Inlets {
                book: self.public.inlets.book.clone(),
                marker: PhantomData,
            },
        }
    }
    /// Repeated public leaves append fanout; identical projections are rejected.
    pub fn expose<T: Send + Sync + 'static>(
        &mut self,
        public: PublicView<T, M>,
        to: Input<T, M>,
    ) -> Result<PublicInput, String>
    where
        M: ValueMode<T>,
    {
        let selected = public.view.target()?;
        if public.view.book.scope != self.graph.module.scope
            || public.view.book.declaration_id != self.graph.definition_id
        {
            return Err("public selector belongs to another module".into());
        }
        if to.scope != self.graph.module.scope
            || self.graph.target_declaration_id(&to.source)? != to.declaration_id
        {
            return Err("projection destination belongs to another or stale declaration".into());
        }
        if identity(&selected.schema, selected.local_type) != identity(&to.schema, to.local_type) {
            return Err("projection type mismatch".into());
        }
        let target = self
            .graph
            .verify_target(&to.source, identity(&to.schema, to.local_type))?;
        let slot = if public.view.path.is_empty() {
            "value"
        } else {
            &public.view.path
        };
        let name = module_input_slot_name(&public.view.port, slot);
        if let Some(input) = self.graph.module.inputs.iter_mut().find(|i| i.name == name) {
            if input.targets.contains(&target) {
                return Err("duplicate public input projection".into());
            }
            input.targets.push(target);
            return Ok(PublicInput {
                scope: self.graph.module.scope,
                declaration_id: self.graph.public_input_ids[&name],
                id: input.id,
                name,
                schema: input.schema.clone(),
                local_type: input.local_type,
            });
        }
        self.graph
            .expose_input_typed(public.view.port, slot.into(), to)
    }
    pub fn finish<O: Send + Sync + 'static, OM>(
        mut self,
        output: OutputRef<O, OM>,
    ) -> Result<TypedModulePackage<I, O, M, OM>, String>
    where
        I: PortInterface,
    {
        I::validate_count(self.public.inlets.book.ports.len())?;
        for address in self.public.inlets.book.inputs.iter() {
            let name = module_input_slot_name(&address.port, &address.slot);
            if !self.graph.module.inputs.iter().any(|i| {
                i.name == name
                    && i.identity() == identity(&address.value.schema, address.value.local_type)
            }) {
                return Err(format!(
                    "public input {}.{} has no projection",
                    address.port, address.slot
                ));
            }
        }
        if self.graph.module.inputs.len() != self.public.inlets.book.inputs.len() {
            return Err("module exports inputs outside its declared interface".into());
        }
        self.graph.expose_output("value", output)?;
        self.graph.inspect("").map_err(|e| e.to_string())?;
        Ok(TypedModulePackage {
            package: self.graph.module(),
            marker: PhantomData,
            modes: PhantomData,
        })
    }
}
impl<T, L, M> Module<SinglePort<T, L>, M> {
    pub fn inlet(&self) -> PublicView<T, M> {
        self.inlets().inlet()
    }
}

pub struct TypedModulePackage<I, O: Send + Sync + 'static, IM = LocalValue, OM = IM> {
    package: ModulePackage,
    marker: PhantomData<fn(I) -> O>,
    modes: PhantomData<fn(IM) -> OM>,
}
impl<I, O: Send + Sync + 'static, IM, OM> TypedModulePackage<I, O, IM, OM> {
    pub fn untyped(&self) -> &ModulePackage {
        &self.package
    }
    pub fn compile(self) -> Result<Arc<CompiledGraph>, CompileDiagnostic> {
        self.package.compile()
    }
}
impl<I, O: Send + Sync + 'static, IM, OM> replacement_private::Sealed
    for TypedModulePackage<I, O, IM, OM>
{
}
impl<I, O: Send + Sync + 'static, IM, OM> replacement_private::Sealed
    for &TypedModulePackage<I, O, IM, OM>
{
}
impl<I, O: Send + Sync + 'static, IM, OM> Replacement for TypedModulePackage<I, O, IM, OM> {
    fn apply(self, graph: &mut GraphBuilder, name: &str) -> Result<(), String> {
        graph.replace_instance_impl(name, &self.package)
    }
    fn kind() -> ReplacementKind {
        ReplacementKind::Module
    }
}
impl<I, O: Send + Sync + 'static, IM, OM> Replacement for &TypedModulePackage<I, O, IM, OM> {
    fn apply(self, graph: &mut GraphBuilder, name: &str) -> Result<(), String> {
        graph.replace_instance_impl(name, &self.package)
    }
    fn kind() -> ReplacementKind {
        ReplacementKind::Module
    }
}
impl GraphBuilder {
    /// Discover only a direct node or public module boundary, never child internals.
    pub fn discover(&self, name: &str) -> Result<DiscoveredOperation, String> {
        let pending = self.pending.borrow();
        let book = if let Some(node) = pending.get(name).or_else(|| self.module.named_node(name)) {
            validate_endpoints(node)?;
            AddressBook::node(self.module.scope, node)
        } else {
            let entry = self.dependency_for_instance(name)?;
            AddressBook::instance(
                self.module.scope,
                self.declaration_id(name, true)?,
                name,
                &entry.definition,
            )
        };
        Ok(DiscoveredOperation { book })
    }
    pub fn instantiate<I, O: Send + Sync + 'static, IM, OM>(
        &mut self,
        name: impl Into<String>,
        package: &TypedModulePackage<I, O, IM, OM>,
    ) -> Result<Operation<I, O, IM, OM>, String> {
        let handle = self.instance(name, &package.package)?;
        Ok(Operation {
            book: AddressBook::instance(
                handle.scope,
                handle.declaration_id,
                &handle.name,
                &package.package.root.definition,
            ),
            marker: PhantomData,
            modes: PhantomData,
        })
    }
}
