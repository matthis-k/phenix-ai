//! Trusted host-side provider composition, not a provider implementation.
//!
//! The provider implementation contributes ProviderCatalog/ProviderAuth and
//! describes endpoint, inference protocol and authentication independently.
//! This builder connects already installed concrete plugins to the existing
//! M1 contracts. No provider ID, auth kind, or protocol is special-cased.
//! Manifest paths are launcher-owned; they never arrive over ACP or from the
//! user/model session. The kernel still verifies all artifacts, definitions,
//! slots and grants before it loads any native code.

use phenix_contract::Authority;
use phenix_kernel::resolve::{Composition, ScopedGrant, Selection, SlotBinding};
use phenix_provider_contracts::CredentialIdentity;
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

const CATALOG: &str = "phenix.provider.catalog@1";
const AUTH: &str = "phenix.provider.auth@1";
const CREDENTIALS: &str = "phenix.provider.credentials@1";
const CATALOG_OPS: &[&str] = &["list", "describe", "models"];
const AUTH_OPS: &[&str] = &["status", "begin", "poll", "cancel"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Implementation {
    id: String,
    artifact: String,
}

/// An installed provider chooses implementations, not a special runtime branch.
/// Its endpoint/protocol/auth declaration remains owned by ProviderCatalog.
/// This is only the host-side wiring and capability policy for those choices.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDeployment {
    provider: Implementation,
    credentials: Implementation,
    definitions: Vec<String>,
    account: CredentialIdentity,
    /// Capabilities the concrete auth provider needs from the credential store.
    /// Unlisted methods are denied by the kernel, even if implemented.
    #[serde(default)]
    provider_credential_operations: Vec<CredentialOperation>,
    /// Explicitly authorized ACP management operations. An OAuth-only
    /// provider can leave this empty; a new typed operation requires no
    /// hard-coded changes to the controller or composition builder.
    #[serde(default)]
    credential_management_operations: Vec<CredentialOperation>,
    /// Expose the selected exact-account credential to the native model
    /// installer. OAuth adapters must supply a validated access token, not
    /// hand a persisted OAuth record directly to model inference.
    #[serde(default)]
    model_resolution: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CredentialOperation {
    Import,
    BindEnvironment,
    Resolve,
    Revoke,
}

impl CredentialOperation {
    fn name(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::BindEnvironment => "bind_environment",
            Self::Resolve => "resolve",
            Self::Revoke => "revoke",
        }
    }
}

pub struct ComposedProvider {
    pub management: Composition,
    pub credentials: Option<Composition>,
    pub resolver: Option<Composition>,
}

fn grant(caller: &str, interface: &str, operation: &str, resource: &str) -> ScopedGrant {
    ScopedGrant {
        caller: caller.into(),
        interface: interface.into(),
        operation: operation.into(),
        resources: BTreeSet::from([resource.to_owned()]),
    }
}

fn root(
    provider: &str,
    artifacts: Vec<String>,
    definitions: Vec<String>,
    contract: &str,
    operation: &str,
    slots: Vec<SlotBinding>,
    grants: Vec<ScopedGrant>,
    resource: &str,
) -> Composition {
    let operations = grants.iter().map(ScopedGrant::key).collect();
    Composition {
        format: "phenix.composition.v3".into(),
        entry: Selection {
            interface: contract.into(),
            operation: operation.into(),
            provider: provider.into(),
        },
        artifacts,
        definitions,
        slots,
        grants,
        authority: Authority {
            operations,
            resources: BTreeSet::from([resource.to_owned()]),
        },
        resource: Some(resource.to_owned()),
        excluded: BTreeSet::new(),
        substitutions: vec![],
    }
}

impl ProviderDeployment {
    pub fn compose(self) -> Result<ComposedProvider, String> {
        let ProviderDeployment {
            provider,
            credentials,
            definitions,
            account,
            provider_credential_operations,
            credential_management_operations,
            model_resolution,
        } = self;
        // The host, not an ACP request, selects trusted installation paths.
        // The kernel independently verifies the package metadata and signatures.
        if provider.id.is_empty()
            || credentials.id.is_empty()
            || provider.id == credentials.id
            || definitions.is_empty()
            || definitions
                .iter()
                .any(|path| !Path::new(path).is_absolute())
            || !Path::new(&provider.artifact).is_absolute()
            || !Path::new(&credentials.artifact).is_absolute()
        {
            return Err("provider deployment requires distinct installed implementations and absolute paths".into());
        }
        let resource = account
            .resource()
            .map_err(|_| "invalid provider deployment account")?;
        let allowed = provider_credential_operations
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if allowed.len() != provider_credential_operations.len() {
            return Err("duplicate provider credential operation".into());
        }
        let mut grants = vec![];
        for operation in CATALOG_OPS {
            grants.push(grant("root", CATALOG, operation, &resource));
        }
        for operation in AUTH_OPS {
            grants.push(grant("root", AUTH, operation, &resource));
        }
        for operation in allowed {
            grants.push(grant(
                &provider.id,
                CREDENTIALS,
                operation.name(),
                &resource,
            ));
        }
        let managed = credential_management_operations
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if managed.len() != credential_management_operations.len()
            || (!managed.is_empty()
                && (!managed.contains(&CredentialOperation::Import)
                    || !managed.contains(&CredentialOperation::Revoke)
                    || managed.contains(&CredentialOperation::Resolve)))
        {
            return Err("invalid credential management operation selection".into());
        }
        let management = root(
            &provider.id,
            vec![provider.artifact, credentials.artifact.clone()],
            definitions.clone(),
            CATALOG,
            "list",
            vec![SlotBinding {
                caller: provider.id.clone(),
                slot: "Credentials".into(),
                index: 0,
                provider: credentials.id.clone(),
                export: "Credentials".into(),
            }],
            grants,
            &resource,
        );
        let credentials_root = (!managed.is_empty()).then(|| {
            root(
                &credentials.id,
                vec![credentials.artifact.clone()],
                definitions.clone(),
                CREDENTIALS,
                "import",
                vec![],
                managed
                    .iter()
                    .map(|operation| grant("root", CREDENTIALS, operation.name(), &resource))
                    .collect(),
                &resource,
            )
        });
        let resolver = model_resolution.then(|| {
            root(
                &credentials.id,
                vec![credentials.artifact],
                definitions,
                CREDENTIALS,
                "resolve",
                vec![],
                vec![grant("root", CREDENTIALS, "resolve", &resource)],
                &resource,
            )
        });
        Ok(ComposedProvider {
            management,
            credentials: credentials_root,
            resolver,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ProviderDeployment {
        serde_json::from_value(serde_json::json!({
            "provider": {
                "id": "phenix.provider.example",
                "artifact": "/nix/store/provider"
            },
            "credentials": {
                "id": "phenix.credentials.keyring",
                "artifact": "/nix/store/keyring"
            },
            "definitions": ["/nix/store/provider-contracts"],
            "account": {
                "provider_id": "example",
                "connection_id": "api",
                "profile_id": "work"
            },
            "provider_credential_operations": ["resolve"],
            "credential_management_operations": ["import", "revoke"],
            "model_resolution": true
        }))
        .unwrap()
    }

    #[test]
    fn three_implementations_compose_without_provider_specific_runtime_code() {
        let expected = CredentialIdentity::new("example", "api", "work")
            .unwrap()
            .resource()
            .unwrap();
        let installed = fixture().compose().unwrap();
        let management = &installed.management;
        assert_eq!(management.entry.provider, "phenix.provider.example");
        assert_eq!(management.slots[0].slot, "Credentials");
        assert_eq!(management.slots[0].provider, "phenix.credentials.keyring");
        assert_eq!(management.resource.as_deref(), Some(expected.as_str()));
        assert_eq!(
            management.authority.resources,
            BTreeSet::from([expected.clone()])
        );
        assert_eq!(management.authority.operations.len(), 8);
        assert!(management
            .grants
            .iter()
            .any(|g| g.caller == "phenix.provider.example" && g.operation == "resolve"));
        assert!(!management
            .grants
            .iter()
            .any(|g| g.caller == "phenix.provider.example" && g.operation == "import"));
        let control = installed.credentials.unwrap();
        assert_eq!(control.authority.operations.len(), 2);
        assert!(control
            .authority
            .operations
            .contains("phenix.provider.credentials@1/import"));
        assert_eq!(control.artifacts, vec!["/nix/store/keyring"]);
        let resolver = installed.resolver.unwrap();
        assert_eq!(resolver.authority.operations.len(), 1);
        assert!(resolver
            .authority
            .operations
            .contains("phenix.provider.credentials@1/resolve"));
        assert_eq!(resolver.resource.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn oauth_provider_has_private_store_without_api_key_import_or_model_fallback() {
        let mut manifest = fixture();
        manifest.credential_management_operations.clear();
        manifest.model_resolution = false;
        manifest.provider_credential_operations =
            vec![CredentialOperation::Resolve, CredentialOperation::Import];
        let composed = manifest.compose().unwrap();
        assert!(composed.credentials.is_none());
        assert!(composed.resolver.is_none());
        assert!(composed
            .management
            .grants
            .iter()
            .any(|g| g.caller == "phenix.provider.example" && g.operation == "import"));
        assert!(!composed
            .management
            .grants
            .iter()
            .any(|g| g.caller == "root" && g.interface == CREDENTIALS));
    }

    #[test]
    fn invalid_or_ambiguous_deployments_are_rejected() {
        let mut duplicate = fixture();
        duplicate
            .provider_credential_operations
            .push(CredentialOperation::Resolve);
        assert!(duplicate.compose().is_err());
        let mut same = fixture();
        same.credentials.id = same.provider.id.clone();
        assert!(same.compose().is_err());
        let mut relative = fixture();
        relative.provider.artifact = "relative/plugin.so".into();
        assert!(relative.compose().is_err());
        let invalid = serde_json::json!({
            "provider": {"id":"p","artifact":"/p"},
            "credentials": {"id":"c","artifact":"/c"},
            "definitions": ["/defs"],
            "account": {
                "provider_id":"example",
                "connection_id":"api",
                "profile_id":"../other"
            }
        });
        assert!(serde_json::from_value::<ProviderDeployment>(invalid).is_err());
    }

    #[test]
    fn new_credential_methods_need_no_runtime_grant_count_change() {
        let mut deployment = fixture();
        deployment
            .credential_management_operations
            .push(CredentialOperation::BindEnvironment);
        let composed = deployment.compose().unwrap();
        let control = composed.credentials.unwrap();
        assert_eq!(control.authority.operations.len(), 3);
        assert!(control.authority.operations.contains(
            "phenix.provider.credentials@1/bind_environment"
        ));
        let mut invalid = fixture();
        invalid.credential_management_operations.push(CredentialOperation::Resolve);
        assert!(invalid.compose().is_err());
    }

    #[test]
    fn supported_operations_are_explicit_not_arbitrary_strings() {
        let bad = serde_json::json!({
            "provider": {"id":"p","artifact":"/p"},
            "credentials": {"id":"c","artifact":"/c"},
            "definitions": ["/defs"],
            "account": {
                "provider_id":"example",
                "connection_id":"api",
                "profile_id":"work"
            },
            "provider_credential_operations":["admin"]
        });
        assert!(serde_json::from_value::<ProviderDeployment>(bad).is_err());
    }
}
