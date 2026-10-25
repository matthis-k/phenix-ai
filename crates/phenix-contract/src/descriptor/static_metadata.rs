//! Constant declarations projected into the accepted owned metadata protocol.
use super::{Cardinality, ContractDefinition, MetadataExecution, OperationSchema};
use crate::{Failure, StaticSchema};

#[derive(Clone, Copy, Debug)]
pub struct StaticOperation {
    pub name: &'static str,
    pub key: &'static str,
    pub request: StaticSchema,
    pub response: StaticSchema,
    pub error: StaticSchema,
}

#[derive(Clone, Copy, Debug)]
pub struct StaticContract {
    pub contract: &'static str,
    pub revision: u64,
    pub operations: &'static [StaticOperation],
}

#[derive(Clone, Copy, Debug)]
pub struct StaticImport {
    pub slot: &'static str,
    pub contract: &'static str,
    pub provider: Option<&'static str>,
    pub revision_min: u64,
    pub revision_max: u64,
    pub cardinality: Cardinality,
}

#[derive(Clone, Copy, Debug)]
pub struct StaticExport {
    pub slot: &'static str,
    pub contract: &'static str,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct StaticProperty {
    pub id: &'static str,
    pub schema: StaticSchema,
    pub get: bool,
    pub set: bool,
    pub watch: bool,
}

impl StaticContract {
    pub fn owned(self) -> Result<ContractDefinition, Failure> {
        Ok(ContractDefinition {
            contract: self.contract.into(),
            revision: self.revision,
            operations: self
                .operations
                .iter()
                .map(|operation| {
                    Ok(OperationSchema {
                        name: operation.name.into(),
                        request: operation.request.owned()?,
                        response: operation.response.owned()?,
                        error: operation.error.owned()?,
                    })
                })
                .collect::<Result<_, Failure>>()?,
        })
    }
}

pub const METADATA_BYTES: usize = 128 * 1024;
/// Exact ELF section size for the M1 native metadata profile, including its prefix.
pub const NATIVE_SECTION_BYTES: usize = 128 * 1024;

/// Canonical metadata bytes for an offline definition package.
pub struct MetadataBytes {
    data: [u8; METADATA_BYTES],
    len: usize,
}

impl MetadataBytes {
    pub fn bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// Embed a length-prefixed canonical payload with zero padding.
    pub const fn native_section(&self) -> [u8; NATIVE_SECTION_BYTES] {
        assert!(
            self.len <= NATIVE_SECTION_BYTES - 4,
            "native metadata exceeds section"
        );
        let mut section = [0; NATIVE_SECTION_BYTES];
        let prefix = (self.len as u32).to_be_bytes();
        let mut index = 0;
        while index < 4 {
            section[index] = prefix[index];
            index += 1;
        }
        index = 0;
        while index < self.len {
            section[index + 4] = self.data[index];
            index += 1;
        }
        section
    }

    const fn byte(&mut self, value: u8) {
        assert!(self.len < METADATA_BYTES, "metadata exceeds 128 KiB");
        self.data[self.len] = value;
        self.len += 1;
    }

    const fn raw(&mut self, bytes: &[u8]) {
        let mut index = 0;
        while index < bytes.len() {
            self.byte(bytes[index]);
            index += 1;
        }
    }

    const fn count(&mut self, count: usize) {
        assert!(count <= crate::MAX_COUNT, "metadata count limit");
        self.raw(&(count as u32).to_be_bytes());
    }

    const fn key(&mut self, text: &str) {
        self.count(text.len());
        self.raw(text.as_bytes());
    }

    const fn text(&mut self, text: &str) {
        self.byte(5);
        self.key(text);
    }

    const fn list(&mut self, count: usize) {
        self.byte(7);
        self.count(count);
    }

    const fn map(&mut self, count: usize) {
        self.byte(8);
        self.count(count);
    }

    const fn number(&mut self, value: u64) {
        self.byte(4);
        self.raw(&value.to_be_bytes());
    }

    const fn boolean(&mut self, value: bool) {
        self.byte(if value { 2 } else { 1 });
    }

    const fn schema(&mut self, schema: StaticSchema) {
        let encoded = schema.encode();
        self.byte(6);
        self.count(encoded.len());
        let mut index = 0;
        while index < encoded.len() {
            self.byte(encoded.at(index));
            index += 1;
        }
    }

    const fn contract(&mut self, contract: StaticContract) {
        assert!(contract_id(contract.contract), "invalid contract identity");
        assert!(contract.revision > 0, "reserved contract revision");
        assert!(!contract.operations.is_empty(), "empty contract");
        self.map(3);
        self.key("contract");
        self.text(contract.contract);
        self.key("operations");
        self.list(contract.operations.len());
        let mut index = 0;
        while index < contract.operations.len() {
            let operation = contract.operations[index];
            assert!(identity(operation.name), "invalid operation name");
            assert!(
                operation_identity(contract.contract, operation),
                "operation key differs from contract"
            );
            if index > 0 {
                assert!(
                    crate::static_schema::compare(
                        contract.operations[index - 1].name,
                        operation.name
                    ) < 0,
                    "operation names must be sorted and unique"
                );
            }
            self.map(4);
            self.key("error");
            self.schema(operation.error);
            self.key("name");
            self.text(operation.name);
            self.key("request");
            self.schema(operation.request);
            self.key("response");
            self.schema(operation.response);
            index += 1;
        }
        self.key("revision");
        self.number(contract.revision);
    }
}

const fn identity(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 256 {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if !byte.is_ascii_alphanumeric() && byte != b'.' && byte != b'_' && byte != b'-' {
            return false;
        }
        index += 1;
    }
    true
}

const fn contract_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut split = 0;
    while split < bytes.len() && bytes[split] != b'@' {
        split += 1;
    }
    if split == 0 || split > 256 || split + 1 >= bytes.len() {
        return false;
    }
    let mut index = 0;
    while index < split {
        let byte = bytes[index];
        if !byte.is_ascii_alphanumeric() && byte != b'.' && byte != b'_' && byte != b'-' {
            return false;
        }
        index += 1;
    }
    index = split + 1;
    if bytes[index] == b'0' {
        return false;
    }
    let mut major = 0u64;
    while index < bytes.len() {
        let byte = bytes[index];
        if !byte.is_ascii_digit() {
            return false;
        }
        major = major * 10 + (byte - b'0') as u64;
        if major > u32::MAX as u64 {
            return false;
        }
        index += 1;
    }
    major > 0
}

const fn operation_identity(contract: &str, operation: StaticOperation) -> bool {
    let key = operation.key.as_bytes();
    let contract = contract.as_bytes();
    let name = operation.name.as_bytes();
    if key.len() != contract.len() + 1 + name.len() {
        return false;
    }
    let mut index = 0;
    while index < contract.len() {
        if key[index] != contract[index] {
            return false;
        }
        index += 1;
    }
    if key[index] != b'/' {
        return false;
    }
    index = 0;
    while index < name.len() {
        if key[contract.len() + 1 + index] != name[index] {
            return false;
        }
        index += 1;
    }
    true
}

pub const fn definitions<const N: usize>(
    plugin: &str,
    version: &str,
    contracts: [StaticContract; N],
) -> MetadataBytes {
    metadata(
        plugin,
        version,
        MetadataExecution::None,
        contracts,
        [],
        [],
        [],
    )
}

/// Check exported handler coverage without calling plugin code.
pub const fn validate_handlers<const P: usize, const H: usize>(
    contracts: [StaticContract; P],
    handlers: [StaticOperation; H],
) {
    let mut handler = 0;
    while handler < H {
        let mut other = 0;
        while other < handler {
            assert!(
                crate::static_schema::compare(handlers[other].key, handlers[handler].key) != 0,
                "duplicate exported handler"
            );
            other += 1;
        }
        let mut found = false;
        let mut contract = 0;
        while contract < P {
            let mut operation = 0;
            while operation < contracts[contract].operations.len() {
                let expected = contracts[contract].operations[operation];
                if crate::static_schema::compare(expected.key, handlers[handler].key) == 0 {
                    assert!(
                        same_schema(expected.request, handlers[handler].request)
                            && same_schema(expected.response, handlers[handler].response)
                            && same_schema(expected.error, handlers[handler].error),
                        "exported handler schema mismatch"
                    );
                    found = true;
                }
                operation += 1;
            }
            contract += 1;
        }
        assert!(found, "handler has no provided contract");
        handler += 1;
    }
    let mut contract = 0;
    while contract < P {
        let mut operation = 0;
        while operation < contracts[contract].operations.len() {
            let mut found = false;
            handler = 0;
            while handler < H {
                if crate::static_schema::compare(
                    contracts[contract].operations[operation].key,
                    handlers[handler].key,
                ) == 0
                {
                    found = true;
                }
                handler += 1;
            }
            assert!(found, "provided operation has no handler");
            operation += 1;
        }
        contract += 1;
    }
}

const fn same_schema(left: StaticSchema, right: StaticSchema) -> bool {
    let left = left.encode();
    let right = right.encode();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left.at(index) != right.at(index) {
            return false;
        }
        index += 1;
    }
    true
}

/// Compile-time lowering into the same canonical protocol as owned metadata.
pub const fn metadata<const N: usize, const I: usize, const E: usize, const P: usize>(
    plugin: &str,
    version: &str,
    execution: MetadataExecution,
    mut contracts: [StaticContract; N],
    mut requires: [StaticImport; I],
    mut provides: [StaticExport; E],
    mut properties: [StaticProperty; P],
) -> MetadataBytes {
    assert!(
        identity(plugin) && crate::static_schema::compare(plugin, "root") != 0,
        "invalid plugin identity"
    );
    assert!(identity(version), "invalid plugin version");
    if let MetadataExecution::None = execution {
        assert!(
            I == 0 && E == 0 && P == 0,
            "definitions cannot contain executable contributions"
        );
    }
    let mut index = 1;
    while index < N {
        let mut at = index;
        while at > 0
            && crate::static_schema::compare(contracts[at - 1].contract, contracts[at].contract) > 0
        {
            contracts.swap(at - 1, at);
            at -= 1;
        }
        index += 1;
    }
    index = 1;
    while index < I {
        let mut at = index;
        while at > 0 && crate::static_schema::compare(requires[at - 1].slot, requires[at].slot) > 0
        {
            requires.swap(at - 1, at);
            at -= 1;
        }
        index += 1;
    }
    index = 1;
    while index < E {
        let mut at = index;
        while at > 0 && crate::static_schema::compare(provides[at - 1].slot, provides[at].slot) > 0
        {
            provides.swap(at - 1, at);
            at -= 1;
        }
        index += 1;
    }
    index = 1;
    while index < P {
        let mut at = index;
        while at > 0 && crate::static_schema::compare(properties[at - 1].id, properties[at].id) > 0
        {
            properties.swap(at - 1, at);
            at -= 1;
        }
        index += 1;
    }
    let mut output = MetadataBytes {
        data: [0; METADATA_BYTES],
        len: 0,
    };
    output.raw(&crate::value::V2_PREFIX);
    output.map(8);
    output.key("definitions");
    output.list(N);
    index = 0;
    while index < N {
        if index > 0 {
            assert!(
                crate::static_schema::compare(
                    contracts[index - 1].contract,
                    contracts[index].contract
                ) < 0,
                "duplicate contract identity"
            );
        }
        output.contract(contracts[index]);
        index += 1;
    }
    output.key("execution");
    output.text(match execution {
        MetadataExecution::None => "none",
        MetadataExecution::StatePending => "state-pending",
    });
    output.key("format");
    output.text("phenix.contract-metadata.v2");
    output.key("plugin");
    output.text(plugin);
    output.key("properties");
    output.list(P);
    index = 0;
    while index < P {
        let property = properties[index];
        assert!(identity(property.id), "invalid property identity");
        assert!(
            property.get || property.set || property.watch,
            "property needs an operation"
        );
        if index > 0 {
            assert!(
                crate::static_schema::compare(properties[index - 1].id, property.id) < 0,
                "duplicate property identity"
            );
        }
        output.map(5);
        output.key("get");
        output.boolean(property.get);
        output.key("id");
        output.text(property.id);
        output.key("schema");
        output.schema(property.schema);
        output.key("set");
        output.boolean(property.set);
        output.key("watch");
        output.boolean(property.watch);
        index += 1;
    }
    output.key("provides");
    output.list(E);
    index = 0;
    while index < E {
        let export = provides[index];
        assert!(
            identity(export.slot) && contract_id(export.contract),
            "invalid export identity"
        );
        assert!(export.revision > 0, "reserved export revision");
        if index > 0 {
            assert!(
                crate::static_schema::compare(provides[index - 1].slot, export.slot) < 0,
                "duplicate export slot"
            );
        }
        output.map(3);
        output.key("contract");
        output.text(export.contract);
        output.key("revision");
        output.number(export.revision);
        output.key("slot");
        output.text(export.slot);
        index += 1;
    }
    output.key("requires");
    output.list(I);
    index = 0;
    while index < I {
        let import = requires[index];
        assert!(
            identity(import.slot) && contract_id(import.contract),
            "invalid import identity"
        );
        assert!(
            import.revision_min > 0 && import.revision_max >= import.revision_min,
            "invalid import revisions"
        );
        if index > 0 {
            assert!(
                crate::static_schema::compare(requires[index - 1].slot, import.slot) < 0,
                "duplicate import slot"
            );
        }
        let (kind, minimum) = match import.cardinality {
            Cardinality::One => ("one", 1),
            Cardinality::Optional => ("optional", 0),
            Cardinality::Many { minimum } => ("many", minimum),
        };
        assert!(
            minimum <= crate::MAX_COUNT as u64,
            "import cardinality limit"
        );
        output.map(7);
        output.key("cardinality");
        output.text(kind);
        output.key("contract");
        output.text(import.contract);
        output.key("minimum");
        output.number(minimum);
        output.key("provider");
        match import.provider {
            Some(provider) => {
                assert!(
                    identity(provider) && crate::static_schema::compare(provider, "root") != 0,
                    "invalid import provider"
                );
                output.text(provider);
            }
            None => output.byte(0),
        }
        output.key("revision_max");
        output.number(import.revision_max);
        output.key("revision_min");
        output.number(import.revision_min);
        output.key("slot");
        output.text(import.slot);
        index += 1;
    }
    output.key("version");
    output.text(version);
    output
}

/**
 * Embed static graph declarations without initializing a native plugin.
 * This writer uses the same canonical Value envelope as GraphContribution.
 * Authored declaration order has no meaning. This const encoder selects
 * members in key order so the native section matches GraphContribution::encode.
 * Graph dependency edges are resolved only after all nodes are collected.
 */
/// Select the next canonical graph member without allocating during const
/// metadata emission. This deliberately does not require source-order sorting.
/// Missing a next distinct key means the declarations contain a duplicate.
const fn graph_member_len(graph: crate::StaticGraphContribution, field: u8) -> usize {
    match field {
        0 => graph.actions.len(),
        1 => graph.events.len(),
        _ => graph.nodes.len(),
    }
}

const fn graph_member_name(
    graph: crate::StaticGraphContribution,
    field: u8,
    index: usize,
) -> &'static str {
    match field {
        0 => graph.actions[index].id,
        1 => graph.events[index].id,
        _ => graph.nodes[index].id,
    }
}

const fn next_graph_member(
    graph: crate::StaticGraphContribution,
    field: u8,
    previous: Option<usize>,
) -> usize {
    let count = graph_member_len(graph, field);
    let mut chosen = count;
    let mut candidate = 0;
    while candidate < count {
        let key = graph_member_name(graph, field, candidate);
        let follows = match previous {
            Some(prev) => {
                crate::static_schema::compare(graph_member_name(graph, field, prev), key) < 0
            },
            None => true,
        };
        if follows
            && (chosen == count
                || crate::static_schema::compare(key, graph_member_name(graph, field, chosen)) < 0)
        {
            chosen = candidate;
        }
        candidate += 1;
    }
    assert!(chosen < count, "duplicate graph declaration identity");
    chosen
}

const fn next_graph_key(values: &[&str], previous: Option<usize>) -> usize {
    let mut chosen = values.len();
    let mut candidate = 0;
    while candidate < values.len() {
        let key = values[candidate];
        let follows = match previous {
            Some(prev) => crate::static_schema::compare(values[prev], key) < 0,
            None => true,
        };
        if follows
            && (chosen == values.len() || crate::static_schema::compare(key, values[chosen]) < 0)
        {
            chosen = candidate;
        }
        candidate += 1;
    }
    assert!(chosen < values.len(), "duplicate graph key");
    chosen
}

pub const fn graph(contribution: crate::StaticGraphContribution) -> MetadataBytes {
    assert!(identity(contribution.plugin), "invalid graph contributor");
    let mut output = MetadataBytes {
        data: [0; METADATA_BYTES],
        len: 0,
    };
    output.raw(&crate::value::V2_PREFIX);
    output.map(5);
    output.key("actions");
    output.list(contribution.actions.len());
    let mut i = 0;
    let mut previous = None;
    while i < contribution.actions.len() {
        let index = next_graph_member(contribution, 0, previous);
        let action = contribution.actions[index];
        previous = Some(index);
        assert!(
            identity(action.id) && identity(action.event),
            "invalid graph action identity"
        );
        assert!(
            !action.description.is_empty() && action.description.len() <= 4096,
            "invalid action description"
        );
        output.map(4);
        output.key("description");
        output.text(action.description);
        output.key("event");
        output.text(action.event);
        output.key("id");
        output.text(action.id);
        output.key("requires");
        output.list(action.requires.len());
        let mut j = 0;
        let mut previous_requirement = None;
        while j < action.requires.len() {
            let index = next_graph_key(action.requires, previous_requirement);
            let key = action.requires[index];
            assert!(
                operation_key_str(key),
                "invalid action authority requirement"
            );
            output.text(key);
            previous_requirement = Some(index);
            j += 1;
        }
        i += 1;
    }
    output.key("events");
    output.list(contribution.events.len());
    i = 0;
    previous = None;
    while i < contribution.events.len() {
        let index = next_graph_member(contribution, 1, previous);
        let event = contribution.events[index];
        previous = Some(index);
        assert!(identity(event.id), "invalid event identity");
        output.map(2);
        output.key("id");
        output.text(event.id);
        output.key("payload");
        output.schema(event.payload);
        i += 1;
    }
    output.key("format");
    output.text("phenix.graph.v1");
    output.key("nodes");
    output.list(contribution.nodes.len());
    i = 0;
    previous = None;
    while i < contribution.nodes.len() {
        let index = next_graph_member(contribution, 2, previous);
        let node = contribution.nodes[index];
        previous = Some(index);
        assert!(
            identity(node.id) && operation_key_str(node.operation),
            "invalid graph node identity or operation"
        );
        output.map(8);
        output.key("after");
        output_graph_keys(&mut output, node.after);
        output.key("failure");
        output.text(match node.failure {
            crate::FailurePolicy::Skip => "skip",
            crate::FailurePolicy::FailRoot => "fail-root",
        });
        output.key("id");
        output.text(node.id);
        output.key("join");
        output.text("all-active");
        output.key("operation");
        output.text(node.operation);
        output.key("selector");
        output.text(node.selector);
        output.key("triggers");
        output_graph_keys(&mut output, node.triggers);
        output.key("writes");
        output_graph_keys(&mut output, node.writes);
        i += 1;
    }
    output.key("plugin");
    output.text(contribution.plugin);
    output
}

const fn output_graph_keys(output: &mut MetadataBytes, values: &[&str]) {
    output.list(values.len());
    let mut i = 0;
    let mut previous = None;
    while i < values.len() {
        let index = next_graph_key(values, previous);
        output.text(values[index]);
        previous = Some(index);
        i += 1;
    }
}
const fn operation_key_str(key: &str) -> bool {
    let bytes = key.as_bytes();
    let mut slash = 0;
    while slash < bytes.len() && bytes[slash] != b'/' {
        slash += 1;
    }
    slash > 0 && slash + 1 < bytes.len()
}
