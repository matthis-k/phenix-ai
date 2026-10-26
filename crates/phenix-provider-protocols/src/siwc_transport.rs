//! Reusable provider-owned OAuth2/OIDC HTTP transport.
//!
//! The public methods contact only the pinned SIWC token and issuer-JWKS
//! endpoints. Authentication is granted *after* signed OIDC verification,
//! granted-scope validation and exact-account credential persistence.
//! HTTP errors never expose token response bodies or credential values.
use crate::siwc::TOKEN_URL;
use crate::siwc_verified::{TokenEnvelope, JWKS_URL, MAX_JWKS_BODY, MAX_TOKEN_BODY};
use curl::easy::{Easy2, Handler, HttpVersion, IpResolve, List, WriteError};
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const ISSUER_CONFIGURATION_URL: &str = "https://auth.openai.com/.well-known/openid-configuration";
const MAX_ISSUER_CONFIGURATION: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    InvalidForm,
    Unavailable,
    Rejected,
    TooLarge,
    InvalidTokenResponse,
    /// Failure categories are sourced only from libcurl's numeric CURLcode;
    /// never display its error buffer (which can contain URL or proxy details).
    Dns,
    Proxy,
    Connect,
    Transfer,
    Network,
    Timeout,
    Tls,
    /// Status only, never the token endpoint's response body or URL.
    HttpStatus(u16),
}

#[derive(Default)]
struct BoundedBody {
    bytes: Vec<u8>,
    bound: usize,
    too_large: bool,
}
// Protect partial token responses on rejected, interrupted and oversized
// exchanges, not just on successful token parsing.
impl Drop for BoundedBody {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl Handler for BoundedBody {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        if self
            .bytes
            .len()
            .checked_add(data.len())
            .is_none_or(|n| n > self.bound)
        {
            self.too_large = true;
            return Ok(0);
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }
}

/// Preserve safe transport failure categories without passing curl's error
/// buffer, token response, authorization code or request URL to the client.
fn classify_curl_error(problem: &curl::Error) -> TransportError {
    if problem.is_operation_timedout() {
        TransportError::Timeout
    } else if problem.is_peer_failed_verification()
        || problem.is_ssl_connect_error()
        || problem.is_ssl_cacert_badfile()
        || problem.is_ssl_certproblem()
        || problem.is_ssl_issuer_error()
    {
        TransportError::Tls
    } else if problem.is_couldnt_resolve_proxy() {
        TransportError::Proxy
    } else if problem.is_couldnt_resolve_host() {
        TransportError::Dns
    } else if problem.is_couldnt_connect() {
        TransportError::Connect
    } else if problem.is_http2_error()
        || problem.is_http2_stream_error()
        || problem.is_recv_error()
        || problem.is_send_error()
        || problem.is_got_nothing()
    {
        // A connection was attempted and may already have returned headers.
        // Do not misdiagnose a failed HTTP transfer as a DNS/connect failure.
        TransportError::Transfer
    } else {
        TransportError::Network
    }
}

/// This is deliberately an implementation helper, not an arbitrary HTTP
/// request exposed to provider credentials or client-side URLs.
fn request(url: &str, post: Option<&str>, limit: usize) -> Result<Vec<u8>, TransportError> {
    request_with_auth(url, post, limit, None, false)
}

fn request_with_auth(
    url: &str,
    post: Option<&str>,
    limit: usize,
    bearer: Option<&str>,
    allow_empty: bool,
) -> Result<Vec<u8>, TransportError> {
    // OAuth authorization codes are one-shot. A second POST is safe only if
    // libcurl did not resolve/connect, so no token request reached the issuer.
    // Do not retry timeouts, HTTP transfers, TLS failures or any HTTP status:
    // a server might have consumed the code already.
    let result = request_with_auth_family(url, post, limit, bearer, allow_empty, false);
    if matches!(&result, Err(TransportError::Dns | TransportError::Connect)) {
        return request_with_auth_family(url, post, limit, bearer, allow_empty, true);
    }
    result
}

fn request_with_auth_family(
    url: &str,
    post: Option<&str>,
    limit: usize,
    bearer: Option<&str>,
    allow_empty: bool,
    ipv4_only: bool,
) -> Result<Vec<u8>, TransportError> {
    let mut easy = Easy2::new(BoundedBody {
        bytes: Vec::new(),
        bound: limit,
        too_large: false,
    });
    let mut headers = List::new();
    headers
        .append("Accept: application/json")
        .map_err(|_| TransportError::Unavailable)?;
    if post.is_some() {
        headers
            .append("Content-Type: application/x-www-form-urlencoded")
            .map_err(|_| TransportError::Unavailable)?;
    }
    if let Some(token) = bearer {
        if token.is_empty()
            || token.len() > 4096
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b':')
        {
            return Err(TransportError::InvalidForm);
        }
        headers
            .append(&format!("Authorization: Bearer {token}"))
            .map_err(|_| TransportError::Unavailable)?;
    }
    easy.url(url).map_err(|_| TransportError::Unavailable)?;
    easy.http_headers(headers)
        .map_err(|_| TransportError::Unavailable)?;
    easy.follow_location(false)
        .map_err(|_| TransportError::Unavailable)?;
    easy.connect_timeout(Duration::from_secs(5))
        .map_err(|_| TransportError::Unavailable)?;
    easy.timeout(Duration::from_secs(15))
        .map_err(|_| TransportError::Unavailable)?;
    easy.ssl_verify_peer(true)
        .map_err(|_| TransportError::Unavailable)?;
    easy.ssl_verify_host(true)
        .map_err(|_| TransportError::Unavailable)?;
    easy.signal(false)
        .map_err(|_| TransportError::Unavailable)?;
    if ipv4_only {
        // Normal path stays dual-stack, including on IPv6-only hosts.
        // Only a failed pre-connect attempt takes the IPv4 recovery path.
        easy.ip_resolve(IpResolve::V4)
            .map_err(|_| TransportError::Unavailable)?;
    }
    if let Some(form) = post {
        // Token exchange is a short, non-streaming form POST. Pin HTTP/1.1
        // to avoid problematic HTTP/2 framing in embedded libcurl builds.
        // TLS peer and hostname verification remain mandatory.
        easy.http_version(HttpVersion::V11)
            .map_err(|_| TransportError::Unavailable)?;
        easy.post(true).map_err(|_| TransportError::Unavailable)?;
        easy.post_fields_copy(form.as_bytes())
            .map_err(|_| TransportError::Unavailable)?;
    } else {
        easy.get(true).map_err(|_| TransportError::Unavailable)?;
    }
    let outcome = easy.perform();
    let code = easy.response_code().unwrap_or_default();
    if easy.get_ref().too_large {
        return Err(TransportError::TooLarge);
    }
    // Curl can fail before an HTTP response exists, even though the browser
    // sign-in itself succeeded. Preserve that distinction for safe UX.
    outcome.map_err(|problem| classify_curl_error(&problem))?;
    // Never follow redirects or accept error/redirect HTML as OAuth tokens.
    // HTTP status is safe diagnostic metadata; the response body remains secret.
    match code {
        200 => {}
        400 | 401 | 403 => return Err(TransportError::Rejected),
        0 => return Err(TransportError::Network),
        status => return Err(TransportError::HttpStatus(status as u16)),
    }
    if !allow_empty && easy.get_ref().bytes.is_empty() {
        return Err(TransportError::Unavailable);
    }
    Ok(std::mem::take(&mut easy.get_mut().bytes))
}

fn valid_form(form: &str) -> bool {
    // Form values are created by the typed authorization-code/refresh helper,
    // never by the editor. A malicious newline must not become an HTTP header.
    !form.is_empty()
        && form.len() <= 32 * 1024
        && form.is_ascii()
        && !form.bytes().any(|b| b.is_ascii_control() || b == b'#')
        && form.contains("grant_type=")
        && form.contains("client_id=")
        && form.contains("resource=")
}

/// Exchange a state-correlated authorization code or an authorized refresh
/// token at the fixed SIWC token endpoint. No user-provided token URL.
pub fn exchange_token(form: &str) -> Result<TokenEnvelope, TransportError> {
    if !valid_form(form) {
        return Err(TransportError::InvalidForm);
    }
    let mut raw = request(TOKEN_URL, Some(form), MAX_TOKEN_BODY)?;
    let parsed = TokenEnvelope::parse(&raw).map_err(|_| TransportError::InvalidTokenResponse);
    raw.zeroize();
    parsed
}

/// Fetch only the documented ChatGPT-plan catalog, scoped to the already
/// verified provider-owned access token. Never follow redirects or accept an
/// endpoint supplied by a model, callback or user-facing client.
pub fn chatgpt_models(access_token: &str) -> Result<Vec<u8>, TransportError> {
    request_with_auth(
        "https://api.openai.com/v1/models",
        None,
        256 * 1024,
        Some(access_token),
        false,
    )
}

/// Discover the revocation endpoint from OpenAI's pinned issuer metadata.
/// Never use an OAuth callback, a model response or a client-supplied URL.
/// The response must remain on the exact issuer origin with no URL credentials,
/// nonstandard port, query parameters or fragments.
fn parse_revocation_endpoint(body: &[u8]) -> Result<url::Url, TransportError> {
    if body.is_empty() || body.len() > MAX_ISSUER_CONFIGURATION {
        return Err(TransportError::Unavailable);
    }
    let config: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| TransportError::Unavailable)?;
    if config.get("issuer").and_then(serde_json::Value::as_str) != Some("https://auth.openai.com") {
        return Err(TransportError::Unavailable);
    }
    let endpoint = config
        .get("revocation_endpoint")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .ok_or(TransportError::Unavailable)?;
    let url = url::Url::parse(endpoint).map_err(|_| TransportError::Unavailable)?;
    if url.scheme() != "https"
        || url.host_str() != Some("auth.openai.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() == "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(TransportError::Unavailable);
    }
    Ok(url)
}

/// Provider-only renewable-session revocation transport.
///
/// This performs no local credential mutation. The provider must first stop
/// model requests and then decide whether to clear tokens even when remote
/// revocation cannot be confirmed. An empty HTTP 200 is success per SIWC.
/// The refresh token never appears in the URL, errors or the returned value.
pub fn revoke_renewable_session(
    refresh_token: &str,
    issued_client_id: &str,
) -> Result<(), TransportError> {
    if refresh_token.is_empty()
        || refresh_token.len() > 16 * 1024
        || !refresh_token.bytes().all(|byte| byte.is_ascii_graphic())
        || issued_client_id.is_empty()
        || issued_client_id == "dynamic_agent_client"
        || issued_client_id.len() > 256
        || !issued_client_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(TransportError::InvalidForm);
    }

    let discovery = request(ISSUER_CONFIGURATION_URL, None, MAX_ISSUER_CONFIGURATION)?;
    let endpoint = parse_revocation_endpoint(&discovery)?;
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("token", refresh_token)
        .append_pair("token_type_hint", "refresh_token")
        .append_pair("client_id", issued_client_id);
    let form = Zeroizing::new(form.finish());
    let response = request_with_auth(endpoint.as_str(), Some(&form), 4096, None, true)?;
    // SIWC defines an empty 200 for both a revoked and already-invalid token.
    // A nonempty response is not a confirmed remote sign-out.
    if !response.is_empty() {
        return Err(TransportError::Unavailable);
    }
    Ok(())
}

/// Fetch the issuer's fixed JWKS; caller then verifies a signed token with the
/// issuer/audience/nonce, without accepting a discovery URL from JWT claims.
pub fn issuer_jwks() -> Result<Vec<u8>, TransportError> {
    request(JWKS_URL, None, MAX_JWKS_BODY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{Ipv4Addr, SocketAddrV4, TcpListener},
        thread,
    };

    fn local(
        response: &'static str,
        post: Option<&str>,
        limit: usize,
    ) -> (Result<Vec<u8>, TransportError>, String) {
        local_with_allow_empty(response, post, limit, false)
    }

    fn local_with_allow_empty(
        response: &'static str,
        post: Option<&str>,
        limit: usize,
        allow_empty: bool,
    ) -> (Result<Vec<u8>, TransportError>, String) {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                bytes.extend_from_slice(&buf[..n]);
                let head_end = bytes.windows(4).position(|x| x == b"\r\n\r\n");
                if let Some(end) = head_end {
                    let header = std::str::from_utf8(&bytes[..end]).unwrap();
                    let size = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or_default();
                    if bytes.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        let outcome = request_with_auth(&address, post, limit, None, allow_empty);
        (outcome, server.join().unwrap())
    }

    #[test]
    fn recovery_rerun_only_matches_preconnect_failures() {
        let recover =
            |error: TransportError| matches!(error, TransportError::Dns | TransportError::Connect);
        for failure in [TransportError::Dns, TransportError::Connect] {
            assert!(recover(failure));
        }
        for failure in [
            TransportError::Proxy,
            TransportError::Transfer,
            TransportError::Network,
            TransportError::Timeout,
            TransportError::Tls,
            TransportError::Rejected,
            TransportError::HttpStatus(502),
            TransportError::InvalidTokenResponse,
        ] {
            assert!(!recover(failure));
        }
    }

    #[test]
    fn curl_failures_are_classified_without_exposing_error_buffers() {
        // libcurl CURLcode values are stable: proxy DNS=5, host DNS=6,
        // TCP connect=7, HTTP/2=16, timeout=28, TLS=35, receive=56,
        // HTTP/2 stream=92. The application never forwards libcurl's
        // verbose error buffer, which may contain private proxy URLs.
        for (code, expected) in [
            (5, TransportError::Proxy),
            (6, TransportError::Dns),
            (7, TransportError::Connect),
            (16, TransportError::Transfer),
            (28, TransportError::Timeout),
            (35, TransportError::Tls),
            (56, TransportError::Transfer),
            (92, TransportError::Transfer),
        ] {
            let mut failure = curl::Error::new(code);
            failure.set_extra("private proxy URL and code".into());
            assert_eq!(classify_curl_error(&failure), expected);
        }
    }

    #[test]
    fn rejects_redirect_rejection_and_oversized_response_without_exposure() {
        let (redirect, received) = local(
            "HTTP/1.1 302 Found\r\nLocation: https://attacker.invalid\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            None, MAX_JWKS_BODY);
        assert_eq!(redirect, Err(TransportError::HttpStatus(302)));
        assert!(received.starts_with("GET / HTTP/1.1"));

        let (rejected, _) = local(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret",
            Some("grant_type=authorization_code&client_id=issued&resource=valid"),
            MAX_TOKEN_BODY,
        );
        assert_eq!(rejected, Err(TransportError::Rejected));

        let (too_large, _) = local(
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n123456",
            None,
            3,
        );
        assert_eq!(too_large, Err(TransportError::TooLarge));
    }

    #[test]
    fn oauth_post_reports_status_without_exposing_error_bodies() {
        let form = "grant_type=authorization_code&client_id=issued&resource=valid&code=private";
        for (code, expected) in [
            (302, TransportError::HttpStatus(302)),
            (404, TransportError::HttpStatus(404)),
            (429, TransportError::HttpStatus(429)),
            (500, TransportError::HttpStatus(500)),
            (503, TransportError::HttpStatus(503)),
        ] {
            let response = format!(
                "HTTP/1.1 {code} Failure\r\nContent-Length: 16\r\nConnection: close\r\n\r\nprivate-error-42"
            );
            let response: &'static str = Box::leak(response.into_boxed_str());
            let (result, request) = local(response, Some(form), MAX_TOKEN_BODY);
            assert_eq!(result, Err(expected));
            assert!(request.starts_with("POST / HTTP/1.1"));
            assert!(request.ends_with(form));
        }
    }

    #[test]
    fn issuer_discovery_accepts_only_exact_openai_revocation_origin() {
        let ok = br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://auth.openai.com/api/accounts/oauth/revoke"}"#;
        assert_eq!(
            parse_revocation_endpoint(ok).unwrap().as_str(),
            "https://auth.openai.com/api/accounts/oauth/revoke"
        );
        for data in [
            br#"{"issuer":"https://evil.invalid","revocation_endpoint":"https://auth.openai.com/api/accounts/oauth/revoke"}"#.as_slice(),
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"http://auth.openai.com/revoke"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://evil.invalid/revoke"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://auth.openai.com.evil.invalid/revoke"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://user@auth.openai.com/revoke"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://auth.openai.com:1234/revoke"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://auth.openai.com/revoke?token=leak"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://auth.openai.com/revoke#token"}"#,
            br#"{"issuer":"https://auth.openai.com","revocation_endpoint":"https://auth.openai.com/"}"#,
            br#"{"issuer":"https://auth.openai.com"}"#,
            b"not json",
        ] {
            assert_eq!(parse_revocation_endpoint(data), Err(TransportError::Unavailable));
        }
        assert_eq!(
            parse_revocation_endpoint(&vec![b'x'; MAX_ISSUER_CONFIGURATION + 1]),
            Err(TransportError::Unavailable)
        );
    }

    #[test]
    fn empty_success_is_only_accepted_for_revocation_and_invalid_forms_fail() {
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (normal, _) = local(response, None, 4096);
        assert_eq!(normal, Err(TransportError::Unavailable));
        let revocation_form = "token=secret&token_type_hint=refresh_token&client_id=issued";
        let (revoked, received) =
            local_with_allow_empty(response, Some(revocation_form), 4096, true);
        assert_eq!(revoked, Ok(Vec::new()));
        assert!(received.starts_with("POST / HTTP/1.1"));
        assert!(received.ends_with(revocation_form));
        for (token, client) in [
            ("", "issued-client"),
            ("bad\nheader", "issued-client"),
            ("valid-token", "dynamic_agent_client"),
            ("valid-token", "bad client"),
        ] {
            assert_eq!(
                revoke_renewable_session(token, client),
                Err(TransportError::InvalidForm)
            );
        }
    }

    #[test]
    fn bearer_catalog_rejects_control_characters_before_network() {
        for token in ["", "bad\nheader", "bad:extra", "x\r", "a b"] {
            assert_eq!(chatgpt_models(token), Err(TransportError::InvalidForm),);
        }
    }

    #[test]
    fn form_encoded_post_is_not_a_query_and_requires_token_verification() {
        let body = r#"{"token_type":"Bearer","access_token":"fixture-a","refresh_token":"fixture-r","id_token":"unverified","expires_in":3600,"scope":"openid"}"#;
        let http = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body);
        let response: &'static str = Box::leak(http.into_boxed_str());
        let form = "grant_type=authorization_code&client_id=issued&resource=https%3A%2F%2Fapi.openai.com%2Fv1&code=private";
        let (raw, request) = local(response, Some(form), MAX_TOKEN_BODY);
        assert!(request.starts_with("POST / HTTP/1.1"));
        assert!(request.ends_with(form));
        let token = TokenEnvelope::parse(&raw.unwrap()).unwrap();
        assert_eq!(token.granted_scope, "openid"); // token still unverified
        assert!(matches!(
            exchange_token("grant_type=code\nAuthorization: evil"),
            Err(TransportError::InvalidForm)
        ));
    }
}
