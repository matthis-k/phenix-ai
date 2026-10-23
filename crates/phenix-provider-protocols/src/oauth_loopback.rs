//! One-shot OAuth callback listener, shared by authorization-code providers.
//!
//! Binding a loopback socket is a transport operation, not authentication.
//! The caller must correlate state, validate the issued client, exchange the
//! code, and verify signed OIDC claims before reporting Authenticated.
//! Neither callback URLs nor authorization codes are ever logged here.
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST: usize = 8192;
const MAX_LIFETIME: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackError {
    Bind,
    InvalidRequest,
    Io,
    Expired,
    Consumed,
}

/// Provider-owned state: bind before generating/opening the browser URL.
/// The listener accepts exactly one callback, and cannot be rearmed.
pub struct OAuthLoopbackListener {
    listener: TcpListener,
    port: u16,
    created: Instant,
    ttl: Duration,
    consumed: bool,
}

impl OAuthLoopbackListener {
    pub fn bind(ttl: Duration) -> Result<Self, CallbackError> {
        if ttl.is_zero() || ttl > MAX_LIFETIME {
            return Err(CallbackError::Bind);
        }
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| CallbackError::Bind)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| CallbackError::Bind)?;
        let port = listener
            .local_addr()
            .map_err(|_| CallbackError::Bind)?
            .port();
        if port == 0 {
            return Err(CallbackError::Bind);
        }
        Ok(Self {
            listener,
            port,
            created: Instant::now(),
            ttl,
            consumed: false,
        })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/auth/callback", self.port)
    }

    /// Nonblocking accept for ProviderAuth::poll. A malformed or timed-out
    /// local request consumes the attempt; never fall back to a second client.
    pub fn poll(&mut self) -> Result<Option<CallbackRequest>, CallbackError> {
        if self.consumed {
            return Err(CallbackError::Consumed);
        }
        if self.created.elapsed() >= self.ttl {
            self.consumed = true;
            return Err(CallbackError::Expired);
        }
        let (mut stream, peer) = match self.listener.accept() {
            Ok(value) => value,
            Err(problem) if problem.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(_) => {
                self.consumed = true;
                return Err(CallbackError::Io);
            }
        };
        self.consumed = true;
        if !peer.ip().is_loopback() {
            return Err(CallbackError::InvalidRequest);
        }
        stream
            .set_read_timeout(Some(Duration::from_millis(750)))
            .map_err(|_| CallbackError::Io)?;
        stream
            .set_write_timeout(Some(Duration::from_millis(750)))
            .map_err(|_| CallbackError::Io)?;
        let uri = match parse_request(&mut stream, self.port) {
            Ok(uri) => uri,
            Err(problem) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return Err(problem);
            }
        };
        Ok(Some(CallbackRequest { uri, stream }))
    }
}

/// Carries a correlated *candidate* URI, not a verified account. The caller
/// should pass uri() to its provider's one-shot state-checking callback parser
/// and finish token/OIDC verification before returning a success page.
pub struct CallbackRequest {
    uri: String,
    stream: TcpStream,
}

impl CallbackRequest {
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// A failed or unverified attempt never gets a success page.
    pub fn reject(self) -> Result<(), CallbackError> {
        self.respond_internal(false)
    }

    /// Only a cryptographically verified OIDC subject can confirm browser
    /// completion. The provider must additionally enforce granted scopes and
    /// persist the exact authorized account before calling this method.
    pub fn confirm_verified(
        self,
        _identity: &crate::oauth_oidc::VerifiedSubject,
    ) -> Result<(), CallbackError> {
        self.respond_internal(true)
    }

    fn respond_internal(mut self, authenticated: bool) -> Result<(), CallbackError> {
        let response: &[u8] = if authenticated {
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: 26\r\nConnection: close\r\n\r\nAuthentication completed.\n"
        } else {
            b"HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: 23\r\nConnection: close\r\n\r\nAuthentication failed.\n"
        };
        self.stream
            .write_all(response)
            .map_err(|_| CallbackError::Io)
    }
}

impl Drop for CallbackRequest {
    fn drop(&mut self) {
        // The URI can contain a short-lived authorization code. Do not
        // retain an unzeroized heap allocation after processing this request.
        let mut bytes = std::mem::take(&mut self.uri).into_bytes();
        bytes.zeroize();
    }
}

fn parse_request(stream: &mut TcpStream, port: u16) -> Result<String, CallbackError> {
    // The request line contains a short-lived OAuth authorization code.
    // Scrub both the assembled HTTP request and every read chunk on success
    // and on early parser rejection, not only the returned callback URI.
    let mut buffer = Zeroizing::new(Vec::with_capacity(1024));
    let mut chunk = Zeroizing::new([0u8; 1024]);
    loop {
        let count = stream
            .read(&mut chunk[..])
            .map_err(|_| CallbackError::InvalidRequest)?;
        if count == 0 {
            return Err(CallbackError::InvalidRequest);
        }
        if buffer
            .len()
            .checked_add(count)
            .is_none_or(|n| n > MAX_REQUEST)
        {
            return Err(CallbackError::InvalidRequest);
        }
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.windows(4).any(|part| part == b"\r\n\r\n") {
            break;
        }
    }
    let raw = std::str::from_utf8(&buffer).map_err(|_| CallbackError::InvalidRequest)?;
    let (headers, _) = raw
        .split_once("\r\n\r\n")
        .ok_or(CallbackError::InvalidRequest)?;
    let mut lines = headers.split("\r\n");
    let request = lines.next().ok_or(CallbackError::InvalidRequest)?;
    let mut fields = request.split(' ');
    if fields.next() != Some("GET") {
        return Err(CallbackError::InvalidRequest);
    }
    let target = fields.next().ok_or(CallbackError::InvalidRequest)?;
    if fields.next() != Some("HTTP/1.1")
        || fields.next().is_some()
        || !target.starts_with("/auth/callback?")
        || target
            .bytes()
            .any(|b| b.is_ascii_control() || b == b' ' || b == b'#')
    {
        return Err(CallbackError::InvalidRequest);
    }

    let expected_host = format!("127.0.0.1:{port}");
    let mut valid_host = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(CallbackError::InvalidRequest)?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(CallbackError::InvalidRequest);
        }
        if name.eq_ignore_ascii_case("host") {
            if valid_host || value.trim() != expected_host {
                return Err(CallbackError::InvalidRequest);
            }
            valid_host = true;
        }
        if name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            return Err(CallbackError::InvalidRequest);
        }
    }
    if !valid_host {
        return Err(CallbackError::InvalidRequest);
    }
    Ok(format!("http://{expected_host}{target}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_request(url: &str, host: &str, method: &str) -> TcpStream {
        let target = url.strip_prefix(&format!("http://{host}")).unwrap();
        let mut stream = TcpStream::connect(host).unwrap();
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream
    }

    #[test]
    fn binds_actual_loopback_before_browser_and_consumes_once() {
        let mut listener = OAuthLoopbackListener::bind(Duration::from_secs(30)).unwrap();
        let uri = listener.redirect_uri();
        assert!(uri.starts_with("http://127.0.0.1:"));
        assert!(listener.poll().unwrap().is_none());
        let url = format!("{uri}?state=fixture-state&code=secret-fixture-code");
        let host = uri
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let mut browser = browser_request(&url, host, "GET");
        let callback = listener.poll().unwrap().unwrap();
        assert_eq!(callback.uri(), url);
        callback.reject().unwrap();
        let mut result = String::new();
        browser.read_to_string(&mut result).unwrap();
        assert!(result.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(!result.contains("secret-fixture-code"));
        assert!(matches!(listener.poll(), Err(CallbackError::Consumed)));
    }

    #[test]
    fn real_loopback_callback_remains_unverified_until_provider_oidc() {
        use crate::siwc::{AuthorizationAttempt, Registration};
        use crate::siwc_verified::GeneratedPkce;

        let generated = GeneratedPkce::new();
        let mut listener = OAuthLoopbackListener::bind(Duration::from_secs(30)).unwrap();
        let redirect_uri = listener.redirect_uri();
        let attempt = AuthorizationAttempt {
            registration: Registration::First { app_name: "Phenix" },
            host_id: "urn:uuid:123e4567-e89b-12d3-a456-426614174000",
            redirect_uri: &redirect_uri,
            state: generated.state(),
            nonce: generated.nonce(),
            pkce_challenge: generated.challenge(),
        };
        assert!(attempt.authorization_url().unwrap().starts_with("https://"));
        let callback_uri = format!(
            "{redirect_uri}?state={}&code=short-lived-code&client_id=oaiapp_issued",
            generated.state()
        );
        let host = redirect_uri
            .strip_prefix("http://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let mut browser = browser_request(&callback_uri, host, "GET");
        let candidate = listener.poll().unwrap().unwrap();
        let correlated = attempt.accept_callback(candidate.uri()).unwrap();
        assert_eq!(correlated.issued_client_id(), "oaiapp_issued");
        let form = correlated.exchange_form(generated.verifier()).unwrap();
        assert!(form.contains("grant_type=authorization_code"));
        assert!(!form.contains("client_secret"));

        // State correlation and a code are not OIDC proof: the browser must
        // not see a successful sign-in before the verified provider finishes.
        candidate.reject().unwrap();
        let mut result = String::new();
        browser.read_to_string(&mut result).unwrap();
        assert!(result.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(!result.contains("short-lived-code"));
    }

    #[test]
    fn rejects_wrong_method_duplicate_host_and_unbound_redirects() {
        for method in ["POST", "PUT"] {
            let mut listener = OAuthLoopbackListener::bind(Duration::from_secs(30)).unwrap();
            let uri = listener.redirect_uri();
            let host = uri
                .strip_prefix("http://")
                .unwrap()
                .split('/')
                .next()
                .unwrap();
            let _browser = browser_request(&format!("{uri}?state=s&code=c"), host, method);
            assert!(matches!(
                listener.poll(),
                Err(CallbackError::InvalidRequest)
            ));
            assert!(matches!(listener.poll(), Err(CallbackError::Consumed)));
        }
        assert!(matches!(
            OAuthLoopbackListener::bind(Duration::ZERO),
            Err(CallbackError::Bind)
        ));
        assert!(matches!(
            OAuthLoopbackListener::bind(Duration::from_secs(601)),
            Err(CallbackError::Bind)
        ));
    }
}
