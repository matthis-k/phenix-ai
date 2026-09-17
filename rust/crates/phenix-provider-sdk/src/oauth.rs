use crate::{auth::OAuthMethod, Auth, AuthKind, CredentialStore, ProviderError, Secret, Token};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const REFRESH_MARGIN_SECONDS: u64 = 5 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OAuthExternal {
    pub uri: String,
    pub instructions: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

pub(crate) fn begin(
    method: &OAuthMethod,
    provider: &str,
    store: CredentialStore,
) -> Result<OAuthExternal, ProviderError> {
    let listener = bind_callback(method)?;
    let port = listener
        .local_addr()
        .map_err(|error| oauth_error(format!("cannot inspect OAuth callback address: {error}")))?
        .port();
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let state = CsrfToken::new_random();
    let uri = authorization_url(method, &redirect_uri, challenge.as_str(), state.secret())?;

    let method = method.clone();
    let provider = provider.to_owned();
    let expected_state = state.secret().to_owned();
    thread::Builder::new()
        .name(format!("phenix-oauth-{provider}"))
        .spawn(move || {
            let result = complete_login(
                listener,
                &method,
                &provider,
                &store,
                &redirect_uri,
                verifier,
                &expected_state,
            );
            if let Err(error) = result {
                eprintln!("Phenix OAuth authentication for {provider} failed: {error}");
            }
        })
        .map_err(|error| oauth_error(format!("cannot start OAuth callback worker: {error}")))?;

    Ok(OAuthExternal {
        uri,
        instructions: Some(
            "Complete authorization in the browser. Phenix will accept the verified localhost callback and store the credential automatically."
                .to_owned(),
        ),
    })
}

pub(crate) fn refresh_if_needed(
    method: &OAuthMethod,
    provider: &str,
    store: &CredentialStore,
    auth: Auth,
) -> Result<Auth, ProviderError> {
    let Auth::OAuth {
        access_token,
        refresh_token,
        expires_at,
    } = auth
    else {
        return Ok(auth);
    };
    let now = unix_time()?;
    if expires_at.is_none_or(|expires_at| expires_at > now.saturating_add(REFRESH_MARGIN_SECONDS)) {
        return Ok(Auth::OAuth {
            access_token,
            refresh_token,
            expires_at,
        });
    }
    let Some(refresh_token) = refresh_token else {
        return Err(oauth_error(format!(
            "OAuth credential for {provider} is expiring and has no refresh token"
        )));
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| oauth_error(format!("cannot start OAuth refresh runtime: {error}")))?;
    let response = runtime.block_on(post_token_form(
        &method.token_url,
        &[
            ("grant_type", "refresh_token".to_owned()),
            ("client_id", method.client_id.clone()),
            ("refresh_token", refresh_token.expose().to_owned()),
        ],
    ))?;
    let access_token = Token::parse(response.access_token)
        .map_err(|error| oauth_error(format!("invalid OAuth access token: {error}")))?;
    let refresh_token = response
        .refresh_token
        .map(Secret::parse)
        .transpose()
        .map_err(|error| oauth_error(format!("invalid OAuth refresh token: {error}")))?
        .or(Some(refresh_token));
    let expires_at = token_expiry(access_token.expose())
        .or_else(|| response.expires_in.map(|seconds| now.saturating_add(seconds)));
    let refreshed = Auth::OAuth {
        access_token,
        refresh_token,
        expires_at,
    };
    replace_oauth(store, provider, refreshed.clone())?;
    Ok(refreshed)
}

pub(crate) fn apply_headers(
    method: &OAuthMethod,
    access_token: &Token,
    headers: &mut BTreeMap<String, String>,
) -> Result<(), ProviderError> {
    headers.insert(
        "authorization".to_owned(),
        format!("Bearer {}", access_token.expose()),
    );
    headers.extend(method.static_headers.clone());
    if let Some(header) = &method.account_id_header {
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
    Ok(())
}

fn complete_login(
    listener: TcpListener,
    method: &OAuthMethod,
    provider: &str,
    store: &CredentialStore,
    redirect_uri: &str,
    verifier: PkceCodeVerifier,
    expected_state: &str,
) -> Result<(), ProviderError> {
    listener
        .set_nonblocking(false)
        .map_err(|error| oauth_error(format!("cannot configure OAuth callback listener: {error}")))?;
    let (mut stream, _) = listener
        .accept()
        .map_err(|error| oauth_error(format!("OAuth callback failed: {error}")))?;
    stream
        .set_read_timeout(Some(LOGIN_TIMEOUT))
        .map_err(|error| oauth_error(format!("cannot configure OAuth callback timeout: {error}")))?;
    let code = receive_callback(&mut stream, expected_state)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| oauth_error(format!("cannot start OAuth token runtime: {error}")))?;
    let response = runtime.block_on(post_token_form(
        &method.token_url,
        &[
            ("grant_type", "authorization_code".to_owned()),
            ("code", code),
            ("redirect_uri", redirect_uri.to_owned()),
            ("client_id", method.client_id.clone()),
            ("code_verifier", verifier.secret().to_owned()),
        ],
    ))?;
    let now = unix_time()?;
    let access_token = Token::parse(response.access_token)
        .map_err(|error| oauth_error(format!("invalid OAuth access token: {error}")))?;
    let refresh_token = response
        .refresh_token
        .map(Secret::parse)
        .transpose()
        .map_err(|error| oauth_error(format!("invalid OAuth refresh token: {error}")))?;
    let expires_at = token_expiry(access_token.expose())
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
    replace_oauth(
        store,
        provider,
        Auth::OAuth {
            access_token,
            refresh_token,
            expires_at,
        },
    )?;
    Ok(())
}

fn bind_callback(method: &OAuthMethod) -> Result<TcpListener, ProviderError> {
    let mut last_error = None;
    for port in &method.callback_ports {
        match TcpListener::bind(("127.0.0.1", *port)) {
            Ok(listener) => return Ok(listener),
            Err(error) => last_error = Some(error),
        }
    }
    Err(oauth_error(format!(
        "cannot bind OAuth callback on configured ports {:?}: {}",
        method.callback_ports,
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no callback ports configured".to_owned())
    )))
}

fn authorization_url(
    method: &OAuthMethod,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> Result<String, ProviderError> {
    let mut url = Url::parse(&method.authorization_url)
        .map_err(|error| oauth_error(format!("invalid OAuth authorization URL: {error}")))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &method.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state);
        if !method.scopes.is_empty() {
            query.append_pair("scope", &method.scopes.join(" "));
        }
        for (name, value) in &method.authorization_params {
            query.append_pair(name, value);
        }
    }
    Ok(url.into())
}

fn receive_callback(stream: &mut TcpStream, expected_state: &str) -> Result<String, ProviderError> {
    let mut request = vec![0_u8; 16 * 1024];
    let length = stream
        .read(&mut request)
        .map_err(|error| oauth_error(format!("cannot read OAuth callback: {error}")))?;
    let request = std::str::from_utf8(&request[..length])
        .map_err(|_| oauth_error("OAuth callback was not valid HTTP"))?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| oauth_error("OAuth callback did not contain a request target"))?;
    let url = Url::parse(&format!("http://localhost{target}"))
        .map_err(|error| oauth_error(format!("invalid OAuth callback URL: {error}")))?;
    let query = url.query_pairs().collect::<BTreeMap<_, _>>();
    let result = if let Some(error) = query.get("error") {
        Err(oauth_error(format!("OAuth authorization was rejected: {error}")))
    } else if query.get("state").map(|value| value.as_ref()) != Some(expected_state) {
        Err(oauth_error("OAuth callback state verification failed"))
    } else {
        query
            .get("code")
            .map(|value| value.to_string())
            .ok_or_else(|| oauth_error("OAuth callback did not contain an authorization code"))
    };
    let (status, body) = if result.is_ok() {
        (
            "200 OK",
            "Phenix authentication completed. You may close this tab and return to Neovim.",
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
    let _ = stream.write_all(response.as_bytes());
    result
}

async fn post_token_form(
    token_url: &str,
    form: &[("static str", String)],
) -> Result<TokenResponse, ProviderError> {
    let response = Client::new()
        .post(token_url)
        .form(form)
        .send()
        .await
        .map_err(|error| oauth_error(format!("OAuth token request failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| "unreadable response".to_owned());
        return Err(oauth_error(format!(
            "OAuth token endpoint returned {status}: {message}"
        )));
    }
    response
        .json()
        .await
        .map_err(|error| oauth_error(format!("invalid OAuth token response: {error}")))
}

fn replace_oauth(store: &CredentialStore, provider: &str, auth: Auth) -> Result<(), ProviderError> {
    let _ = store.remove(provider, AuthKind::OAuth)?;
    store.add(provider, auth)?;
    Ok(())
}

fn jwt_string_claim(token: &str, path: &[String]) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let mut value: &Value = &serde_json::from_slice::<Value>(&bytes).ok()?;
    for segment in path {
        value = value.get(segment)?;
    }
    value.as_str().map(ToOwned::to_owned)
}

fn token_expiry(token: &str) -> Option<u64> {
    jwt_string_number_claim(token, &["exp"])
}

fn jwt_string_number_claim(token: &str, path: &[&str]) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    let mut value = &json;
    for segment in path {
        value = value.get(*segment)?;
    }
    value.as_u64()
}

fn unix_time() -> Result<u64, ProviderError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| oauth_error(format!("system clock predates Unix epoch: {error}")))
}

fn oauth_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Authentication {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_url_contains_pkce_state_and_provider_policy() {
        let method = OAuthMethod::authorization_code(
            "https://example.test/oauth/authorize",
            "https://example.test/oauth/token",
            "client",
        )
        .unwrap()
        .with_scope("openid")
        .with_scope("offline_access")
        .with_authorization_param("originator", "phenix");
        let url = authorization_url(&method, "http://localhost:1455/auth/callback", "challenge", "state")
            .unwrap();
        let url = Url::parse(&url).unwrap();
        let query = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(query.get("client_id").map(|value| value.as_ref()), Some("client"));
        assert_eq!(query.get("code_challenge").map(|value| value.as_ref()), Some("challenge"));
        assert_eq!(query.get("state").map(|value| value.as_ref()), Some("state"));
        assert_eq!(query.get("scope").map(|value| value.as_ref()), Some("openid offline_access"));
        assert_eq!(query.get("originator").map(|value| value.as_ref()), Some("phenix"));
    }
}
