use crate::PermissionId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Authority {
    capabilities: BTreeSet<PermissionId>,
}

impl Authority {
    pub fn new(capabilities: impl IntoIterator<Item = PermissionId>) -> Self {
        Self {
            capabilities: capabilities.into_iter().collect(),
        }
    }

    pub fn permits(&self, capability: &PermissionId) -> bool {
        self.capabilities.contains(capability)
    }

    pub fn permits_all(&self, required: &Self) -> bool {
        required
            .capabilities
            .iter()
            .all(|capability| self.permits(capability))
    }

    pub fn attenuate(&self, requested: &Self) -> Self {
        Self {
            capabilities: self
                .capabilities
                .intersection(&requested.capabilities)
                .cloned()
                .collect(),
        }
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &PermissionId> {
        self.capabilities.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(value: &str) -> PermissionId {
        PermissionId::parse(value).unwrap()
    }

    #[test]
    fn attenuation_cannot_regain_authority() {
        let parent = Authority::new([cap("fs.read"), cap("network.read")]);
        let requested = Authority::new([cap("fs.read"), cap("fs.write")]);
        let child = parent.attenuate(&requested);

        assert!(child.permits(&cap("fs.read")));
        assert!(!child.permits(&cap("fs.write")));
        assert!(!child.permits(&cap("network.read")));
    }

    #[test]
    fn serialized_authority_is_deterministic() {
        let authority = Authority::new([cap("network.read"), cap("fs.read")]);
        assert_eq!(
            serde_json::to_string(&authority).unwrap(),
            "[\"fs.read\",\"network.read\"]"
        );
    }
}
