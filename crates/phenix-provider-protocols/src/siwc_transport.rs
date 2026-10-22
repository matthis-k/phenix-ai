//! Reusable provider-owned OAuth2/OIDC HTTP transport.
//!
//! The public methods contact only the pinned SIWC token and issuer-JWKS
//! endpoints. Authentication is granted *after* signed OIDC verification,
//! granted-scope validation and exact-account credential persistence.
//! HTTP errors never expose token response bodies or credential values.
use crate::siwc::TOKEN_URL;
use crate::siwc_verified::{TokenEnvelope, JWKS_URL, MAX_JWKS_BODY, MAX_TOKEN_BODY};
use curl::easy::{Easy2, Handler, List, WriteError};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    InvalidForm,
    Unavailable,
    Rejected,
    TooLarge,
    InvalidTokenResponse,
}

#[derive(Default)]
struct BoundedBody {
    bytes: Vec<u8>,
    bound: usize,
    too_large: bool,
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

/// This is deliberately an implementation helper, not an arbitrary HTTP
/// request exposed to provider credentials or client-side URLs.
fn request(url: &str, post: Option<&str>, limit: usize) -> Result<Vec<u8>, TransportError> {
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
    if let Some(form) = post {
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
    // Do not follow redirects or accept error/redirect HTML as token/JWKS.
    // Deliberately do not include the response body or URL in errors.
    match code {
        200 => {}
        400 | 401 | 403 => return Err(TransportError::Rejected),
        _ => return Err(TransportError::Unavailable),
    }
    outcome.map_err(|_| TransportError::Unavailable)?;
    if easy.get_ref().bytes.is_empty() {
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
    raw.fill(0);
    parsed
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
        let outcome = request(&address, post, limit);
        (outcome, server.join().unwrap())
    }

    #[test]
    fn rejects_redirect_rejection_and_oversized_response_without_exposure() {
        let (redirect, received) = local(
            "HTTP/1.1 302 Found\r\nLocation: https://attacker.invalid\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            None, MAX_JWKS_BODY);
        assert_eq!(redirect, Err(TransportError::Unavailable));
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
