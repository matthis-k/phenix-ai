from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one replacement, found {count}\n--- needle ---\n{old}")
    file.write_text(text.replace(old, new, 1))


# Persist provider account identity separately from bearer-token shape. This is
# needed by OAuth providers such as OpenAI Codex where account identity may be
# carried by the ID token rather than the access token.
replace_once(
    "rust/crates/phenix-provider-sdk/src/types.rs",
    '''    OAuth {
        access_token: Token,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<Secret>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
    },
''',
    '''    OAuth {
        access_token: Token,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<Secret>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
    },
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/store.rs",
    '''    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}
''',
    '''    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/store.rs",
    '''            Auth::OAuth {
                access_token,
                refresh_token,
                expires_at,
            } => {
''',
    '''            Auth::OAuth {
                access_token,
                refresh_token,
                expires_at,
                account_id,
            } => {
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/store.rs",
    '''                self.oauth = Some(OAuthCredential {
                    access_token,
                    refresh_token,
                    expires_at,
                });
''',
    '''                self.oauth = Some(OAuthCredential {
                    access_token,
                    refresh_token,
                    expires_at,
                    account_id,
                });
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/store.rs",
    '''            AuthKind::OAuth => self.oauth.as_ref().map(|oauth| Auth::OAuth {
                access_token: oauth.access_token.clone(),
                refresh_token: oauth.refresh_token.clone(),
                expires_at: oauth.expires_at,
            }),
''',
    '''            AuthKind::OAuth => self.oauth.as_ref().map(|oauth| Auth::OAuth {
                access_token: oauth.access_token.clone(),
                refresh_token: oauth.refresh_token.clone(),
                expires_at: oauth.expires_at,
                account_id: oauth.account_id.clone(),
            }),
''',
)

# Keep old credentials wire-compatible while preserving account identity on
# refresh and preferring claims from a newly returned ID token.
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''    let Auth::OAuth {
        access_token,
        refresh_token,
        expires_at,
    } = auth
''',
    '''    let Auth::OAuth {
        access_token,
        refresh_token,
        expires_at,
        account_id,
    } = auth
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''        return Ok(Auth::OAuth {
            access_token,
            refresh_token,
            expires_at,
        });
''',
    '''        return Ok(Auth::OAuth {
            access_token,
            refresh_token,
            expires_at,
            account_id,
        });
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''    let expires_at = token_expiry(access_token.expose())
        .or_else(|| response.expires_in.map(|seconds| now.saturating_add(seconds)));
    let refreshed = Auth::OAuth {
        access_token,
        refresh_token,
        expires_at,
    };
''',
    '''    let expires_at = token_expiry(access_token.expose())
        .or_else(|| response.expires_in.map(|seconds| now.saturating_add(seconds)));
    let account_id = oauth_account_id(method, response.id_token.as_deref(), access_token.expose())?
        .or(account_id);
    let refreshed = Auth::OAuth {
        access_token,
        refresh_token,
        expires_at,
        account_id,
    };
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''pub(crate) fn apply_headers(
    method: &OAuthMethod,
    access_token: &Token,
    headers: &mut BTreeMap<String, String>,
) -> Result<(), ProviderError> {
''',
    '''pub(crate) fn apply_headers(
    method: &OAuthMethod,
    access_token: &Token,
    account_id: Option<&str>,
    headers: &mut BTreeMap<String, String>,
) -> Result<(), ProviderError> {
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''    if let Some(header) = &method.account_id_header {
        let account_id = method
            .account_id_claim_paths
            .iter()
            .find_map(|path| jwt_string_claim(access_token.expose(), path))
            .ok_or_else(|| {
                oauth_error(format!(
                    "OAuth access token does not contain an account id required for {header}"
                ))
            })?;
        headers.insert(header.clone(), account_id);
    }
''',
    '''    if let Some(header) = &method.account_id_header {
        let account_id = account_id
            .map(ToOwned::to_owned)
            .or_else(|| {
                method
                    .account_id_claim_paths
                    .iter()
                    .find_map(|path| jwt_string_claim(access_token.expose(), path))
            })
            .ok_or_else(|| oauth_error(format!("OAuth credential has no account id required for {header}")))?;
        headers.insert(header.clone(), account_id);
    }
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''    let expires_at = token_expiry(access_token.expose())
        .or_else(|| response.expires_in.map(|seconds| now.saturating_add(seconds)));
    if method.account_id_header.is_some() {
        let account_in_access = method
            .account_id_claim_paths
            .iter()
            .any(|path| jwt_string_claim(access_token.expose(), path).is_some());
        let account_in_id = response.id_token.as_deref().is_some_and(|token| {
            method
                .account_id_claim_paths
                .iter()
                .any(|path| jwt_string_claim(token, path).is_some())
        });
        if !account_in_access && account_in_id {
            return Err(oauth_error(
                "OAuth account id is present only in the id token; provider request presentation requires it in the access token"
                    .to_owned(),
            ));
        }
    }
''',
    '''    let expires_at = token_expiry(access_token.expose())
        .or_else(|| response.expires_in.map(|seconds| now.saturating_add(seconds)));
    let account_id = oauth_account_id(method, response.id_token.as_deref(), access_token.expose())?;
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''        Auth::OAuth {
            access_token,
            refresh_token,
            expires_at,
        },
''',
    '''        Auth::OAuth {
            access_token,
            refresh_token,
            expires_at,
            account_id,
        },
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/oauth.rs",
    '''fn jwt_string_claim(token: &str, path: &[String]) -> Option<String> {
''',
    '''fn oauth_account_id(
    method: &OAuthMethod,
    id_token: Option<&str>,
    access_token: &str,
) -> Result<Option<String>, ProviderError> {
    if method.account_id_header.is_none() {
        return Ok(None);
    }
    let value = id_token
        .and_then(|token| {
            method
                .account_id_claim_paths
                .iter()
                .find_map(|path| jwt_string_claim(token, path))
        })
        .or_else(|| {
            method
                .account_id_claim_paths
                .iter()
                .find_map(|path| jwt_string_claim(access_token, path))
        });
    value.map(Some).ok_or_else(|| {
        oauth_error("OAuth token response does not contain the required account identity")
    })
}

fn jwt_string_claim(token: &str, path: &[String]) -> Option<String> {
''',
)

# Provider request application gets persisted identity alongside the bearer.
replace_once(
    "rust/crates/phenix-provider-sdk/src/runtime.rs",
    '''        Some(Auth::OAuth { access_token, .. }) => {
            let method = spec.auth.oauth.as_ref().ok_or_else(|| {
''',
    '''        Some(Auth::OAuth {
            access_token,
            account_id,
            ..
        }) => {
            let method = spec.auth.oauth.as_ref().ok_or_else(|| {
''',
)
replace_once(
    "rust/crates/phenix-provider-sdk/src/runtime.rs",
    '''            oauth::apply_headers(method, access_token, headers)
''',
    '''            oauth::apply_headers(method, access_token, account_id.as_deref(), headers)
''',
)

# Fix authentication status construction without borrowing a moved enum value.
replace_once(
    "rust/crates/phenix-harness/src/application_selection.rs",
    '''            methods.push(AuthenticationMethod {
                id: format!("{}:{suffix}", provider_id),
                name: format!("{} {label}", provider_id),
                description: Some(format!("Authenticate {} using {label}", provider_id)),
                kind,
                authenticated: authenticated.contains(&auth_kind(&kind)),
            });
''',
    '''            let is_authenticated = authenticated.contains(&auth_kind(&kind));
            methods.push(AuthenticationMethod {
                id: format!("{}:{suffix}", provider_id),
                name: format!("{} {label}", provider_id),
                description: Some(format!("Authenticate {} using {label}", provider_id)),
                kind,
                authenticated: is_authenticated,
            });
''',
)

# Expose authentication through the host-neutral Lua application facade.
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''        Acknowledged, Content, ElicitationRequest, ElicitationResponse, ExecutionChange,
        ExecutionState, InteractionHandlers, ModelSelectInput, Models, PageInput,
''',
    '''        Acknowledged, AuthenticateInput, AuthenticationMethods, AuthenticationResult, Content,
        ElicitationRequest, ElicitationResponse, Empty, ExecutionChange, ExecutionState,
        InteractionHandlers, ModelSelectInput, Models, PageInput,
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''    Cancel as AppCancel, CloseSession as AppCloseSession, CreateSession as AppCreateSession,
    DecideReview as AppDecideReview, GetProvenance as AppGetProvenance,
''',
    '''    Authenticate as AppAuthenticate, Cancel as AppCancel, CloseSession as AppCloseSession,
    CreateSession as AppCreateSession, DecideReview as AppDecideReview,
    DiscoverAuthentication as AppDiscoverAuthentication, GetProvenance as AppGetProvenance,
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''    SessionRename,
    Acknowledged,
    Prompt {
''',
    '''    SessionRename,
    AuthenticationMethods,
    AuthenticationResult,
    Acknowledged,
    Prompt {
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''    SessionInfo(SessionInfo),
    Acknowledged(Acknowledged),
    Prompt(PromptResult),
''',
    '''    SessionInfo(SessionInfo),
    AuthenticationMethods(AuthenticationMethods),
    AuthenticationResult(AuthenticationResult),
    Acknowledged(Acknowledged),
    Prompt(PromptResult),
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''        methods.add_method("sessions", |lua, this, ()| {
            lua.create_userdata(FacadeSessions {
                core: Rc::clone(&this.core),
            })
        });
''',
    '''        methods.add_method("sessions", |lua, this, ()| {
            lua.create_userdata(FacadeSessions {
                core: Rc::clone(&this.core),
            })
        });
        methods.add_method("authentication_methods", |lua, this, ()| {
            require_ready(&this.core)?;
            let request = application_request::<AppDiscoverAuthentication>(
                &this.core,
                Empty {},
                RequestProjection::AuthenticationMethods,
            )?;
            lua.create_userdata(request)
        });
        methods.add_method(
            "authenticate",
            |lua, this, (method_id, secret): (String, Option<String>)| {
                require_ready(&this.core)?;
                let request = application_request::<AppAuthenticate>(
                    &this.core,
                    AuthenticateInput { method_id, secret },
                    RequestProjection::AuthenticationResult,
                )?;
                lua.create_userdata(request)
            },
        );
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''        RequestProjection::Acknowledged => Ok(FacadeOutcome::Acknowledged(decode(&value)?)),
''',
    '''        RequestProjection::AuthenticationMethods => {
            Ok(FacadeOutcome::AuthenticationMethods(decode(&value)?))
        }
        RequestProjection::AuthenticationResult => {
            Ok(FacadeOutcome::AuthenticationResult(decode(&value)?))
        }
        RequestProjection::Acknowledged => Ok(FacadeOutcome::Acknowledged(decode(&value)?)),
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''        FacadeOutcome::SessionInfo(value) => {
            facade_value(lua, &value.to_value()).map_err(lua_error)
        }
        FacadeOutcome::Acknowledged(value) => {
''',
    '''        FacadeOutcome::SessionInfo(value) => {
            facade_value(lua, &value.to_value()).map_err(lua_error)
        }
        FacadeOutcome::AuthenticationMethods(value) => {
            facade_value(lua, &value.to_value()).map_err(lua_error)
        }
        FacadeOutcome::AuthenticationResult(value) => {
            facade_value(lua, &value.to_value()).map_err(lua_error)
        }
        FacadeOutcome::Acknowledged(value) => {
''',
)
replace_once(
    "rust/crates/phenix-binding-lua/src/facade.rs",
    '''    for (name, operation) in [
        ("models", AppListModels::ID),
''',
    '''    for (name, operation) in [
        ("authentication", AppDiscoverAuthentication::ID),
        ("models", AppListModels::ID),
''',
)

print("OAuth identity and Lua facade integration applied")
