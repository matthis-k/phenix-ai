use crate::PermissionId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Authority {
    permissions: BTreeSet<PermissionId>,
}

impl Authority {
    pub fn new(permissions: impl IntoIterator<Item = PermissionId>) -> Self {
        Self {
            permissions: permissions.into_iter().collect(),
        }
    }

    pub fn permits(&self, permission: &PermissionId) -> bool {
        self.permissions.contains(permission)
    }

    pub fn permits_all(&self, required: &Self) -> bool {
        required
            .permissions
            .iter()
            .all(|permission| self.permits(permission))
    }

    pub fn attenuate(&self, requested: &Self) -> Self {
        Self {
            permissions: self
                .permissions
                .intersection(&requested.permissions)
                .cloned()
                .collect(),
        }
    }

    pub fn permissions(&self) -> impl Iterator<Item = &PermissionId> {
        self.permissions.iter()
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
