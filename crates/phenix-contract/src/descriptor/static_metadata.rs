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
