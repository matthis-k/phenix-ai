use crate::credentials::{CredentialStore, StoredCredential};
use genai::resolver::AuthData;
use genai::Headers;
use oauth2::{
    AsyncHttpClient, AuthUrl, AuthorizationCode, ClientId, HttpRequest, HttpResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

pub(crate) const PROVIDER: &str = "openai-codex";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const REFRESH_MARGIN_SECONDS: u64 = 5 * 60;
const SCOPE: &str = "openid profile email offline_access api.connectors.read api.connectors.invoke";

#[derive(Clone)]
pub(crate) struct CodexOAuth {
    store: CredentialStore,
    refresh_lock: Arc<Mutex<()>>,
}

impl CodexOAuth {
    pub(crate) fn new(store: CredentialStore) -> Self {
        Self {
            store,
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn auth_data(&self) -> Result<Option<AuthData>, String> {
        let _guard = self.refresh_lock.lock().await;
        let Some(credential) = self.store.resolve(PROVIDER)? else {
            return Ok(None);
        };
        let credential = match credential {
            StoredCredential::OAuth { expires_at, .. }
                if expires_at <= unix_time()?.saturating_add(REFRESH_MARGIN_SECONDS) =>
            {
                refresh(&self.store, credential).await?
            }
            StoredCredential::OAuth { .. } => credential,
            StoredCredential::ApiKey { .. } => {
                return Err("openai-codex requires ChatGPT OAuth, not an API key".to_owned());
            }
        };
        let StoredCredential::OAuth {
            access_token,
            account_id,
            ..
        } = credential
        else {
            unreachable!("credential variant checked above")
        };
        Ok(Some(AuthData::RequestOverride {
            url: RESPONSES_URL.to_owned(),
            headers: Headers::from([
                (
                    "Authorization",
                    format!("Bearer {}", access_token.expose_secret()),
                ),
                ("ChatGPT-Account-ID", account_id),
                ("originator", "phenix".to_owned()),
                ("version", env!("CARGO_PKG_VERSION").to_owned()),
            ]),
        }))
    }
}

pub(crate) async fn login(store: &CredentialStore) -> Result<(), String> {
    let listener = match TcpListener::bind(("127.0.0.1", 1455)).await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind(("127.0.0.1", 1457))
            .await
            .map_err(|error| {
                format!("cannot bind OAuth callback on ports 1455 or 1457: {error}")
            })?,
    };
    let port = listener
        .local_addr()
        .map_err(|error| format!("cannot inspect OAuth callback address: {error}"))?
        .port();
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    let client = codex_client(&redirect_uri)?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (authorization_url, state) = codex_authorization_url(&client, &challenge)?;

    eprintln!("Sign in with ChatGPT to authorize Phenix:\n\n{authorization_url}\n");
    eprintln!("Waiting for the verified OAuth callback on localhost:{port} …");

    let result = tokio::time::timeout(LOGIN_TIMEOUT, receive_callback(listener, &state)).await;
    let code = match result {
        Ok(result) => result?,
        Err(_) => return Err("OAuth login timed out after 10 minutes".to_owned()),
    };
    let tokens = exchange_code(&client, &code, verifier).await?;
    let credential = credential_from_tokens(tokens)?;
    store.save_oauth(PROVIDER, credential)?;
    Ok(())
}

/// OAuth client bound to the Codex provider endpoints and redirect URI.
fn codex_client(redirect_uri: &str) -> Result<CodexClient, String> {
    let client = oauth2::Client::new(ClientId::new(CLIENT_ID.to_owned()))
        .set_auth_uri(
            AuthUrl::new(format!("{ISSUER}/oauth/authorize"))
                .map_err(|error| format!("invalid OAuth issuer: {error}"))?,
        )
        .set_token_uri(
            TokenUrl::new(TOKEN_URL.to_owned())
                .map_err(|error| format!("invalid OAuth token URL: {error}"))?,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_owned())
                .map_err(|error| format!("invalid OAuth redirect URI: {error}"))?,
        );
    Ok(client)
}

/// Codex authorization URL plus the CSRF state token to match in the callback.
fn codex_authorization_url(
    client: &CodexClient,
    challenge: &PkceCodeChallenge,
) -> Result<(Url, oauth2::CsrfToken), String> {
    let (url, state) = client
        .authorize_url(oauth2::CsrfToken::new_random)
        .add_scope(Scope::new(SCOPE.to_owned()))
        .set_pkce_challenge(challenge.clone())
        .add_extra_param("id_token_add_organizations", "true")
        .add_extra_param("codex_cli_simplified_flow", "true")
        .add_extra_param("originator", "phenix")
        .url();
    Ok((url, state))
}

async fn receive_callback(
    listener: TcpListener,
    expected_state: &oauth2::CsrfToken,
) -> Result<String, String> {
    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|error| format!("OAuth callback failed: {error}"))?;
    let mut request = vec![0_u8; 16 * 1024];
    let length = stream
        .read(&mut request)
        .await
        .map_err(|error| format!("cannot read OAuth callback: {error}"))?;
    let request = std::str::from_utf8(&request[..length])
        .map_err(|_| "OAuth callback was not valid HTTP".to_owned())?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "OAuth callback did not contain a request target".to_owned())?;
    let url = Url::parse(&format!("http://localhost{target}"))
        .map_err(|error| format!("invalid OAuth callback URL: {error}"))?;
    let query = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    let result = if let Some(error) = query.get("error") {
        Err(format!("OAuth authorization was rejected: {error}"))
    } else if query.get("state").map(|value| value.as_ref()) != Some(expected_state.secret()) {
        Err("OAuth callback state verification failed".to_owned())
    } else {
        query
            .get("code")
            .map(|value| value.to_string())
            .ok_or_else(|| "OAuth callback did not contain an authorization code".to_owned())
    };
    let (status, body) = if result.is_ok() {
        (
            "200 OK",
            "Phenix authentication completed. You may close this tab.",
        )
    } else {
        (
            "400 Bad Request",
            "Phenix authentication failed. Return to Neovim for details.",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    result
}

/// Non-standard token fields the Codex endpoint returns.
#[derive(Debug, Default, Deserialize, Serialize)]
struct CodexTokenExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id_token: Option<String>,
}

impl oauth2::ExtraTokenFields for CodexTokenExtras {}

type CodexToken = oauth2::StandardTokenResponse<CodexTokenExtras, oauth2::basic::BasicTokenType>;

/// Codex client specialization carrying `CodexTokenExtras` (the id token) on its
/// token response, unlike `oauth2::BasicClient` which drops non-standard fields.
type CodexClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    CodexToken,
    oauth2::basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

async fn exchange_code(
    client: &CodexClient,
    code: &str,
    verifier: PkceCodeVerifier,
) -> Result<CodexToken, String> {
    client
        .exchange_code(AuthorizationCode::new(code.to_owned()))
        .set_pkce_verifier(verifier)
        .request_async(&ReqwestHttp)
        .await
        .map_err(|error| format!("OAuth code exchange failed: {error}"))
}

async fn refresh(
    store: &CredentialStore,
    credential: StoredCredential,
) -> Result<StoredCredential, String> {
    refresh_with(store, credential, &ReqwestHttp, unix_time()?).await
}

async fn refresh_with<C>(
    store: &CredentialStore,
    credential: StoredCredential,
    http_client: &C,
    now: u64,
) -> Result<StoredCredential, String>
where
    C: for<'c> AsyncHttpClient<'c>,
{
    let StoredCredential::OAuth {
        refresh_token,
        id_token,
        account_id,
        ..
    } = credential
    else {
        return Err("cannot refresh a non-OAuth credential".to_owned());
    };
    let client = codex_client("http://localhost/oauth/callback")?;
    let response = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.expose_secret().to_owned()))
        .request_async(http_client)
        .await
        .map_err(|error| format!("OAuth token refresh failed: {error}"))?;
    let refreshed_access = response.access_token().secret().to_owned();
    let refreshed_refresh = response
        .refresh_token()
        .map(|token| token.secret().to_owned())
        .unwrap_or_else(|| refresh_token.expose_secret().to_owned());
    let refreshed_id = response
        .extra_fields()
        .id_token
        .clone()
        .unwrap_or_else(|| id_token.expose_secret().to_owned());
    let account_id = account_id_from_token(&refreshed_id)
        .or_else(|| account_id_from_token(&refreshed_access))
        .unwrap_or(account_id);
    let expires_at = token_expiry(&refreshed_access).unwrap_or(now.saturating_add(3600));
    let refreshed = StoredCredential::OAuth {
        access_token: SecretString::from(refreshed_access),
        refresh_token: SecretString::from(refreshed_refresh),
        id_token: SecretString::from(refreshed_id),
        account_id,
        expires_at,
    };
    store.save_oauth(PROVIDER, refreshed.clone())?;
    Ok(refreshed)
}

fn credential_from_tokens(tokens: CodexToken) -> Result<StoredCredential, String> {
    credential_from_tokens_at(tokens, unix_time()?)
}

fn credential_from_tokens_at(tokens: CodexToken, now: u64) -> Result<StoredCredential, String> {
    let access_token = tokens.access_token().secret().to_owned();
    let refresh_token = tokens
        .refresh_token()
        .map(|token| token.secret().to_owned())
        .ok_or_else(|| "OAuth token response did not include a refresh token".to_owned())?;
    let id_token = tokens
        .extra_fields()
        .id_token
        .clone()
        .ok_or_else(|| "OAuth token response did not include an ID token".to_owned())?;
    let account_id = account_id_from_token(&id_token)
        .or_else(|| account_id_from_token(&access_token))
        .ok_or_else(|| "OAuth token does not identify a ChatGPT account".to_owned())?;
    let expires_at = token_expiry(&access_token).unwrap_or(now.saturating_add(3600));
    Ok(StoredCredential::OAuth {
        access_token: SecretString::from(access_token),
        refresh_token: SecretString::from(refresh_token),
        id_token: SecretString::from(id_token),
        account_id,
        expires_at,
    })
}

/// Decode a JWT's claims WITHOUT verifying its signature or validating its
/// claims.
///
/// The Codex id/access tokens carry account identity and expiry claims that are
/// not independently verifiable here (no JWKS/public key is configured for the
/// ChatGPT issuer), so claim extraction is intentionally unverified. Every
/// validation switch is disabled (signature, expiry, audience, not-before, and
/// the required-claim set), so `exp` is read but not enforced and a token that
/// carries an `aud` claim is not rejected. This helper is extraction-only:
/// it must never be used to authorize, and any signature/claims verification
/// must be added separately before the token is trusted.
fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
    let mut validation = Validation::new(Algorithm::RS256);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.validate_nbf = false;
    validation.required_spec_claims.clear();
    decode::<serde_json::Value>(
        token,
        // No public key is used: signature validation is disabled above.
        &DecodingKey::from_secret(b"unused"),
        &validation,
    )
    .map(|data| data.claims)
    .ok()
}

fn token_expiry(token: &str) -> Option<u64> {
    jwt_payload(token)?
        .get("exp")
        .and_then(serde_json::Value::as_u64)
}

fn account_id_from_token(token: &str) -> Option<String> {
    let claims = jwt_payload(token)?;
    claims
        .get("chatgpt_account_id")
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
        })?
        .as_str()
        .map(ToOwned::to_owned)
}

fn unix_time() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("system clock predates Unix epoch: {error}"))
}

/// Blocking-free HTTP client adapter mapping `oauth2`'s request types onto the
/// workspace `reqwest` (0.13) transport without pulling a second `reqwest` into the tree.
struct ReqwestHttp;

impl<'c> AsyncHttpClient<'c> for ReqwestHttp {
    type Error = oauth2::HttpClientError<std::io::Error>;

    type Future =
        Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + Sync + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method().as_str().as_bytes())
                .map_err(|error| {
                    oauth2::HttpClientError::Other(format!(
                        "unsupported OAuth request method: {error}"
                    ))
                })?;
            let client = reqwest::Client::new();
            let mut builder = client.request(method, request.uri().to_string());
            for (name, value) in request.headers() {
                builder = builder.header(
                    name.as_str(),
                    value.to_str().map_err(|error| {
                        oauth2::HttpClientError::Other(format!(
                            "invalid OAuth request header: {error}"
                        ))
                    })?,
                );
            }
            builder = builder.body(request.into_body());
            let response = builder.send().await.map_err(|error| {
                oauth2::HttpClientError::Other(format!("OAuth transport request failed: {error}"))
            })?;
            let status = response.status();
            let headers = response.headers().clone();
            let body = response.bytes().await.map_err(|error| {
                oauth2::HttpClientError::Other(format!("OAuth transport read failed: {error}"))
            })?;
            let mut response_builder = oauth2::http::Response::builder().status(status.as_u16());
            for (name, value) in &headers {
                response_builder = response_builder.header(name.as_str(), value.as_bytes());
            }
            response_builder
                .body(body.to_vec())
                .map_err(oauth2::HttpClientError::Http)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oauth2::basic::BasicTokenType;
    use oauth2::AccessToken;

    const NOW: u64 = 1_700_000_000;

    /// Build a deterministic HS256 JWT with the given claims (signature is not
    /// relevant to extraction, but the token must be well formed).
    fn token(claims: serde_json::Value) -> String {
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"fixture"),
        )
        .expect("fixture token encodes")
    }

    fn codex_token(access: &str, refresh: Option<&str>, id: Option<&str>) -> CodexToken {
        let extra = CodexTokenExtras {
            id_token: id.map(str::to_owned),
        };
        let mut token = CodexToken::new(
            AccessToken::new(access.to_owned()),
            BasicTokenType::Bearer,
            extra,
        );
        token.set_refresh_token(refresh.map(|value| RefreshToken::new(value.to_owned())));
        token
    }

    fn oauth_credential(credential: &StoredCredential) -> (String, String, String, String, u64) {
        let StoredCredential::OAuth {
            access_token,
            refresh_token,
            id_token,
            account_id,
            expires_at,
        } = credential
        else {
            panic!("expected an OAuth credential");
        };
        (
            access_token.expose_secret().to_owned(),
            refresh_token.expose_secret().to_owned(),
            id_token.expose_secret().to_owned(),
            account_id.clone(),
            *expires_at,
        )
    }

    #[test]
    fn authorization_url_preserves_provider_query_contract() {
        let client = codex_client("http://localhost:1455/auth/callback").unwrap();
        let (challenge, _) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = codex_authorization_url(&client, &challenge).unwrap();
        let query = url
            .query_pairs()
            .into_owned()
            .collect::<std::collections::BTreeMap<_, _>>();

        for (key, expected) in [
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", "http://localhost:1455/auth/callback"),
            ("scope", SCOPE),
            ("code_challenge_method", "S256"),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "phenix"),
        ] {
            assert_eq!(query.get(key).map(String::as_str), Some(expected), "{key}");
        }
        assert!(!query["code_challenge"].is_empty());
        assert_eq!(query["state"].as_str(), state.secret().as_str());
        assert_eq!(query.len(), 10);
        assert!(!query.contains_key("version"));
    }

    #[test]
    fn extraction_reads_account_id_from_id_and_access_tokens() {
        let id_token = token(serde_json::json!({
            "chatgpt_account_id": "account-123",
            "exp": NOW + 60 * 60,
        }));
        assert_eq!(
            account_id_from_token(&id_token).as_deref(),
            Some("account-123")
        );

        let access_token = token(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "account-456" },
            "exp": NOW + 60 * 60,
        }));
        assert_eq!(
            account_id_from_token(&access_token).as_deref(),
            Some("account-456")
        );
        assert_eq!(token_expiry(&access_token), Some(NOW + 60 * 60));
    }

    #[test]
    fn extraction_does_not_reject_audience_bearing_tokens() {
        let token = token(serde_json::json!({
            "iss": "https://auth.openai.com",
            "aud": "https://api.openai.com",
            "chatgpt_account_id": "account-aud",
            "exp": NOW + 60 * 60,
        }));
        assert_eq!(
            account_id_from_token(&token).as_deref(),
            Some("account-aud")
        );
    }

    #[test]
    fn extraction_does_not_reject_expired_claims() {
        let expired = token(serde_json::json!({
            "chatgpt_account_id": "account-expired",
            "exp": NOW - 60 * 60,
        }));
        assert_eq!(
            account_id_from_token(&expired).as_deref(),
            Some("account-expired")
        );
        assert_eq!(token_expiry(&expired), Some(NOW - 60 * 60));
    }

    #[test]
    fn malformed_tokens_yield_no_claims() {
        assert!(jwt_payload("not-a-jwt").is_none());
        assert!(jwt_payload("a.b.c.d").is_none());
        assert!(jwt_payload("").is_none());
        let bad_payload = token(serde_json::json!({ "exp": "not-a-number" }));
        assert!(token_expiry(&bad_payload).is_none());
    }

    #[test]
    fn missing_account_id_reads_as_none() {
        let token = token(serde_json::json!({ "exp": NOW + 60 * 60 }));
        assert_eq!(account_id_from_token(&token), None);
    }

    #[test]
    fn credential_creation_prefers_id_token_account_id() {
        let id_token = token(serde_json::json!({
            "chatgpt_account_id": "id-account",
            "exp": NOW + 60 * 60,
        }));
        let access_token = token(serde_json::json!({
            "chatgpt_account_id": "access-account",
            "exp": NOW + 30 * 60,
        }));
        let credential = credential_from_tokens_at(
            codex_token(&access_token, Some("refresh"), Some(&id_token)),
            NOW,
        )
        .unwrap();
        let (_, refresh, _, account_id, expires_at) = oauth_credential(&credential);
        assert_eq!(refresh, "refresh");
        assert_eq!(account_id, "id-account");
        assert_eq!(expires_at, NOW + 30 * 60);
    }

    #[test]
    fn credential_creation_falls_back_to_access_token_account_id() {
        let id_token = token(serde_json::json!({ "exp": NOW + 60 * 60 }));
        let access_token = token(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "access-account" },
            "exp": NOW + 30 * 60,
        }));
        let credential = credential_from_tokens_at(
            codex_token(&access_token, Some("refresh"), Some(&id_token)),
            NOW,
        )
        .unwrap();
        let (_, _, _, account_id, _) = oauth_credential(&credential);
        assert_eq!(account_id, "access-account");
    }

    #[test]
    fn credential_creation_selects_access_expiry_even_when_expired_or_audience_bearing() {
        let id_token = token(serde_json::json!({
            "chatgpt_account_id": "id-account",
            "exp": NOW + 60 * 60,
        }));
        let access_token = token(serde_json::json!({
            "aud": "https://api.openai.com",
            "chatgpt_account_id": "access-account",
            "exp": NOW - 60 * 60,
        }));
        let credential = credential_from_tokens_at(
            codex_token(&access_token, Some("refresh"), Some(&id_token)),
            NOW,
        )
        .unwrap();
        let (_, _, _, account_id, expires_at) = oauth_credential(&credential);
        assert_eq!(account_id, "id-account");
        assert_eq!(expires_at, NOW - 60 * 60);
    }

    #[test]
    fn credential_creation_rejects_missing_account_id() {
        let id_token = token(serde_json::json!({ "exp": NOW + 60 * 60 }));
        let access_token = token(serde_json::json!({ "exp": NOW + 30 * 60 }));
        let error = credential_from_tokens_at(
            codex_token(&access_token, Some("refresh"), Some(&id_token)),
            NOW,
        )
        .unwrap_err();
        assert_eq!(error, "OAuth token does not identify a ChatGPT account");
    }

    #[test]
    fn credential_creation_rejects_missing_refresh_token() {
        let id_token = token(serde_json::json!({
            "chatgpt_account_id": "id-account",
            "exp": NOW + 60 * 60,
        }));
        let access_token = token(serde_json::json!({
            "chatgpt_account_id": "access-account",
            "exp": NOW + 30 * 60,
        }));
        let error =
            credential_from_tokens_at(codex_token(&access_token, None, Some(&id_token)), NOW)
                .unwrap_err();
        assert_eq!(
            error,
            "OAuth token response did not include a refresh token"
        );
    }

    #[test]
    fn credential_creation_rejects_missing_id_token() {
        let access_token = token(serde_json::json!({
            "chatgpt_account_id": "access-account",
            "exp": NOW + 30 * 60,
        }));
        let error =
            credential_from_tokens_at(codex_token(&access_token, Some("refresh"), None), NOW)
                .unwrap_err();
        assert_eq!(error, "OAuth token response did not include an ID token");
    }

    #[test]
    fn credential_creation_uses_one_hour_fallback_for_missing_or_invalid_expiry() {
        let id_token = token(serde_json::json!({ "chatgpt_account_id": "id-account" }));
        let access_without_exp = token(serde_json::json!({
            "chatgpt_account_id": "access-account",
        }));
        let credential = credential_from_tokens_at(
            codex_token(&access_without_exp, Some("refresh"), Some(&id_token)),
            NOW,
        )
        .unwrap();
        let (_, _, _, _, expires_at) = oauth_credential(&credential);
        assert_eq!(expires_at, NOW + 3600);

        let malformed_expiry = token(serde_json::json!({
            "chatgpt_account_id": "access-account",
            "exp": "not-a-number",
        }));
        let credential = credential_from_tokens_at(
            codex_token(&malformed_expiry, Some("refresh"), Some(&id_token)),
            NOW,
        )
        .unwrap();
        let (_, _, _, _, expires_at) = oauth_credential(&credential);
        assert_eq!(expires_at, NOW + 3600);
    }

    /// A deterministic fake token endpoint. `body` is the raw JSON response the
    /// endpoint returns; it is cloned so the same closure answers every call.
    fn fake_token_endpoint(body: String) -> impl for<'c> AsyncHttpClient<'c> {
        move |_request: HttpRequest| {
            let body = body.clone();
            async move { Ok::<_, std::io::Error>(HttpResponse::new(body.into_bytes())) }
        }
    }

    fn token_response(access: &str, refresh: Option<&str>, id: Option<&str>) -> String {
        let mut response = serde_json::Map::new();
        response.insert("access_token".to_owned(), access.into());
        response.insert("token_type".to_owned(), "Bearer".into());
        if let Some(refresh) = refresh {
            response.insert("refresh_token".to_owned(), refresh.into());
        }
        if let Some(id) = id {
            response.insert("id_token".to_owned(), id.into());
        }
        serde_json::Value::Object(response).to_string()
    }

    fn temp_store() -> CredentialStore {
        CredentialStore {
            path: std::env::temp_dir().join(format!(
                "phenix-oauth-refresh-test-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            )),
        }
    }

    fn stored_oauth(
        access: &str,
        refresh: &str,
        id: &str,
        account_id: &str,
        expires_at: u64,
    ) -> StoredCredential {
        StoredCredential::OAuth {
            access_token: SecretString::from(access.to_owned()),
            refresh_token: SecretString::from(refresh.to_owned()),
            id_token: SecretString::from(id.to_owned()),
            account_id: account_id.to_owned(),
            expires_at,
        }
    }

    #[tokio::test]
    async fn refresh_replaces_tokens_and_selects_expiry() {
        let store = temp_store();
        let refreshed_access = token(serde_json::json!({
            "chatgpt_account_id": "refreshed-account",
            "exp": NOW + 1200,
        }));
        let refreshed_id = token(serde_json::json!({
            "chatgpt_account_id": "refreshed-account",
            "exp": NOW + 2400,
        }));
        let client = fake_token_endpoint(token_response(
            &refreshed_access,
            Some("new-refresh"),
            Some(&refreshed_id),
        ));
        let credential = stored_oauth("old-access", "old-refresh", "old-id", "old-account", NOW);
        let refreshed = refresh_with(&store, credential, &client, NOW)
            .await
            .unwrap();
        let (access, refresh, id, account_id, expires_at) = oauth_credential(&refreshed);
        assert_eq!(access, refreshed_access);
        assert_eq!(refresh, "new-refresh");
        assert_eq!(id, refreshed_id);
        assert_eq!(account_id, "refreshed-account");
        assert_eq!(expires_at, NOW + 1200);
        let _ = std::fs::remove_file(&store.path);
    }

    #[tokio::test]
    async fn refresh_preserves_omitted_refresh_and_id_tokens() {
        let store = temp_store();
        let refreshed_access = token(serde_json::json!({
            "chatgpt_account_id": "account",
            "exp": NOW + 1200,
        }));
        let client = fake_token_endpoint(token_response(&refreshed_access, None, None));
        let credential = stored_oauth("old-access", "old-refresh", "old-id", "account", NOW);
        let refreshed = refresh_with(&store, credential, &client, NOW)
            .await
            .unwrap();
        let (_, refresh, id, _, _) = oauth_credential(&refreshed);
        assert_eq!(refresh, "old-refresh");
        assert_eq!(id, "old-id");
        let _ = std::fs::remove_file(&store.path);
    }

    #[tokio::test]
    async fn refresh_selects_access_account_id_when_id_token_lacks_one() {
        let store = temp_store();
        let refreshed_access = token(serde_json::json!({
            "chatgpt_account_id": "access-account",
            "exp": NOW + 1200,
        }));
        let refreshed_id = token(serde_json::json!({ "exp": NOW + 2400 }));
        let client =
            fake_token_endpoint(token_response(&refreshed_access, None, Some(&refreshed_id)));
        let credential = stored_oauth("old-access", "old-refresh", "old-id", "old-account", NOW);
        let refreshed = refresh_with(&store, credential, &client, NOW)
            .await
            .unwrap();
        let (_, _, _, account_id, _) = oauth_credential(&refreshed);
        assert_eq!(account_id, "access-account");
        let _ = std::fs::remove_file(&store.path);
    }

    #[tokio::test]
    async fn refresh_uses_one_hour_fallback_for_missing_or_invalid_expiry() {
        let store = temp_store();
        let refreshed_access = token(serde_json::json!({
            "chatgpt_account_id": "account",
        }));
        let client = fake_token_endpoint(token_response(&refreshed_access, None, None));
        let credential = stored_oauth("old-access", "old-refresh", "old-id", "account", NOW);
        let refreshed = refresh_with(&store, credential, &client, NOW)
            .await
            .unwrap();
        let (_, _, _, _, expires_at) = oauth_credential(&refreshed);
        assert_eq!(expires_at, NOW + 3600);

        let refreshed_access = token(serde_json::json!({
            "chatgpt_account_id": "account",
            "exp": "not-a-number",
        }));
        let client = fake_token_endpoint(token_response(&refreshed_access, None, None));
        let credential = stored_oauth("old-access", "old-refresh", "old-id", "account", NOW);
        let refreshed = refresh_with(&store, credential, &client, NOW)
            .await
            .unwrap();
        let (_, _, _, _, expires_at) = oauth_credential(&refreshed);
        assert_eq!(expires_at, NOW + 3600);
        let _ = std::fs::remove_file(&store.path);
    }

    #[tokio::test]
    async fn refresh_failure_does_not_save_a_partial_credential() {
        let store = temp_store();
        let failing: &'static str = "fixture token endpoint failure";
        let client = move |_request: HttpRequest| async move {
            Err::<HttpResponse, std::io::Error>(std::io::Error::other(failing))
        };
        let credential = stored_oauth("old-access", "old-refresh", "old-id", "account", NOW);
        let result = refresh_with(&store, credential, &client, NOW).await;
        assert!(result.is_err());
        assert!(!store.path.exists());
    }
}
