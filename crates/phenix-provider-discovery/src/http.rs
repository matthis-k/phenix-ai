//! Reusable, bounded read-only model catalog HTTP adapter.
//!
//! Providers select a published, approved URL and supply the credential from
//! their authorized account store. No provider name or model protocol is
//! baked into this mechanism. An HTTP result is never authentication evidence
//! on its own; providers still check exact account authority and parse the
//! selected catalog schema before publishing models.
use curl::easy::{Easy2, Handler, List, WriteError};
use phenix_provider_protocols::{validate_endpoint, EndpointMode};
use std::time::Duration;
use url::Url;

/// Only safe transport categories reach a provider's typed error mapping.
/// Never include bearer values, URLs, HTTP bodies, or curl error strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogFetchError {
    UnsafeEndpoint,
    InvalidBearer,
    Unauthorized,
    RateLimited,
    Rejected,
    Unavailable,
    TooLarge,
}

struct Body {
    bytes: Vec<u8>,
    too_large: bool,
    max_bytes: Option<usize>,
}

impl Handler for Body {
    fn write(&mut self, incoming: &[u8]) -> Result<usize, WriteError> {
        if self.max_bytes.is_some_and(|limit| {
            self.bytes
                .len()
                .checked_add(incoming.len())
                .is_none_or(|total| total > limit)
        }) {
            self.too_large = true;
            return Ok(0);
        }
        self.bytes.extend_from_slice(incoming);
        Ok(incoming.len())
    }
}

fn approved_endpoint(endpoint: &str) -> bool {
    // Reuse the same strict userinfo/origin/scheme validator as provider
    // declarations. In particular, https://@host is not an acceptable
    // credential target even though URL parsing normalizes its username.
    let Ok(checked) = validate_endpoint(endpoint, EndpointMode::LocalLoopback) else {
        return false;
    };
    let Ok(url) = Url::parse(&checked.url) else {
        return false;
    };
    (!checked.is_loopback_http || url.host_str() == Some("127.0.0.1"))
        && url.path() != "/"
        && url.query().is_none()
        && url.fragment().is_none()
}

/// Fetch a catalog from the already selected provider URL, without forwarding
/// redirects or interpreting its format. TLS peer/host checks are mandatory.
/// The provider retains responsibility for credential grants and URL origin.
pub fn fetch_bearer_catalog(endpoint: &str, bearer: &str) -> Result<Vec<u8>, CatalogFetchError> {
    fetch_bearer_catalog_with_limit(endpoint, bearer, None)
}

/// Only an explicit caller policy applies a total HTTP catalog size limit.
/// Catalogs are not truncated: an opted-in limit fails atomically.
pub fn fetch_bearer_catalog_with_limit(
    endpoint: &str,
    bearer: &str,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, CatalogFetchError> {
    if !approved_endpoint(endpoint) {
        return Err(CatalogFetchError::UnsafeEndpoint);
    }
    if bearer.is_empty()
        || bearer.len() > 4096
        || !bearer
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b':')
    {
        return Err(CatalogFetchError::InvalidBearer);
    }
    let mut easy = Easy2::new(Body {
        bytes: Vec::new(),
        too_large: false,
        max_bytes,
    });
    let mut headers = List::new();
    headers
        .append("Accept: application/json")
        .map_err(|_| CatalogFetchError::Unavailable)?;
    headers
        .append(&format!("Authorization: Bearer {bearer}"))
        .map_err(|_| CatalogFetchError::Unavailable)?;
    easy.url(endpoint)
        .and_then(|_| easy.get(true))
        .and_then(|_| easy.follow_location(false))
        .and_then(|_| easy.connect_timeout(Duration::from_secs(4)))
        .and_then(|_| easy.timeout(Duration::from_secs(15)))
        .and_then(|_| easy.ssl_verify_peer(true))
        .and_then(|_| easy.ssl_verify_host(true))
        .and_then(|_| easy.signal(false))
        .and_then(|_| easy.http_headers(headers))
        .map_err(|_| CatalogFetchError::Unavailable)?;
    let outcome = easy.perform();
    let status = easy.response_code().unwrap_or_default();
    if easy.get_ref().too_large {
        return Err(CatalogFetchError::TooLarge);
    }
    match status {
        200 => {}
        401 | 403 => return Err(CatalogFetchError::Unauthorized),
        429 => return Err(CatalogFetchError::RateLimited),
        400..=499 => return Err(CatalogFetchError::Rejected),
        _ => return Err(CatalogFetchError::Unavailable),
    }
    outcome.map_err(|_| CatalogFetchError::Unavailable)?;
    if easy.get_ref().bytes.is_empty() {
        return Err(CatalogFetchError::Unavailable);
    }
    Ok(std::mem::take(&mut easy.get_mut().bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn local(response: &str) -> (Result<Vec<u8>, CatalogFetchError>, String) {
        local_with_limit(response, None)
    }

    fn local_with_limit(
        response: &str,
        max_bytes: Option<usize>,
    ) -> (Result<Vec<u8>, CatalogFetchError>, String) {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://127.0.0.1:{}/models",
            server.local_addr().unwrap().port()
        );
        let owned = response.to_owned();
        let reader = thread::spawn(move || {
            let (mut client, _) = server.accept().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 4096];
            let read = client.read(&mut request).unwrap();
            client.write_all(owned.as_bytes()).unwrap();
            String::from_utf8(request[..read].to_vec()).unwrap()
        });
        let result = fetch_bearer_catalog_with_limit(&endpoint, "fake-test-key", max_bytes);
        (result, reader.join().unwrap())
    }

    #[test]
    fn standard_bearer_catalog_is_bounded_and_does_not_follow_redirects() {
        let body = r#"{"data":[{"id":"gpt-test"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (ok, request) = local(&response);
        assert_eq!(ok.unwrap(), body.as_bytes());
        assert!(request.starts_with("GET /models HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer fake-test-key"));
        let (denied, _) =
            local("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert_eq!(denied, Err(CatalogFetchError::Unauthorized));
        let (limited, _) = local(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(limited, Err(CatalogFetchError::RateLimited));
        let (redirect, _) = local(
            "HTTP/1.1 302 Found\r\nLocation: https://other.invalid/models\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(redirect, Err(CatalogFetchError::Unavailable));
        let huge = "x".repeat(256 * 1024 + 1);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            huge.len(),
            huge
        );
        let (unbounded, _) = local(&response);
        assert_eq!(unbounded.unwrap(), huge.as_bytes());
        let (too_large, _) = local_with_limit(&response, Some(256 * 1024));
        assert_eq!(too_large, Err(CatalogFetchError::TooLarge));
    }

    #[test]
    fn rejects_unapproved_urls_and_unsafe_bearers_before_network() {
        for endpoint in [
            "http://example.org/models",
            "http://localhost:4567/models",
            "https://user:pass@example.com/models",
            "https://@example.com/models",
            "https://example.com/models?key=x",
            "https://example.com/models#fragment",
        ] {
            assert_eq!(
                fetch_bearer_catalog(endpoint, "token"),
                Err(CatalogFetchError::UnsafeEndpoint)
            );
        }
        for token in ["", "bad\nheader", "bad token", "bad:header"] {
            assert_eq!(
                fetch_bearer_catalog("https://example.com/models", token),
                Err(CatalogFetchError::InvalidBearer)
            );
        }
    }
}
