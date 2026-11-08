//! Graph-owned structural interfaces and explicit semantic contracts.
use crate::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfacePhase {
    Definition,
    Authoring,
    Compiled,
}

/// Context shared by construction, inspection, binding and final validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceError {
    pub contract: Option<Box<str>>,
    pub path: Box<str>,
    pub phase: InterfacePhase,
    pub endpoint: Option<Box<str>>,
    pub expected: Option<TypeIdentity>,
    pub actual: Option<TypeIdentity>,
    pub message: Box<str>,
}
impl InterfaceError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            contract: None,
            path: "".into(),
            phase: InterfacePhase::Definition,
            endpoint: None,
            expected: None,
            actual: None,
            message: message.into().into_boxed_str(),
        }
    }
    pub(crate) fn context(
        mut self,
        phase: InterfacePhase,
        path: &str,
        contract: Option<&str>,
    ) -> Self {
        self.phase = phase;
        self.path = path.into();
        self.contract = contract.map(Box::<str>::from);
        self
    }
}
impl fmt::Display for InterfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} interface at {:?}", self.phase, self.path)?;
        if let Some(id) = &self.contract {
            write!(f, " contract {id:?}")?;
        }
        if let Some(endpoint) = &self.endpoint {
            write!(f, " endpoint {endpoint}")?;
        }
        write!(f, ": {}", self.message)?;
        if self.expected.is_some() || self.actual.is_some() {
            write!(
                f,
                " (expected {:?}, actual {:?})",
                self.expected, self.actual
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for InterfaceError {}
fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'@'))
}
pub(crate) fn value_identity(value: DeclaredValue) -> TypeIdentity {
    identity(value.schema, value.local_type)
}

/// ID-free complete boundary snapshot. Unnamed endpoints retain numeric addresses
/// separately and can never satisfy named requirements.
#[derive(Clone, Debug, Default)]
pub struct InterfaceShape {
    inputs: BTreeMap<String, BTreeMap<String, DeclaredValue>>,
    outputs: BTreeMap<String, DeclaredValue>,
    unnamed_inputs: BTreeMap<(u16, u16), TypeIdentity>,
    unnamed_outputs: BTreeMap<u16, TypeIdentity>,
}
impl PartialEq for InterfaceShape {
    fn eq(&self, other: &Self) -> bool {
        self.same_shape(other)
    }
}
impl Eq for InterfaceShape {}
impl InterfaceShape {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn node(sig: NodeSignature) -> Result<Self, InterfaceError> {
        Self::new()
            .input_slot("run", "value", sig.input)?
            .output("value", sig.output)
    }
    pub fn input_slot(
        mut self,
        port: impl Into<String>,
        slot: impl Into<String>,
        value: DeclaredValue,
    ) -> Result<Self, InterfaceError> {
        let (port, slot) = (port.into(), slot.into());
        validate_name_segment(&port).map_err(|e| InterfaceError::new(e.to_string()))?;
        validate_name_segment(&slot).map_err(|e| InterfaceError::new(e.to_string()))?;
        if self
            .inputs
            .entry(port.clone())
            .or_default()
            .insert(slot.clone(), value)
            .is_some()
        {
            return Err(InterfaceError::new(format!(
                "duplicate input slot name {port}.{slot}"
            )));
        }
        Ok(self)
    }
    pub fn output(
        mut self,
        name: impl Into<String>,
        value: DeclaredValue,
    ) -> Result<Self, InterfaceError> {
        let name = name.into();
        validate_name_segment(&name).map_err(|e| InterfaceError::new(e.to_string()))?;
        if self.outputs.insert(name.clone(), value).is_some() {
            return Err(InterfaceError::new(format!("duplicate output {name}")));
        }
        Ok(self)
    }
    pub fn inputs(&self) -> impl Iterator<Item = (&str, &str, DeclaredValue)> {
        self.inputs
            .iter()
            .flat_map(|(p, slots)| slots.iter().map(move |(s, v)| (p.as_str(), s.as_str(), *v)))
    }
    pub fn outputs(&self) -> impl Iterator<Item = (&str, DeclaredValue)> {
        self.outputs.iter().map(|(n, v)| (n.as_str(), *v))
    }
    /// Exact input ports, slots and identities; additional outputs are allowed.
    pub fn check_shape(&self, actual: &Self) -> Result<(), InterfaceError> {
        for port in self.inputs.keys() {
            if !actual.inputs.contains_key(port) {
                return Err(InterfaceError::new(format!("missing input port {port}")));
            }
        }
        for (port, slot, value) in self.inputs() {
            check_endpoint(
                format!("input {port}.{slot}"),
                value_identity(value),
                actual
                    .inputs
                    .get(port)
                    .and_then(|p| p.get(slot))
                    .copied()
                    .map(value_identity),
            )?;
        }
        for (port, slots) in &actual.inputs {
            if !self.inputs.contains_key(port) {
                let mut error = InterfaceError::new("unexpected input port");
                error.endpoint = Some(format!("input port {port}").into());
                return Err(error);
            }
            for (slot, value) in slots {
                if !self.inputs[port].contains_key(slot) {
                    let mut error = InterfaceError::new("unexpected input slot");
                    error.endpoint = Some(format!("input {port}.{slot}").into());
                    error.actual = Some(value_identity(*value));
                    return Err(error);
                }
            }
        }
        if self.unnamed_inputs != actual.unnamed_inputs {
            return Err(InterfaceError::new(
                "unnamed input set or identity mismatch",
            ));
        }
        for (name, value) in self.outputs() {
            check_endpoint(
                format!("output {name}"),
                value_identity(value),
                actual.outputs.get(name).copied().map(value_identity),
            )?;
        }
        if !self
            .unnamed_outputs
            .iter()
            .all(|(k, v)| actual.unnamed_outputs.get(k) == Some(v))
        {
            return Err(InterfaceError::new("unnamed endpoint mismatch"));
        }
        Ok(())
    }
    pub fn same_shape(&self, actual: &Self) -> bool {
        self.inputs.keys().eq(actual.inputs.keys())
            && self.check_shape(actual).is_ok()
            && actual.check_shape(self).is_ok()
    }
    pub(crate) fn node_ports(
        ports: &[Port],
        outputs: &[OutputSlot],
    ) -> Result<Self, InterfaceError> {
        let mut shape = Self::new();
        let mut port_ids = BTreeSet::new();
        for port in ports {
            if port.slots.is_empty() {
                return Err(InterfaceError::new("empty input port"));
            }
            if !port_ids.insert(port.id) {
                return Err(InterfaceError::new("duplicate input port ID"));
            }
            let mut slot_ids = BTreeSet::new();
            if let Some(name) = &port.name {
                validate_name_segment(name).map_err(|e| InterfaceError::new(e.to_string()))?;
                if shape.inputs.insert(name.clone(), BTreeMap::new()).is_some() {
                    return Err(InterfaceError::new("duplicate input port name"));
                }
            }
            let mut slot_names = BTreeSet::new();
            for slot in &port.slots {
                if !slot_ids.insert(slot.id) {
                    return Err(InterfaceError::new("duplicate input slot ID"));
                }
                if let Some(name) = &slot.name {
                    validate_name_segment(name).map_err(|e| InterfaceError::new(e.to_string()))?;
                    if !slot_names.insert(name) {
                        return Err(InterfaceError::new("duplicate input slot name"));
                    }
                }
                if let (Some(p), Some(s)) = (&port.name, &slot.name) {
                    shape = shape.input_slot(
                        p,
                        s,
                        DeclaredValue {
                            schema: slot.schema,
                            local_type: slot.local_type,
                        },
                    )?;
                } else if shape
                    .unnamed_inputs
                    .insert((port.id.0, slot.id.0), slot.identity())
                    .is_some()
                {
                    return Err(InterfaceError::new("duplicate unnamed input"));
                }
            }
        }
        let mut output_ids = BTreeSet::new();
        for slot in outputs {
            if !output_ids.insert(slot.id) {
                return Err(InterfaceError::new("duplicate output ID"));
            }
            if let Some(name) = &slot.name {
                shape = shape.output(
                    name,
                    DeclaredValue {
                        schema: slot.schema,
                        local_type: slot.local_type,
                    },
                )?;
            } else if shape
                .unnamed_outputs
                .insert(slot.id.0, slot.identity())
                .is_some()
            {
                return Err(InterfaceError::new("duplicate unnamed output"));
            }
        }
        Ok(shape)
    }
    pub(crate) fn module_ports(
        inputs: &[ModuleInput],
        outputs: &[ModuleOutput],
    ) -> Result<Self, InterfaceError> {
        let mut shape = Self::new();
        for p in inputs {
            shape = shape.input_slot(
                &p.port,
                &p.slot,
                DeclaredValue {
                    schema: p.schema,
                    local_type: p.local_type,
                },
            )?;
        }
        for p in outputs {
            shape = shape.output(
                &p.name,
                DeclaredValue {
                    schema: p.schema,
                    local_type: p.local_type,
                },
            )?;
        }
        Ok(shape)
    }
}
fn check_endpoint(
    endpoint: String,
    expected: TypeIdentity,
    actual: Option<TypeIdentity>,
) -> Result<(), InterfaceError> {
    if actual == Some(expected) {
        return Ok(());
    }
    let mut error = InterfaceError::new("missing or incompatible endpoint");
    error.endpoint = Some(endpoint.into());
    error.expected = Some(expected);
    error.actual = actual;
    Err(error)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphContract {
    id: String,
    required: InterfaceShape,
}
impl GraphContract {
    pub fn new(id: impl Into<String>, required: InterfaceShape) -> Result<Self, InterfaceError> {
        let id = id.into();
        if id.len() > 512 || !id.split('/').all(valid_segment) {
            return Err(InterfaceError::new("invalid contract identity"));
        }
        Ok(Self { id, required })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn required(&self) -> &InterfaceShape {
        &self.required
    }
    /// Structural validation only; does not require semantic membership.
    pub fn check_shape(&self, unit: &dyn GraphUnit) -> Result<(), InterfaceError> {
        let snapshot = unit.inspect().map_err(|mut error| {
            error.contract = Some(self.id().into());
            error
        })?;
        self.required.check_shape(&snapshot.shape).map_err(|error| {
            error.context(
                snapshot.provenance.phase,
                &snapshot.provenance.path,
                Some(self.id()),
            )
        })
    }
    /// Require an explicit, exactly matching definition and the actual shape.
    pub fn check_implementation(&self, unit: &dyn GraphUnit) -> Result<(), InterfaceError> {
        unit.inspect()
            .map_err(|mut error| {
                error.contract = Some(self.id().into());
                error
            })?
            .check_contract(self)
    }
    /// Definition compatibility is exact, unlike structural satisfaction.
    pub fn check_definition(&self, provided: &Self) -> Result<(), InterfaceError> {
        if self != provided {
            return Err(
                InterfaceError::new("different contract identity or definition").context(
                    InterfacePhase::Definition,
                    "",
                    Some(self.id()),
                ),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitProvenance {
    pub phase: InterfacePhase,
    pub definition: String,
    pub path: String,
    pub instance_path: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitSnapshot {
    pub shape: InterfaceShape,
    pub claims: Vec<GraphContract>,
    pub provenance: UnitProvenance,
}
pub(crate) mod sealed {
    pub trait Sealed {}
}
/// Authoritative views only; snapshots and cached authoring handles are not units.
/// External objects cannot attest graph membership:
///
/// ```compile_fail
/// use phenix_graph::{GraphUnit, UnitSnapshot, InterfaceError};
/// struct Fabricated;
/// impl GraphUnit for Fabricated {
///     fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> { unimplemented!() }
/// }
/// ```
///
/// Cached declaration handles are intentionally not authoritative views:
///
/// ```compile_fail
/// use phenix_graph::{GraphBuilder, GraphUnit};
/// let mut graph = GraphBuilder::new("root");
/// let handle = graph.node("worker", |x: i64| x).unwrap();
/// let unit: &dyn GraphUnit = &handle;
/// ```
pub trait GraphUnit: sealed::Sealed {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError>;
}
impl UnitSnapshot {
    pub fn check_contract(&self, contract: &GraphContract) -> Result<(), InterfaceError> {
        let result = (|| {
            let mut claims = self
                .claims
                .iter()
                .filter(|c| c.id() == contract.id())
                .peekable();
            if claims.peek().is_none() {
                return Err(InterfaceError::new("missing explicit contract claim"));
            }
            for claim in claims {
                contract.check_definition(claim)?;
            }
            contract.required.check_shape(&self.shape)
        })();
        result.map_err(|e| {
            e.context(
                self.provenance.phase,
                &self.provenance.path,
                Some(contract.id()),
            )
        })
    }
}

fn snapshot(
    shape: Result<InterfaceShape, InterfaceError>,
    claims: &[GraphContract],
    phase: InterfacePhase,
    definition: &str,
    path: &str,
    instance_path: &[String],
) -> Result<UnitSnapshot, InterfaceError> {
    Ok(UnitSnapshot {
        shape: shape.map_err(|e| e.context(phase, path, None))?,
        claims: claims.to_vec(),
        provenance: UnitProvenance {
            phase,
            definition: definition.into(),
            path: path.into(),
            instance_path: instance_path.to_vec(),
        },
    })
}
pub(crate) fn declaration_snapshot(
    def: &ModuleDef,
    path: &str,
    phase: InterfacePhase,
) -> Result<UnitSnapshot, InterfaceError> {
    snapshot(
        InterfaceShape::module_ports(&def.inputs, &def.outputs),
        &def.claims,
        phase,
        &def.id,
        path,
        &[],
    )
}
pub(crate) fn node_snapshot(
    node: &NodeTemplate,
    definition: &str,
    path: &str,
    phase: InterfacePhase,
    instances: &[String],
) -> Result<UnitSnapshot, InterfaceError> {
    snapshot(
        InterfaceShape::node_ports(&node.ports, &node.outputs),
        &node.claims,
        phase,
        definition,
        path,
        instances,
    )
}
impl sealed::Sealed for CompiledGraph {}
/// The compiled graph itself is the authoritative root boundary at path `""`.
/// Named lookup remains separate: `find("")` does not select the root.
impl GraphUnit for CompiledGraph {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> {
        let def = self
            .metadata
            .root_definition
            .as_ref()
            .and_then(|id| self.metadata.definitions.get(id))
            .ok_or_else(|| {
                InterfaceError::new("missing compiled root declaration").context(
                    InterfacePhase::Compiled,
                    "",
                    None,
                )
            })?;
        declaration_snapshot(def, "", InterfacePhase::Compiled)
    }
}
impl CompiledGraph {
    /// Bind the public root boundary, independently of named unit lookup.
    /// This does not attest a single contribution origin for the composed root;
    /// kernel offers using named lookup still reject the empty root path.
    pub fn bind_contract(&self, contract: &GraphContract) -> Result<BoundUnit<'_>, InterfaceError> {
        contract.check_implementation(self)?;
        Ok(BoundUnit {
            graph: self,
            path: String::new(),
            contract: contract.clone(),
        })
    }
}
impl sealed::Sealed for NodeRef<'_> {}
impl GraphUnit for NodeRef<'_> {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> {
        let origin = self
            .graph
            .metadata
            .nodes
            .get(&self.id)
            .ok_or_else(|| InterfaceError::new("missing node provenance"))?;
        let def = &self.graph.metadata.definitions[&origin.definition];
        let node = def
            .nodes
            .iter()
            .find(|n| n.id == origin.local_node)
            .ok_or_else(|| InterfaceError::new("missing node declaration"))?;
        let path = origin
            .instance_path
            .iter()
            .map(String::as_str)
            .chain(node.name.as_deref())
            .collect::<Vec<_>>()
            .join("/");
        let physical = self
            .graph
            .nodes
            .get(&self.id)
            .ok_or_else(|| InterfaceError::new("missing physical node"))?;
        snapshot(
            InterfaceShape::node_ports(&physical.ports, &physical.outputs),
            &node.claims,
            InterfacePhase::Compiled,
            &origin.definition,
            &path,
            &origin.instance_path,
        )
    }
}
impl sealed::Sealed for ModuleRef<'_> {}
impl GraphUnit for ModuleRef<'_> {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> {
        let def = &self.graph.metadata.definitions[&self.lookup.definition];
        snapshot(
            InterfaceShape::module_ports(&def.inputs, &def.outputs),
            &def.claims,
            InterfacePhase::Compiled,
            &def.id,
            &self.qualified_name(),
            &self.lookup.instance_path,
        )
    }
}
impl sealed::Sealed for FindResult<'_> {}
impl GraphUnit for FindResult<'_> {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> {
        match self {
            Self::Node(n) => n.inspect(),
            Self::Module(m) => m.inspect(),
        }
    }
}

/// A projection tied to one immutable compiled graph, not a transferable receipt.
#[derive(Clone)]
pub struct BoundUnit<'g> {
    graph: &'g CompiledGraph,
    path: String,
    contract: GraphContract,
}

/// A compiled input projection retaining its owning graph borrow.
/// This is not a standalone code lease; the compiled graph must remain alive.
pub struct BoundInput<'g> {
    unit: BoundUnit<'g>,
    endpoint: String,
    targets: Vec<InputAddr>,
}
impl BoundInput<'_> {
    /// Explicit low-level extraction, checked against the owning compiled graph.
    /// Raw addresses carry no provenance and are not validated handles once extracted.
    pub fn resolve(&self, graph: &CompiledGraph) -> Result<&[InputAddr], InterfaceError> {
        self.unit.check_graph(graph, &self.endpoint)?;
        Ok(&self.targets)
    }
}

/// A compiled output projection retaining its owning graph borrow.
/// This is not a standalone code lease; the compiled graph must remain alive.
pub struct BoundOutput<'g> {
    unit: BoundUnit<'g>,
    endpoint: String,
    source: OutputAddr,
}
impl BoundOutput<'_> {
    /// Explicit low-level extraction, checked against the owning compiled graph.
    /// The raw address carries no provenance and is not a validated handle once extracted.
    pub fn resolve(&self, graph: &CompiledGraph) -> Result<OutputAddr, InterfaceError> {
        self.unit.check_graph(graph, &self.endpoint)?;
        Ok(self.source)
    }
}
impl<'g> FindResult<'g> {
    pub fn bind_contract(self, contract: &GraphContract) -> Result<BoundUnit<'g>, InterfaceError> {
        let snapshot = self.inspect()?;
        snapshot.check_contract(contract)?;
        let graph = match self {
            Self::Node(n) => n.graph,
            Self::Module(m) => m.graph,
        };
        Ok(BoundUnit {
            graph,
            path: snapshot.provenance.path,
            contract: contract.clone(),
        })
    }
}
impl<'g> NodeRef<'g> {
    pub fn bind_contract(self, contract: &GraphContract) -> Result<BoundUnit<'g>, InterfaceError> {
        FindResult::Node(self).bind_contract(contract)
    }
}
impl<'g> ModuleRef<'g> {
    pub fn bind_contract(self, contract: &GraphContract) -> Result<BoundUnit<'g>, InterfaceError> {
        FindResult::Module(self).bind_contract(contract)
    }
}
impl<'g> BoundUnit<'g> {
    fn check_graph(&self, graph: &CompiledGraph, endpoint: &str) -> Result<(), InterfaceError> {
        if !std::ptr::eq(self.graph, graph) {
            return Err(self.lookup_error(
                endpoint.into(),
                "endpoint belongs to a different compiled graph generation",
            ));
        }
        Ok(())
    }
    fn lookup_error(&self, endpoint: String, message: &str) -> InterfaceError {
        let mut error = InterfaceError::new(message).context(
            InterfacePhase::Compiled,
            &self.path,
            Some(self.contract.id()),
        );
        error.endpoint = Some(endpoint.into());
        error
    }
    pub fn input(&self, port: &str, slot: &str) -> Result<BoundInput<'g>, InterfaceError> {
        if !self
            .contract
            .required
            .inputs
            .get(port)
            .is_some_and(|p| p.contains_key(slot))
        {
            return Err(self.lookup_error(
                format!("input {port}.{slot}"),
                "input is not promised by contract",
            ));
        }
        let result = if self.path.is_empty() {
            self.graph
                .metadata
                .root_definition
                .as_ref()
                .and_then(|id| self.graph.metadata.definitions.get(id))
                .and_then(|def| def.inputs.iter().find(|p| p.port == port && p.slot == slot))
                .and_then(|p| self.graph.metadata.inputs.get(&p.id).cloned())
        } else {
            let unit = self.graph.find(&self.path).ok_or_else(|| {
                self.lookup_error(format!("input {port}.{slot}"), "compiled unit is missing")
            })?;
            match unit {
                FindResult::Node(n) => n.input(port, slot).map(|a| vec![a]),
                FindResult::Module(m) => {
                    let def = &m.graph.metadata.definitions[&m.lookup.definition];
                    def.inputs
                        .iter()
                        .find(|p| p.port == port && p.slot == slot)
                        .and_then(|p| {
                            m.graph.metadata.instances[m.lookup.instance_metadata]
                                .inputs
                                .get(&p.id)
                                .cloned()
                        })
                }
            }
        };
        let targets = result.ok_or_else(|| {
            self.lookup_error(format!("input {port}.{slot}"), "compiled input is missing")
        })?;
        Ok(BoundInput {
            unit: self.clone(),
            endpoint: format!("input {port}.{slot}"),
            targets,
        })
    }
    pub fn output(&self, name: &str) -> Result<BoundOutput<'g>, InterfaceError> {
        if !self.contract.required.outputs.contains_key(name) {
            return Err(self.lookup_error(
                format!("output {name}"),
                "output is not promised by contract",
            ));
        }
        let result = if self.path.is_empty() {
            self.graph
                .metadata
                .root_definition
                .as_ref()
                .and_then(|id| self.graph.metadata.definitions.get(id))
                .and_then(|def| def.outputs.iter().find(|p| p.name == name))
                .and_then(|p| self.graph.metadata.outputs.get(&p.id).copied())
        } else {
            let unit = self.graph.find(&self.path).ok_or_else(|| {
                self.lookup_error(format!("output {name}"), "compiled unit is missing")
            })?;
            match unit {
                FindResult::Node(n) => n.output(name),
                FindResult::Module(m) => {
                    let def = &m.graph.metadata.definitions[&m.lookup.definition];
                    def.outputs.iter().find(|p| p.name == name).and_then(|p| {
                        m.graph.metadata.instances[m.lookup.instance_metadata]
                            .outputs
                            .get(&p.id)
                            .copied()
                    })
                }
            }
        };
        let source = result.ok_or_else(|| {
            self.lookup_error(format!("output {name}"), "compiled output is missing")
        })?;
        Ok(BoundOutput {
            unit: self.clone(),
            endpoint: format!("output {name}"),
            source,
        })
    }
}

pub(crate) fn validate_metadata(metadata: &CompiledMetadata) -> Result<(), InterfaceError> {
    let mut definitions: BTreeMap<String, GraphContract> = BTreeMap::new();
    let mut check = |unit: UnitSnapshot| -> Result<(), InterfaceError> {
        for claim in &unit.claims {
            if let Some(old) = definitions.get(claim.id()) {
                old.check_definition(claim).map_err(|e| {
                    e.context(
                        InterfacePhase::Compiled,
                        &unit.provenance.path,
                        Some(claim.id()),
                    )
                })?;
            } else {
                definitions.insert(claim.id().into(), claim.clone());
            }
            unit.check_contract(claim)?;
        }
        Ok(())
    };
    // Walk every expanded declaration, including the reusable root boundary.
    if let Some(root) = &metadata.root_definition {
        check(declaration_snapshot(
            &metadata.definitions[root],
            "",
            InterfacePhase::Compiled,
        )?)?;
    }
    let mut instances = metadata.instances.iter().collect::<Vec<_>>();
    instances.sort_by(|a, b| a.instance_path.cmp(&b.instance_path));
    for instance in instances {
        check(declaration_snapshot(
            &metadata.definitions[&instance.definition],
            &instance.instance_path.join("/"),
            InterfacePhase::Compiled,
        )?)?;
    }
    for origin in metadata.nodes.values() {
        let def = &metadata.definitions[&origin.definition];
        let node = def
            .nodes
            .iter()
            .find(|n| n.id == origin.local_node)
            .ok_or_else(|| InterfaceError::new("missing node declaration"))?;
        let path = origin
            .instance_path
            .iter()
            .map(String::as_str)
            .chain(node.name.as_deref())
            .collect::<Vec<_>>()
            .join("/");
        check(node_snapshot(
            node,
            &def.id,
            &path,
            InterfacePhase::Compiled,
            &origin.instance_path,
        )?)?;
    }
    for (id, def) in &metadata.definitions {
        for (path, contract) in &def.contract_requests {
            if let Some(old) = definitions.get(contract.id()) {
                old.check_definition(contract)
                    .map_err(|e| e.context(InterfacePhase::Compiled, path, Some(contract.id())))?;
            } else {
                definitions.insert(contract.id().into(), contract.clone());
            }
            let resolve = || -> Result<UnitSnapshot, InterfaceError> {
                let mut current = def.as_ref();
                let parts = if path.is_empty() {
                    Vec::new()
                } else {
                    parse_name_path(path)
                        .ok_or_else(|| InterfaceError::new("invalid request path"))?
                };
                for (index, part) in parts.iter().enumerate() {
                    if index + 1 == parts.len()
                        && let Some(node) = current
                            .nodes
                            .iter()
                            .find(|n| n.name.as_deref() == Some(*part))
                    {
                        return node_snapshot(
                            node,
                            &current.id,
                            path,
                            InterfacePhase::Compiled,
                            &[],
                        );
                    }
                    let instance = current
                        .instances
                        .iter()
                        .find(|i| i.name == *part)
                        .ok_or_else(|| InterfaceError::new("requested unit is missing"))?;
                    current = metadata
                        .definitions
                        .get(&instance.definition)
                        .ok_or_else(|| InterfaceError::new("requested definition is missing"))?;
                }
                declaration_snapshot(current, path, InterfacePhase::Compiled)
            };
            resolve()
                .and_then(|unit| unit.check_contract(contract))
                .map_err(|e| {
                    e.context(
                        InterfacePhase::Compiled,
                        &format!("{id}/{path}"),
                        Some(contract.id()),
                    )
                })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_identity_ignores_diagnostic_schema_names() {
        let a = DeclaredValue {
            schema: "first diagnostic",
            local_type: Some(TypeId::of::<i64>()),
        };
        let b = DeclaredValue {
            schema: "second diagnostic",
            ..a
        };
        let a = InterfaceShape::new().output("value", a).unwrap();
        let b = InterfaceShape::new().output("value", b).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn unnamed_endpoints_keep_numeric_addresses_without_inventing_names() {
        let make = |slot| {
            InterfaceShape::node_ports(
                &[Port::new(
                    4,
                    vec![InputSlot::local::<i64>(slot)],
                    AllSameCohort,
                )],
                &[OutputSlot::local::<i64>(slot)],
            )
            .unwrap()
        };
        let a = make(1);
        assert_eq!(a.inputs().count(), 0);
        assert_eq!(a.outputs().count(), 0);
        assert_ne!(a, InterfaceShape::new());
        assert_ne!(a, make(2));
        assert!(
            InterfaceShape::new()
                .input_slot("run", "value", DeclaredValue::local::<i64>())
                .unwrap()
                .check_shape(&a)
                .is_err()
        );
    }

    #[test]
    fn independently_declared_wire_identity_and_local_identity_do_not_coerce() {
        let wire = InterfaceShape::new()
            .output("value", DeclaredValue::wire::<i64>())
            .unwrap();
        let local = InterfaceShape::new()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap();
        assert!(wire.check_shape(&local).is_err());
        assert!(local.check_shape(&wire).is_err());
        wire.check_shape(&wire).unwrap();
    }

    #[test]
    fn h07_unnamed_inputs_require_exact_sets_and_identities() {
        let make =
            |slots| InterfaceShape::node_ports(&[Port::new(4, slots, AllSameCohort)], &[]).unwrap();
        let required = make(vec![InputSlot::local::<i64>(0)]);
        let extra = make(vec![
            InputSlot::local::<i64>(0),
            InputSlot::local::<bool>(1),
        ]);
        let wrong_identity = make(vec![InputSlot::local::<bool>(0)]);
        assert!(required.check_shape(&extra).is_err());
        assert!(extra.check_shape(&required).is_err());
        assert!(required.check_shape(&InterfaceShape::new()).is_err());
        assert!(InterfaceShape::new().check_shape(&required).is_err());
        assert!(required.check_shape(&wrong_identity).is_err());
        required.check_shape(&required).unwrap();
        let extra_output = InterfaceShape {
            unnamed_outputs: BTreeMap::from([(7, TypeIdentity::Local(TypeId::of::<bool>()))]),
            ..required.clone()
        };
        required.check_shape(&extra_output).unwrap();
        assert!(!required.same_shape(&extra_output));
    }
}
