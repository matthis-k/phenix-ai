//! One reusable canonical definition, with no body-dependent boundary authority.
use crate::GraphOrigin;
use crate::contract::Contract;
use crate::interface::Interface;
use crate::interface::InterfaceError;
use crate::network::NetworkDef;
use crate::process::ProcessFactory;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "adapters.rs"]
mod adapters;

#[derive(Clone)]
pub enum Body {
    Primitive(Arc<dyn ProcessFactory>),
    Composite(Arc<NetworkDef>),
}
impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primitive(_) => f.write_str("Primitive(..)"),
            Self::Composite(network) => f.debug_tuple("Composite").field(network).finish(),
        }
    }
}

/// Opaque declaration identity, preserved by cloning, unrelated to allocation or
/// factory identity. New definitions always receive a fresh key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionKey(u64);
impl DefinitionKey {
    fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("definition identity exhausted"),
        )
    }
}

/// Immutable reusable declaration. Claims are explicit semantic promises.
#[derive(Clone, Debug)]
pub struct ComponentDef {
    pub(crate) interface: Arc<Interface>,
    pub(crate) body: Body,
    pub(crate) claims: Vec<Contract>,
    pub(crate) origin: Option<GraphOrigin>,
    key: DefinitionKey,
}
impl ComponentDef {
    pub fn primitive(interface: Arc<Interface>, factory: Arc<dyn ProcessFactory>) -> Self {
        Self {
            interface,
            body: Body::Primitive(factory),
            claims: Vec::new(),
            origin: None,
            key: DefinitionKey::fresh(),
        }
    }
    pub fn composite(network: NetworkDef) -> Self {
        Self {
            interface: network.interface().clone(),
            claims: network.claims.clone(),
            origin: network.origin.clone(),
            body: Body::Composite(Arc::new(network)),
            key: DefinitionKey::fresh(),
        }
    }
    /// Provenance supplied by the trusted host, not a grant or semantic claim.
    pub fn with_origin(mut self, origin: GraphOrigin) -> Self {
        self.origin = Some(origin);
        self
    }
    pub fn interface(&self) -> &Arc<Interface> {
        &self.interface
    }
    pub fn body(&self) -> &Body {
        &self.body
    }
    pub fn claims(&self) -> &[Contract] {
        &self.claims
    }
    pub fn origin(&self) -> Option<&GraphOrigin> {
        self.origin.as_ref()
    }
    pub fn definition_key(&self) -> DefinitionKey {
        self.key
    }
    pub fn claim(mut self, contract: &Contract) -> Result<Self, InterfaceError> {
        contract.check_shape(&self.interface)?;
        crate::contract::catalog(self.claims.iter().chain(std::iter::once(contract)))?;
        if !self.claims.contains(contract) {
            self.claims.push(contract.clone());
        }
        Ok(self)
    }
}
