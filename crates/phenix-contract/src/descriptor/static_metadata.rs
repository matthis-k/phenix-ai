//! Constant declarations projected into the accepted owned metadata protocol.
use super::{ContractDefinition, OperationSchema};
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

/// Canonical metadata bytes for an offline definition package.
pub struct MetadataBytes {
    data: [u8; METADATA_BYTES],
    len: usize,
}

impl MetadataBytes {
    pub fn bytes(&self) -> &[u8] {
        &self.data[..self.len]
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

pub const fn definitions<const N: usize>(
    plugin: &str,
    version: &str,
    mut contracts: [StaticContract; N],
) -> MetadataBytes {
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
    output.text("none");
    output.key("format");
    output.text("phenix.contract-metadata.v2");
    output.key("plugin");
    output.text(plugin);
    output.key("properties");
    output.list(0);
    output.key("provides");
    output.list(0);
    output.key("requires");
    output.list(0);
    output.key("version");
    output.text(version);
    output
}
