//! Portable, domain-independent plugin contribution declarations.
//!
//! This is inert metadata. It does not register providers, select plugins,
//! dispatch services, or alter an active graph. Only a resolved generation may
//! interpret contributions after schema, compatibility and authority checks.

use crate::{ContractId, PhenixValue, PluginId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

/// The relationship between a plugin and a versioned contribution kind.
///
/// A kind is not a Core enum variant. New kinds are identified by their
/// provider-neutral `ContractId` and interpreted by selected kind providers.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionRole {
    Declare,
    Require,
    Provide,
    Modify,
    Observe,
}

/// One immutable, versioned declaration from a named plugin.
///
/// `id` is a stable versioned identity, independent of its Rust item name.
/// `kind` names a versioned schema, not a particular provider implementation.
/// `payload` is an inert structural value; Core does not assume its domain.
/// Kind-specific validation and lowering are separate preparation operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub owner: PluginId,
    pub id: ContractId,
    pub kind: ContractId,
    pub role: ContributionRole,
    pub payload: PhenixValue,
}

/// Conflicts detectable before any contribution-kind or provider resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContributionSetError {
    DuplicateIdentity {
        id: ContractId,
        first_owner: PluginId,
        second_owner: PluginId,
    },
    OwnerMismatch {
        id: ContractId,
        expected: PluginId,
        actual: PluginId,
    },
    DuplicateSelectedOwner {
        owner: PluginId,
    },
    Serialization(String),
}

impl Display for ContributionSetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateIdentity {
                id,
                first_owner,
                second_owner,
            } => write!(
                f,
                "contribution {id} appears more than once (owners {first_owner} and {second_owner})"
            ),
            Self::OwnerMismatch {
                id,
                expected,
                actual,
            } => write!(
                f,
                "contribution {id} claims owner {actual}, but it was supplied by {expected}"
            ),
            Self::DuplicateSelectedOwner { owner } => {
                write!(
                    f,
                    "selected plugin {owner} supplied multiple contribution envelopes"
                )
            }
            Self::Serialization(error) => {
                write!(f, "canonical contribution encoding failed: {error}")
            }
        }
    }
}

impl Error for ContributionSetError {}

/// Stable input to generation resolution regardless of discovery order.
///
/// This deliberately does not merge competing declarations or pick providers.
/// The semantic composer is responsible for kind/schema checks and conflicts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContributionSet(BTreeMap<ContractId, Contribution>);

fn sort_contributions(contributions: &mut [Contribution]) {
    contributions.sort_by(|a, b| {
        (&a.id, &a.owner, &a.kind, a.role).cmp(&(&b.id, &b.owner, &b.kind, b.role))
    });
}

impl Serialize for ContributionSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // A sorted list keeps duplicate detection possible during decoding.
        self.0.values().collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContributionSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let declarations = Vec::<Contribution>::deserialize(deserializer)?;
        Self::collect(declarations).map_err(D::Error::custom)
    }
}

impl ContributionSet {
    pub fn collect(
        contributions: impl IntoIterator<Item = Contribution>,
    ) -> Result<Self, ContributionSetError> {
        let mut ordered: Vec<_> = contributions.into_iter().collect();
        // Duplicate diagnostics must not change with plugin enumeration order.
        sort_contributions(&mut ordered);
        let mut declarations = BTreeMap::new();
        for contribution in ordered {
            let id = contribution.id.clone();
            if let Some(previous) = declarations.insert(id.clone(), contribution.clone()) {
                return Err(ContributionSetError::DuplicateIdentity {
                    id,
                    first_owner: previous.owner,
                    second_owner: contribution.owner,
                });
            }
        }
        Ok(Self(declarations))
    }

    /// Validate ownership against the verified plugin that supplied these bytes.
    ///
    /// Call this before accepting a plugin artifact's declarations; the
    /// serialized `owner` field is provenance data, not a security principal.
    pub fn collect_owned(
        owner: &PluginId,
        contributions: impl IntoIterator<Item = Contribution>,
    ) -> Result<Self, ContributionSetError> {
        let mut owned: Vec<_> = contributions.into_iter().collect();
        // Invalid owner diagnostics must also be independent of input order.
        sort_contributions(&mut owned);
        for contribution in &owned {
            if &contribution.owner != owner {
                return Err(ContributionSetError::OwnerMismatch {
                    id: contribution.id.clone(),
                    expected: owner.clone(),
                    actual: contribution.owner.clone(),
                });
            }
        }
        Self::collect(owned)
    }

    /// Decode portable metadata against the manifest-verified plugin identity.
    ///
    /// The serialized owner is untrusted provenance. Artifact loaders must
    /// supply the independently verified owner and reject forged declarations
    /// before interpreting kinds or preparing a candidate generation.
    pub fn decode_owned(owner: &PluginId, bytes: &[u8]) -> Result<Self, ContributionSetError> {
        let declarations: Vec<Contribution> = serde_json::from_slice(bytes)
            .map_err(|error| ContributionSetError::Serialization(error.to_string()))?;
        Self::collect_owned(owner, declarations)
    }

    /// Normalize the contribution metadata of a selected set of portable
    /// plugin artifacts in one boundary operation.
    ///
    /// The caller must authenticate each artifact and supply its independently
    /// verified manifest owner. The encoded `owner` field is never trusted.
    /// An artifact may supply one envelope; duplicates, forged owners and
    /// conflicting contribution identities reject the candidate.
    ///
    /// This performs no provider selection, kind-specific interpretation,
    /// authority grant or runtime activation.
    pub fn decode_selected<'a>(
        selected: impl IntoIterator<Item = (&'a PluginId, &'a [u8])>,
    ) -> Result<Self, ContributionSetError> {
        // Canonicalize manifest-verified owners before decoding any untrusted
        // artifact bytes. Otherwise, malformed or forged artifacts can change
        // the first reported error simply by changing discovery order.
        let mut selected: Vec<_> = selected.into_iter().collect();
        selected.sort_by_key(|(owner, _)| *owner);
        for pair in selected.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(ContributionSetError::DuplicateSelectedOwner {
                    owner: (*pair[0].0).clone(),
                });
            }
        }

        let mut declarations = Vec::new();
        for (owner, bytes) in selected {
            let batch = Self::decode_owned(owner, bytes)?;
            declarations.extend(batch.iter().cloned());
        }
        Self::collect(declarations)
    }

    pub fn get(&self, id: &ContractId) -> Option<&Contribution> {
        self.0.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Contribution> {
        self.0.values()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Canonical JSON bytes use stable ID ordering and the canonical
    /// `PhenixValue` representation. No hash algorithm is prescribed here.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContributionSetError> {
        serde_json::to_vec(self)
            .map_err(|error| ContributionSetError::Serialization(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, ValueCodec};
    use std::collections::BTreeMap;

    fn contribution(id: &str) -> Contribution {
        Contribution {
            owner: PluginId::parse("acme.fixture").unwrap(),
            id: ContractId::parse(id).unwrap(),
            kind: ContractId::parse("acme.fixture.kind@1").unwrap(),
            role: ContributionRole::Modify,
            payload: PhenixValue::Table(BTreeMap::from([
                (
                    Key::parse("route").unwrap(),
                    PhenixValue::String("execute".into()),
                ),
                (Key::parse("enabled").unwrap(), true.to_value()),
            ])),
        }
    }

    #[test]
    fn canonical_set_is_independent_of_plugin_enumeration() {
        let first = contribution("acme.fixture.first@1");
        let second = contribution("acme.fixture.second@1");

        let forward = ContributionSet::collect([first.clone(), second.clone()]).unwrap();
        let reversed = ContributionSet::collect([second, first]).unwrap();
        assert_eq!(forward, reversed);
        assert_eq!(
            forward.canonical_bytes().unwrap(),
            reversed.canonical_bytes().unwrap()
        );

        let decoded: ContributionSet =
            serde_json::from_slice(&forward.canonical_bytes().unwrap()).unwrap();
        assert_eq!(decoded, forward);
    }

    #[test]
    fn duplicate_identity_is_rejected_even_when_payload_matches() {
        let contribution = contribution("acme.fixture.duplicate@1");
        assert!(matches!(
            ContributionSet::collect([contribution.clone(), contribution]),
            Err(ContributionSetError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn duplicate_conflicts_have_stable_provenance_across_enumeration() {
        let mut alpha = contribution("acme.fixture.duplicate@1");
        alpha.owner = PluginId::parse("acme.alpha").unwrap();
        let mut beta = alpha.clone();
        beta.owner = PluginId::parse("acme.beta").unwrap();
        let mut gamma = alpha.clone();
        gamma.owner = PluginId::parse("acme.gamma").unwrap();
        let a = ContributionSet::collect([gamma.clone(), beta.clone(), alpha.clone()]);
        let b = ContributionSet::collect([alpha, beta, gamma]);
        assert_eq!(a, b);
        assert!(matches!(
            a,
            Err(ContributionSetError::DuplicateIdentity { first_owner, second_owner, .. })
                if first_owner.as_str() == "acme.alpha" && second_owner.as_str() == "acme.beta"
        ));
    }

    #[test]
    fn untrusted_owner_claims_are_rejected() {
        let wrong_owner = PluginId::parse("other.plugin").unwrap();
        let item = contribution("acme.fixture.owned@1");
        assert!(matches!(
            ContributionSet::collect_owned(&wrong_owner, [item]),
            Err(ContributionSetError::OwnerMismatch { .. })
        ));
    }

    #[test]
    fn forged_owner_rejection_is_stable_across_enumeration() {
        let expected = PluginId::parse("acme.fixture").unwrap();
        let mut first = contribution("acme.fixture.first@1");
        first.owner = PluginId::parse("acme.rogue-first").unwrap();
        let mut second = contribution("acme.fixture.second@1");
        second.owner = PluginId::parse("acme.rogue-second").unwrap();
        let forward = ContributionSet::collect_owned(&expected, [second.clone(), first.clone()]);
        let reverse = ContributionSet::collect_owned(&expected, [first, second]);
        assert_eq!(forward, reverse);
        assert!(matches!(
            forward,
            Err(ContributionSetError::OwnerMismatch { id, .. })
                if id.as_str() == "acme.fixture.first@1"
        ));
    }

    #[test]
    fn portable_artifact_bytes_are_bound_to_verified_plugin_owner() {
        // These bytes are independently authorable without a Rust plugin SDK.
        let bytes = br#"[{"owner":"acme.fixture","id":"acme.fixture.external@1","kind":"acme.fixture.kind@1","role":"provide","payload":{"type":"string","value":"external"}}]"#;
        let owner = PluginId::parse("acme.fixture").unwrap();
        let decoded = ContributionSet::decode_owned(&owner, bytes).unwrap();
        assert_eq!(decoded.len(), 1);
        let item = decoded.iter().next().unwrap();
        assert_eq!(item.owner, owner);
        assert_eq!(item.payload, PhenixValue::String("external".into()));

        let unrelated = PluginId::parse("acme.unrelated").unwrap();
        assert!(matches!(
            ContributionSet::decode_owned(&unrelated, bytes),
            Err(ContributionSetError::OwnerMismatch { .. })
        ));
    }

    #[test]
    fn portable_artifact_decode_rejects_duplicate_declarations() {
        let item = contribution("acme.fixture.duplicate@1");
        let bytes = serde_json::to_vec(&vec![item.clone(), item]).unwrap();
        assert!(matches!(
            ContributionSet::decode_owned(&PluginId::parse("acme.fixture").unwrap(), &bytes,),
            Err(ContributionSetError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn selected_portable_artifacts_compose_in_canonical_order() {
        let alpha = PluginId::parse("acme.alpha").unwrap();
        let beta = PluginId::parse("acme.beta").unwrap();
        let mut first = contribution("acme.alpha.fixture@1");
        first.owner = alpha.clone();
        let mut second = contribution("acme.beta.fixture@1");
        second.owner = beta.clone();
        let alpha_bytes = serde_json::to_vec(&vec![first]).unwrap();
        let beta_bytes = serde_json::to_vec(&vec![second]).unwrap();

        let forward = ContributionSet::decode_selected([
            (&alpha, alpha_bytes.as_slice()),
            (&beta, beta_bytes.as_slice()),
        ])
        .unwrap();
        let reversed = ContributionSet::decode_selected([
            (&beta, beta_bytes.as_slice()),
            (&alpha, alpha_bytes.as_slice()),
        ])
        .unwrap();
        assert_eq!(forward.len(), 2);
        assert_eq!(
            forward.canonical_bytes().unwrap(),
            reversed.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn selected_portable_artifacts_reject_duplicate_owners_and_cross_artifact_ids() {
        let alpha = PluginId::parse("acme.alpha").unwrap();
        let beta = PluginId::parse("acme.beta").unwrap();
        let mut first = contribution("acme.fixture.same@1");
        first.owner = alpha.clone();
        let mut second = first.clone();
        second.owner = beta.clone();
        let alpha_bytes = serde_json::to_vec(&vec![first]).unwrap();
        let beta_bytes = serde_json::to_vec(&vec![second]).unwrap();

        assert!(matches!(
            ContributionSet::decode_selected([
                (&alpha, alpha_bytes.as_slice()),
                (&alpha, alpha_bytes.as_slice()),
            ]),
            Err(ContributionSetError::DuplicateSelectedOwner { owner }) if owner == alpha
        ));
        assert!(matches!(
            ContributionSet::decode_selected([
                (&alpha, alpha_bytes.as_slice()),
                (&beta, beta_bytes.as_slice()),
            ]),
            Err(ContributionSetError::DuplicateIdentity { .. })
        ));
        assert!(matches!(
            ContributionSet::decode_selected([(&alpha, beta_bytes.as_slice())]),
            Err(ContributionSetError::OwnerMismatch { .. })
        ));
    }

    #[test]
    fn selected_portable_failures_have_stable_provenance_across_artifact_order() {
        let alpha = PluginId::parse("acme.alpha").unwrap();
        let beta = PluginId::parse("acme.beta").unwrap();
        let mut alpha_claim = contribution("acme.alpha.invalid@1");
        alpha_claim.owner = beta.clone();
        let mut beta_claim = contribution("acme.beta.invalid@1");
        beta_claim.owner = alpha.clone();
        let alpha_bytes = serde_json::to_vec(&vec![alpha_claim]).unwrap();
        let beta_bytes = serde_json::to_vec(&vec![beta_claim]).unwrap();

        let forward = ContributionSet::decode_selected([
            (&beta, beta_bytes.as_slice()),
            (&alpha, alpha_bytes.as_slice()),
        ]);
        let reverse = ContributionSet::decode_selected([
            (&alpha, alpha_bytes.as_slice()),
            (&beta, beta_bytes.as_slice()),
        ]);
        assert_eq!(forward, reverse);
        assert!(matches!(
            forward,
            Err(ContributionSetError::OwnerMismatch { expected, .. })
                if expected == alpha
        ));
    }

    #[test]
    fn duplicate_selected_owners_reject_before_decoding_malformed_artifacts() {
        let alpha = PluginId::parse("acme.alpha").unwrap();
        let beta = PluginId::parse("acme.beta").unwrap();
        let invalid_json = b"not json";
        let empty_set = b"[]";
        let forward = ContributionSet::decode_selected([
            (&beta, &empty_set[..]),
            (&alpha, &invalid_json[..]),
            (&alpha, &empty_set[..]),
        ]);
        let reverse = ContributionSet::decode_selected([
            (&alpha, &empty_set[..]),
            (&alpha, &invalid_json[..]),
            (&beta, &empty_set[..]),
        ]);
        assert_eq!(forward, reverse);
        assert!(matches!(
            forward,
            Err(ContributionSetError::DuplicateSelectedOwner { owner }) if owner == alpha
        ));
    }

    #[test]
    fn duplicate_id_in_serialized_input_is_rejected() {
        let value = contribution("acme.fixture.duplicate@1");
        let duplicated = serde_json::to_value(vec![value.clone(), value]).unwrap();
        assert!(serde_json::from_value::<ContributionSet>(duplicated).is_err());
    }

    #[test]
    fn versioned_kind_and_identity_are_validated_on_decode() {
        let mut value = serde_json::to_value(contribution("acme.fixture.valid@1")).unwrap();
        value["kind"] = serde_json::json!("acme.fixture.unversioned");
        assert!(serde_json::from_value::<Contribution>(value).is_err());
    }

    #[test]
    fn roles_and_payload_are_portable_and_domain_independent() {
        let original = contribution("acme.fixture.portable@1");
        let json = serde_json::to_vec(&original).unwrap();
        let decoded: Contribution = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(decoded.role, ContributionRole::Modify);
    }
}
