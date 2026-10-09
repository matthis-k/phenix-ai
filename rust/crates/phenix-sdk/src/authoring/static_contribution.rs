//! Static authoring of inert, portable plugin contributions.
//!
//! Types defined by plugins may implement `StaticContributionDefinition`.
//! The authoring macro only lowers annotated declarations; it never applies
//! them to a graph or invokes a domain registry.

use phenix_contract::{ContractId, Contribution, ContributionRole, PhenixValue, PluginId};

/// A plugin-defined typed declaration lowered into the canonical metadata.
///
/// This is called while gathering the candidate's static metadata, not during
/// a plugin's `start` or `invoke` lifecycle.
pub trait StaticContributionDefinition {
    fn contribution(&self, owner: &PluginId) -> Result<Contribution, String>;
}

/// Portable fallback for authors without a custom contribution type.
///
/// `payload_json` contains the canonical serialized `PhenixValue` shape.
/// Kind-specific Rust structs can implement `StaticContributionDefinition`
/// directly and avoid hand-writing JSON. They still produce the same metadata.
pub struct StaticRawContribution {
    pub id: &'static str,
    pub kind: &'static str,
    pub role: ContributionRole,
    pub payload_json: &'static str,
}

impl StaticContributionDefinition for StaticRawContribution {
    fn contribution(&self, owner: &PluginId) -> Result<Contribution, String> {
        let id = ContractId::parse(self.id).map_err(str::to_owned)?;
        let kind = ContractId::parse(self.kind).map_err(str::to_owned)?;
        let payload: PhenixValue =
            serde_json::from_str(self.payload_json).map_err(|error| error.to_string())?;
        Ok(Contribution {
            owner: owner.clone(),
            id,
            kind,
            role: self.role,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_declaration_rejects_an_unversioned_kind() {
        let raw = StaticRawContribution {
            id: "fixture.record@1",
            kind: "fixture.kind",
            role: ContributionRole::Declare,
            payload_json: r#"{"type":"string","value":"hello"}"#,
        };
        assert!(
            raw.contribution(&PluginId::parse("fixture").unwrap())
                .is_err()
        );
    }
}
