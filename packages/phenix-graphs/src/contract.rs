//! Canonical contracts. Inspection data is deliberately not an attestation API.
use crate::component::ComponentDef;
use crate::interface::{Interface, InterfaceError};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contract {
    id: String,
    required: Arc<Interface>,
}
impl Contract {
    pub fn new(id: impl Into<String>, required: Arc<Interface>) -> Result<Self, InterfaceError> {
        let id = id.into();
        if id.len() > 512 || !id.split('/').all(valid_segment) {
            return Err(InterfaceError::new("invalid contract identity"));
        }
        Ok(Self { id, required })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn required(&self) -> &Arc<Interface> {
        &self.required
    }
    pub fn check_definition(&self, provided: &Self) -> Result<(), InterfaceError> {
        if self == provided {
            Ok(())
        } else {
            Err(InterfaceError::new(
                "different contract identity or definition",
            ))
        }
    }
    /// Requires the exact complete interface, independently of semantic claims.
    pub fn check_shape(&self, actual: &Interface) -> Result<(), InterfaceError> {
        self.required.check_shape(actual).map_err(|mut error| {
            error.contract = Some(self.id().into());
            error
        })
    }
    /// Checks the immutable declaration, not a caller-supplied snapshot. For an
    /// instantiated scope use CompiledNetwork/NetworkBuilder::check_implementation.
    pub fn check_implementation(&self, component: &ComponentDef) -> Result<(), InterfaceError> {
        self.check(component.interface(), component.claims())
    }
    pub(crate) fn check(
        &self,
        interface: &Interface,
        claims: &[Contract],
    ) -> Result<(), InterfaceError> {
        let mut found = false;
        for claim in claims.iter().filter(|claim| claim.id() == self.id()) {
            self.check_definition(claim)?;
            found = true;
        }
        if !found {
            return Err(InterfaceError::new("missing explicit contract claim"));
        }
        self.check_shape(interface)
    }
}

pub(crate) fn catalog<'a>(
    contracts: impl IntoIterator<Item = &'a Contract>,
) -> Result<(), InterfaceError> {
    let mut seen = BTreeMap::<&str, &Contract>::new();
    for contract in contracts {
        if let Some(old) = seen.insert(contract.id(), contract) {
            old.check_definition(contract)?;
        }
    }
    Ok(())
}

fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'@'))
}
