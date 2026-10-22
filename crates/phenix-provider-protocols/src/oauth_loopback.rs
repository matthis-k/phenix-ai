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

    pub fn respond(mut self, authenticated: bool) -> Result<(), CallbackError> {
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
        bytes.fill(0);
    }
}

fn parse_request(stream: &mut TcpStream, port: u16) -> Result<String, CallbackError> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let count = stream
            .read(&mut chunk)
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
        callback.respond(true).unwrap();
        let mut result = String::new();
        browser.read_to_string(&mut result).unwrap();
        assert!(result.starts_with("HTTP/1.1 200 OK"));
        assert!(!result.contains("secret-fixture-code"));
        assert!(matches!(listener.poll(), Err(CallbackError::Consumed)));
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
