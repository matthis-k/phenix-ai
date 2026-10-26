//! Provider selection, catalog pagination and native connection preflight.
//!
//! This module contains provider-domain orchestration rather than ACP request
//! structs or transport handling. Model selectors never confer kernel grants.
use super::*;

pub(super) struct ProviderManagement {
    pub(super) kernel: Kernel,
    pub(super) selected: Selection,
}

/// Trusted host multiplexer. Each root remains an independently authorized
/// kernel/composition, with no shared token store and no inferred grants.
pub(super) struct ProviderManagementGroup {
    pub(super) roots: Vec<ProviderManagement>,
}

/// Merge *descriptors*, not credentials or provider-owned mutable state.
/// Multiple connection implementations may contribute to the same displayed
/// provider, but claiming the same provider+connection is ambiguous and denied.
fn merge_provider_pages(pages: &[ProviderPage]) -> acp::Result<ProviderPage> {
    if pages.is_empty() || pages.len() > 32 {
        return Err(invalid("provider management root count is invalid"));
    }
    if pages.len() == 1 {
        let page = &pages[0];
        if page.revision == 0 || page.providers.len() > 32 {
            return Err(invalid("provider catalog is invalid"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut provider_ids = std::collections::BTreeSet::new();
        for provider in &page.providers {
            if provider.connections.len() > 32
                || provider.id.is_empty()
                || !provider_ids.insert(provider.id.clone())
            {
                return Err(invalid("invalid registered provider"));
            }
            for connection in &provider.connections {
                if !seen.insert((provider.id.clone(), connection.id.clone())) {
                    return Err(invalid("ambiguous provider connection"));
                }
            }
        }
        return Ok(page.clone());
    }
    let mut merged: Vec<ProviderDescriptor> = Vec::new();
    let mut identities = std::collections::BTreeSet::new();
    let mut digest = 14695981039346656037u64;
    for page in pages {
        if page.revision == 0 || page.providers.len() > 32 {
            return Err(invalid("provider catalog has invalid revision or size"));
        }
        for byte in page.revision.to_le_bytes() {
            digest = (digest ^ u64::from(byte)).wrapping_mul(1099511628211);
        }
        let mut source_ids = std::collections::BTreeSet::new();
        for provider in &page.providers {
            if provider.id.is_empty()
                || provider.connections.len() > 32
                || !source_ids.insert(provider.id.clone())
            {
                return Err(invalid("invalid registered provider"));
            }
            if let Some(existing) = merged.iter_mut().find(|item| item.id == provider.id) {
                if existing.label != provider.label {
                    return Err(invalid("conflicting provider labels"));
                }
                for connection in &provider.connections {
                    if !identities.insert((provider.id.clone(), connection.id.clone())) {
                        return Err(invalid("ambiguous provider connection"));
                    }
                    existing.connections.push(connection.clone());
                }
            } else {
                for connection in &provider.connections {
                    if !identities.insert((provider.id.clone(), connection.id.clone())) {
                        return Err(invalid("ambiguous provider connection"));
                    }
                }
                merged.push(provider.clone());
            }
            for byte in provider.id.bytes().chain([0]) {
                digest = (digest ^ u64::from(byte)).wrapping_mul(1099511628211);
            }
        }
    }
    if merged.len() > 32 || identities.len() > 32 {
        return Err(invalid("combined provider catalog exceeds limits"));
    }
    Ok(ProviderPage {
        providers: merged,
        revision: digest.max(1),
    })
}

impl ProviderManagementGroup {
    fn pages(&self) -> acp::Result<Vec<ProviderPage>> {
        self.roots
            .iter()
            .map(|root| {
                root.read(
                    "phenix.provider.catalog@1",
                    "list",
                    &ProviderQuery {
                        scope: "user".into(),
                    },
                )
            })
            .collect()
    }

    pub(super) fn list(&self) -> acp::Result<ProviderPage> {
        merge_provider_pages(&self.pages()?)
    }

    /// Route by the complete provider+connection identity after verifying no
    /// root duplicates it. An ACP supplied name is never a kernel grant.
    fn root_for(&self, provider_id: &str, connection_id: &str) -> acp::Result<&ProviderManagement> {
        let pages = self.pages()?;
        merge_provider_pages(&pages)?;
        let mut selected = None;
        for (root, page) in self.roots.iter().zip(pages.iter()) {
            if page.providers.iter().any(|provider| {
                provider.id == provider_id
                    && provider
                        .connections
                        .iter()
                        .any(|conn| conn.id == connection_id)
            }) && selected.replace(root).is_some()
            {
                return Err(invalid("ambiguous provider connection root"));
            }
        }
        selected.ok_or_else(|| {
            provider_rejection(ProviderError {
                code: ProviderErrorCode::NotFound,
                message: "provider connection is not installed".into(),
            })
        })
    }

    pub(super) fn read_for<Q: Portable, R: Portable>(
        &self,
        provider_id: &str,
        connection_id: &str,
        interface: &str,
        operation: &str,
        query: &Q,
    ) -> acp::Result<R> {
        self.root_for(provider_id, connection_id)?
            .read(interface, operation, query)
    }

    pub(super) fn current_http_binding(
        &self,
        selection: &ModelSelection,
    ) -> acp::Result<HttpModelBinding> {
        self.root_for(&selection.provider_id, &selection.connection_id)?
            .current_http_binding(selection)
    }

    pub(super) fn authorized_http_offers(
        &self,
        resolvers: &[ModelCredentialResolver],
    ) -> acp::Result<Vec<NativeModelOffer>> {
        // Verify installation identity before allowing any provider to
        // publish an executable choice. A duplicate is always a hard error.
        self.list()?;
        let mut offers = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for root in &self.roots {
            for offer in root.authorized_http_offers(resolvers)? {
                if offers.len() >= 128 || !ids.insert(offer.id.clone()) {
                    return Err(invalid("ambiguous or oversized native model options"));
                }
                offers.push(offer);
            }
        }
        Ok(offers)
    }

    pub(super) fn shutdown(self) -> Result<(), String> {
        for root in self.roots {
            root.kernel.shutdown().map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
}

/// Preserve the native provider's typed denial code without forwarding
/// provider prose (which may contain identity or credential fragments).
pub(super) fn provider_rejection(problem: ProviderError) -> acp::Error {
    acp::Error::invalid_params().data(serde_json::json!({
        "format": "phenix.provider.error.v1",
        "code": problem.code,
    }))
}

// Trusted catalog reads are made through the launcher's already-admitted
// control-plane kernel. Selector values never select a provider implementation
// or create new native grants.
/// Complete one provider-owned model catalog before admitting a choice.
/// A cursor is only pagination state, never a revision or authorization token.
#[derive(Default)]
pub(super) struct ModelPageAccumulator {
    pub(super) models: Vec<phenix_provider_contracts::ProviderModel>,
    pub(super) revision: Option<u64>,
    pub(super) cursors: std::collections::BTreeSet<String>,
    pub(super) pages: usize,
}

impl ModelPageAccumulator {
    pub(super) fn push(
        &mut self,
        page: ModelPage,
        profile: &Option<String>,
    ) -> acp::Result<Option<String>> {
        if self.pages >= 64
            || page.revision == 0
            || self.revision.is_some_and(|prev| prev != page.revision)
            || &page.profile_id != profile
            || page.models.len() > 256
            || self
                .models
                .len()
                .checked_add(page.models.len())
                .is_none_or(|count| count > 4096)
        {
            return Err(invalid(
                "provider catalog page changed or exceeded its budget",
            ));
        }
        self.pages += 1;
        self.revision = Some(page.revision);
        let next = page.next_cursor;
        if let Some(cursor) = &next {
            if page.models.is_empty()
                || cursor.is_empty()
                || cursor.len() > 1024
                || !self.cursors.insert(cursor.clone())
            {
                return Err(invalid("invalid or repeated provider catalog cursor"));
            }
        }
        self.models.extend(page.models);
        Ok(next)
    }

    pub(super) fn complete(self, profile: Option<String>) -> acp::Result<ModelPage> {
        Ok(ModelPage {
            profile_id: profile,
            models: self.models,
            next_cursor: None,
            revision: self
                .revision
                .ok_or_else(|| invalid("missing model catalog"))?,
        })
    }
}

// Preserve the plugin's declared domain outcome until the outer ACP adapter
// maps it to protocol errors. A provider denial is not a native host failure.
fn decode_provider_outcome<R: Portable>(
    outcome: Outcome,
) -> Result<Result<R, ProviderError>, phenix_contract::Failure> {
    match outcome {
        Outcome::Success(value) => Ok(Ok(R::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(ProviderError::from_value(value)?)),
    }
}

impl ProviderManagement {
    fn invoke_typed<Q: Portable, R: Portable>(
        &self,
        interface: &str,
        operation: &str,
        query: &Q,
    ) -> Result<Result<R, ProviderError>, phenix_contract::Failure> {
        let root = self.kernel.admit_root(Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: self.selected.provider.clone(),
        })?;
        decode_provider_outcome(root.invoke(&query.to_value())?)
    }

    pub(super) fn read<Q: Portable, R: Portable>(
        &self,
        interface: &str,
        operation: &str,
        query: &Q,
    ) -> acp::Result<R> {
        match self
            .invoke_typed(interface, operation, query)
            .map_err(internal)?
        {
            Ok(value) => Ok(value),
            Err(problem) => Err(provider_rejection(problem)),
        }
    }

    /// Controller-only preflight. Never treat UI-discovered models or a
    /// preflight result as an executable kernel grant. The later native
    /// installation must recheck auth/registration, resolve the exact secret
    /// and use the same session kernel.
    pub(super) fn current_http_binding(
        &self,
        desired: &ModelSelection,
    ) -> acp::Result<HttpModelBinding> {
        const SCOPE: &str = "user";

        let listed: ProviderPage = self.read(
            "phenix.provider.catalog@1",
            "list",
            &ProviderQuery {
                scope: SCOPE.into(),
            },
        )?;
        if listed.revision == 0 {
            return Err(invalid("provider list has no current revision"));
        }
        let mut candidates = listed
            .providers
            .iter()
            .filter(|p| p.id == desired.provider_id);
        let provided = candidates
            .next()
            .ok_or_else(|| invalid("unknown selected provider"))?;
        if candidates.next().is_some() {
            return Err(invalid("ambiguous selected provider"));
        }
        let described: ProviderDescriptor = self.read(
            "phenix.provider.catalog@1",
            "describe",
            &ProviderDetailsQuery {
                scope: SCOPE.into(),
                provider_id: desired.provider_id.clone(),
            },
        )?;
        if provided != &described {
            return Err(invalid("provider registration changed during selection"));
        }
        let auth: AuthenticationStatus = self.read(
            "phenix.provider.auth@1",
            "status",
            &AuthQuery {
                scope: SCOPE.into(),
                provider_id: desired.provider_id.clone(),
                connection_id: desired.connection_id.clone(),
            },
        )?;

        let mut collected = ModelPageAccumulator::default();
        let mut cursor: Option<String> = None;
        loop {
            let page: ModelPage = self.read(
                "phenix.provider.catalog@1",
                "models",
                &ModelQuery {
                    scope: SCOPE.into(),
                    provider_id: desired.provider_id.clone(),
                    connection_id: desired.connection_id.clone(),
                    profile_id: desired.profile_id.clone(),
                    cursor: cursor.clone(),
                    limit: 256,
                },
            )?;
            cursor = collected.push(page, &desired.profile_id)?;
            if cursor.is_none() {
                let complete = collected.complete(desired.profile_id.clone())?;
                let checked = validate_model_selection(&described, &auth, &complete, desired, true)
                    .map_err(provider_rejection)?;
                return bind_http_model(&described, checked).map_err(provider_rejection);
            }
        }
    }
}

/// A non-secret ACP option assembled from CURRENT plugin-owned provider
/// evidence, never from an arbitrary frontend model list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeModelOffer {
    id: String,
    pub(super) label: String,
    pub(super) selected: ModelSelection,
}

/// The complete provider/connection/account/protocol/endpoint identity is
/// carried by the ACP option ID. JSON is already the protocol's typed wire
/// representation; this selector is NOT an authority token. On submission,
/// compare the whole ID with current runtime-generated offers, not decoded
/// client claims. Distinct account choices must never alias one model name.
pub(super) fn native_model_option_id(
    selection: &ModelSelection,
    revision: u64,
) -> acp::Result<String> {
    let mut fields = serde_json::to_value(selection).map_err(internal)?;
    let object = fields
        .as_object_mut()
        .ok_or_else(|| internal("invalid native selection"))?;
    object.insert("session_revision".into(), serde_json::json!(revision));
    serde_json::to_string(&fields)
        .map(|value| format!("phenix.native-model@1:{value}"))
        .map_err(internal)
}

/// Classify a stale menu without treating client JSON as authority. Only
/// current runtime-issued catalog choices ever reach native installation.
pub(super) fn stale_native_model_choice(chosen: &str, current_revision: u64) -> bool {
    const PREFIX: &str = "phenix.native-model@1:";
    if chosen.len() > 4096 {
        return false;
    }
    chosen
        .strip_prefix(PREFIX)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|json| {
            json.get("session_revision")
                .and_then(|value| value.as_u64())
        })
        .is_some_and(|revision| revision < current_revision)
}

pub(super) fn stale_model_conflict() -> acp::Error {
    acp::Error::invalid_params().data(serde_json::json!({
        "format": "phenix.basic.error.v1",
        "code": ErrorCode::Conflict,
        "message": "model selection changed; refresh the ACP session model menu",
    }))
}

/// A prior revision choice is idempotent ONLY when it selected the exact
/// current model. A stale choice for another model is rejected on admission.
pub(super) fn is_previous_native_selection(
    chosen: &str,
    current: &ModelSelection,
    revision: u64,
) -> acp::Result<bool> {
    let Some(previous) = revision.checked_sub(1) else {
        return Ok(false);
    };
    Ok(native_model_option_id(current, previous)? == chosen)
}

/// One controller model-menu refresh must have a global bound, not one
/// budget independently repeated for every provider and connection.
pub(super) fn charge_model_option_budget(used: &mut usize, maximum: usize) -> acp::Result<()> {
    if *used >= maximum {
        return Err(invalid(
            "provider model selection exceeded discovery budget",
        ));
    }
    *used += 1;
    Ok(())
}

/// Only explicitly unavailable/not-yet-discovered provider catalogs are
/// absent choices. A transport/authority denial, malformed typed reply or
/// conflicting revision must not be confused with an empty model list.
pub(super) fn is_missing_model_discovery(code: &ProviderErrorCode) -> bool {
    matches!(
        code,
        ProviderErrorCode::Unavailable
            | ProviderErrorCode::AuthenticationRequired
            | ProviderErrorCode::NotFound
    )
}

// Missing discovery is a permitted optional capability, not a malformed
// success reply or a blanket suppression of provider/authority denials.
fn classify_model_page(result: Result<ModelPage, ProviderError>) -> acp::Result<Option<ModelPage>> {
    match result {
        Ok(page) => Ok(Some(page)),
        Err(problem) if is_missing_model_discovery(&problem.code) => Ok(None),
        Err(problem) => Err(provider_rejection(problem)),
    }
}

impl ProviderManagement {
    fn optional_model_page(&self, query: &ModelQuery) -> acp::Result<Option<ModelPage>> {
        let reply = self
            .invoke_typed("phenix.provider.catalog@1", "models", query)
            .map_err(internal)?;
        classify_model_page(reply)
    }

    /// Enumerate only models executable with the launcher's exact credential
    /// identity (or explicit no-auth). A stored key/profile is not itself a
    /// capability. Each choice is checked again before native install.
    pub(super) fn authorized_http_offers(
        &self,
        resolvers: &[ModelCredentialResolver],
    ) -> acp::Result<Vec<NativeModelOffer>> {
        self.authorized_http_offers_report(resolvers)
            .map(|(offers, _)| offers)
    }

    /// Audit each missing model surface without exposing provider responses,
    /// tokens or other accounts. An empty model picker is not a success claim.
    pub(super) fn authorized_http_offers_report(
        &self,
        resolvers: &[ModelCredentialResolver],
    ) -> acp::Result<(Vec<NativeModelOffer>, Vec<serde_json::Value>)> {
        let listed: ProviderPage = self.read(
            "phenix.provider.catalog@1",
            "list",
            &ProviderQuery {
                scope: "user".into(),
            },
        )?;
        if listed.revision == 0 || listed.providers.len() > 32 {
            return Err(invalid(
                "provider selection catalog is invalid or too large",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut offered = Vec::new();
        let mut denials = Vec::new();
        let mut inspected_connections = 0;
        let mut inspected_pages = 0;
        for provider in listed.providers {
            if provider.connections.len() > 32 || !ids.insert(provider.id.clone()) {
                return Err(invalid("ambiguous or oversized provider selection"));
            }
            let exact: ProviderDescriptor = self.read(
                "phenix.provider.catalog@1",
                "describe",
                &ProviderDetailsQuery {
                    scope: "user".into(),
                    provider_id: provider.id.clone(),
                },
            )?;
            if exact != provider {
                return Err(invalid("provider changed while listing model options"));
            }
            let mut connections = std::collections::BTreeSet::new();
            for connection in &exact.connections {
                charge_model_option_budget(&mut inspected_connections, 32)?;
                if !connections.insert(&connection.id) {
                    return Err(invalid("duplicate provider connection"));
                }
                let profiles = if connection.auth_methods.is_empty() {
                    vec![None]
                } else {
                    // Each resolver corresponds to one independently granted
                    // exact resource. Named accounts on the same connection
                    // are separate catalog/authorization selections.
                    resolvers
                        .iter()
                        .filter(|source| {
                            source.identity.provider_id() == exact.id
                                && source.identity.connection_id() == connection.id
                        })
                        .map(|source| Some(source.identity.profile_id().to_owned()))
                        .collect::<Vec<_>>()
                };
                if profiles.is_empty() {
                    denials.push(serde_json::json!({
                        "provider_id": exact.id,
                        "connection_id": connection.id,
                        "code": "no_authorized_account_resolver",
                        "message": "No exact-account model credential resolver was installed",
                    }));
                    continue;
                }
                let auth: AuthenticationStatus = self.read(
                    "phenix.provider.auth@1",
                    "status",
                    &AuthQuery {
                        scope: "user".into(),
                        provider_id: exact.id.clone(),
                        connection_id: connection.id.clone(),
                    },
                )?;
                for profile in profiles {
                    if let Some(profile_id) = profile.as_deref() {
                        if !auth.profile_is_valid_for(&exact.id, connection, profile_id) {
                            denials.push(serde_json::json!({
                                "provider_id": exact.id,
                                "connection_id": connection.id,
                                "profile_id": profile_id,
                                "code": "account_not_authenticated",
                                "message": "Account is not authenticated or its grant has expired",
                            }));
                            continue;
                        }
                    }

                    let mut collected = ModelPageAccumulator::default();
                    let mut cursor = None;
                    let full = loop {
                        charge_model_option_budget(&mut inspected_pages, 128)?;
                        let Some(page) = self.optional_model_page(&ModelQuery {
                            scope: "user".into(),
                            provider_id: exact.id.clone(),
                            connection_id: connection.id.clone(),
                            profile_id: profile.clone(),
                            cursor: cursor.clone(),
                            limit: 256,
                        })?
                        else {
                            // Missing discovery is not silently treated as a
                            // healthy empty model catalog.
                            denials.push(serde_json::json!({
                                "provider_id": exact.id,
                                "connection_id": connection.id,
                                "profile_id": profile,
                                "code": "catalog_unavailable",
                                "message": "Provider model discovery is unavailable or denied",
                            }));
                            break None;
                        };
                        cursor = collected.push(page, &profile)?;
                        if cursor.is_none() {
                            break Some(collected.complete(profile.clone())?);
                        }
                    };
                    let Some(full) = full else {
                        continue;
                    };
                    if full.models.is_empty() {
                        denials.push(serde_json::json!({
                            "provider_id": exact.id,
                            "connection_id": connection.id,
                            "profile_id": profile,
                            "code": "catalog_empty",
                            "message": "The provider returned no selectable models",
                        }));
                    }
                    let before_count = offered.len();
                    for model in &full.models {
                        if !model.selectable || model.connection_id != connection.id {
                            continue;
                        }
                        let desired = ModelSelection {
                            provider_id: exact.id.clone(),
                            connection_id: connection.id.clone(),
                            profile_id: profile.clone(),
                            model_id: model.id.clone(),
                            protocol_id: model.protocol_id.clone(),
                            endpoint_id: model.endpoint_id.clone(),
                            catalog_revision: full.revision,
                        };
                        let Ok(admitted) =
                            validate_model_selection(&exact, &auth, &full, &desired, true)
                        else {
                            continue;
                        };
                        let Ok(binding) = bind_http_model(&exact, admitted) else {
                            continue;
                        };
                        // Only installed and verified API key or OAuth
                        // bearer methods can become executable selections.
                        if !matches!(
                            binding.authentication_kind(),
                            None | Some(AuthenticationKind::ApiKey)
                                | Some(AuthenticationKind::OAuthAuthorizationCode)
                                | Some(AuthenticationKind::OAuthDeviceCode)
                        ) {
                            continue;
                        }
                        if binding.credential_identity().is_some()
                            && !resolvers
                                .iter()
                                .any(|r| binding.credential_identity() == Some(&r.identity))
                        {
                            continue;
                        }
                        if offered.len() >= 128 {
                            return Err(invalid("too many selectable HTTP models"));
                        }
                        offered.push(NativeModelOffer {
                            id: native_model_option_id(&desired, 0)?,
                            label: format!(
                                "{} / {} / {} / {}",
                                exact.label,
                                connection.label,
                                profile.as_deref().unwrap_or("local"),
                                model.label
                            ),
                            selected: desired,
                        });
                    }
                    if !full.models.is_empty() && offered.len() == before_count {
                        denials.push(serde_json::json!({
                            "provider_id": exact.id,
                            "connection_id": connection.id,
                            "profile_id": profile,
                            "code": "model_admission_denied",
                            "message": "Discovered models did not pass executable model or credential admission",
                        }));
                    }
                }
            }
        }
        let mut distinct = std::collections::BTreeSet::new();
        if offered.iter().any(|entry| !distinct.insert(&entry.id)) {
            return Err(invalid("duplicate native model option"));
        }
        Ok((offered, denials))
    }
}

#[cfg(test)]
mod typed_result_tests {
    use super::*;

    #[test]
    fn domain_failure_remains_typed_before_acp_mapping() {
        let failure = ProviderError {
            code: ProviderErrorCode::Denied,
            message: "denied".into(),
        };
        let reply = decode_provider_outcome::<ModelPage>(Outcome::Domain(failure.to_value()))
            .expect("valid native reply");
        assert_eq!(reply.unwrap_err().code, ProviderErrorCode::Denied);
    }

    #[test]
    fn missing_discovery_is_optional_but_denied_discovery_is_not() {
        let missing = ProviderError {
            code: ProviderErrorCode::AuthenticationRequired,
            message: "no credentials".into(),
        };
        assert!(classify_model_page(Err(missing)).unwrap().is_none());

        let denied = ProviderError {
            code: ProviderErrorCode::Denied,
            message: "access denied".into(),
        };
        assert!(classify_model_page(Err(denied)).is_err());
    }

    #[test]
    fn successful_native_reply_decodes_through_the_contract() {
        let page = ProviderPage {
            providers: Vec::new(),
            revision: 1,
        };
        let response = decode_provider_outcome::<ProviderPage>(Outcome::Success(page.to_value()))
            .expect("valid native reply")
            .expect("provider success");
        assert_eq!(response.revision, 1);
    }
}

#[cfg(test)]
mod multi_provider_tests {
    use super::*;
    use phenix_provider_contracts::ConnectionDescriptor;

    fn connection(id: &str) -> ConnectionDescriptor {
        ConnectionDescriptor {
            id: id.into(),
            label: id.into(),
            protocols: Vec::new(),
            base_url: None,
            auth_methods: Vec::new(),
            model_sources: Vec::new(),
        }
    }

    fn page(provider: &str, label: &str, connection_id: &str, revision: u64) -> ProviderPage {
        ProviderPage {
            revision,
            providers: vec![ProviderDescriptor {
                id: provider.into(),
                label: label.into(),
                connections: vec![connection(connection_id)],
            }],
        }
    }

    #[test]
    fn one_product_exposes_separate_api_and_oauth_connections() {
        let api = page("openai", "OpenAI", "api", 4);
        let oauth = page("openai", "OpenAI", "chatgpt", 5);
        let merged = merge_provider_pages(&[api.clone(), oauth]).unwrap();
        assert_eq!(merged.providers.len(), 1);
        assert_eq!(merged.providers[0].connections.len(), 2);
        assert_eq!(merged.providers[0].connections[0].id, "api");
        assert_eq!(merged.providers[0].connections[1].id, "chatgpt");
        assert_ne!(merged.revision, 0);
        assert_eq!(
            merge_provider_pages(std::slice::from_ref(&api)).unwrap(),
            api
        );
        let changed = merge_provider_pages(&[api, page("openai", "OpenAI", "chatgpt", 6)]).unwrap();
        assert_ne!(merged.revision, changed.revision);
    }

    #[test]
    fn two_roots_cannot_shadow_an_account_connection() {
        let api = page("openai", "OpenAI", "api", 4);
        assert!(merge_provider_pages(&[api.clone(), api]).is_err());
        assert!(merge_provider_pages(&[
            page("openai", "OpenAI", "api", 1),
            page("openai", "OpenAI ChatGPT", "chatgpt", 2),
        ])
        .is_err());
        assert!(merge_provider_pages(&[page("openai", "OpenAI", "api", 0)]).is_err());
        assert!(merge_provider_pages(&[]).is_err());
        // A single malicious provider implementation cannot split one
        // provider descriptor into two records and evade duplicate detection.
        let mut duplicate = page("openai", "OpenAI", "api", 7);
        duplicate.providers.push(ProviderDescriptor {
            id: "openai".into(),
            label: "OpenAI".into(),
            connections: vec![connection("chatgpt")],
        });
        assert!(merge_provider_pages(&[duplicate]).is_err());
    }

    #[test]
    fn different_installed_provider_identities_remain_distinct() {
        let page = merge_provider_pages(&[
            page("openai", "OpenAI", "api", 1),
            page("anthropic", "Anthropic", "api", 2),
        ])
        .unwrap();
        assert_eq!(page.providers.len(), 2);
        assert_eq!(page.providers[0].id, "openai");
        assert_eq!(page.providers[1].id, "anthropic");
    }
}
