//! Shared provider connection validation. Network authority stays in Phenix.
//!
//! This validates the *configured* URL only. A runtime HTTP transport must
//! still reject redirects and revalidate resolved addresses before dispatch.
//! Do not treat an approved URL as permission to connect or use credentials.

use std::net::IpAddr;
use url::Url;

#[cfg(feature = "siwc-verified")]
pub mod oauth_loopback;
#[cfg(feature = "siwc-verified")]
pub mod oauth_oidc;
/// OpenAI's explicit OSS Sign in with ChatGPT URL/callback boundary.
/// This is protocol data handling, not a ProviderAuth implementation.
pub mod siwc;
#[cfg(feature = "siwc-verified")]
pub mod siwc_flow;
#[cfg(feature = "siwc-verified")]
pub mod siwc_transport;
#[cfg(feature = "siwc-verified")]
pub mod siwc_verified;

#[cfg(feature = "model-codecs")]
pub mod request;
#[cfg(feature = "model-codecs")]
pub mod siwc_models;
#[cfg(feature = "model-codecs")]
pub mod stream;

#[cfg(feature = "model-codecs")]
use phenix_basic_contracts::{error, BasicError, ErrorCode};

#[cfg(feature = "model-codecs")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenaiChat,
    OpenaiResponses,
    AnthropicMessages,
}

#[cfg(feature = "model-codecs")]
impl Protocol {
    pub fn parse(id: &str) -> Result<Self, BasicError> {
        match id {
            "openai-chat" => Ok(Self::OpenaiChat),
            "openai-responses" => Ok(Self::OpenaiResponses),
            "anthropic-messages" => Ok(Self::AnthropicMessages),
            _ => Err(error(
                ErrorCode::InvalidInput,
                "unsupported HTTP model protocol",
            )),
        }
    }
}

#[cfg(feature = "model-codecs")]
fn invalid(message: &str) -> BasicError {
    error(ErrorCode::InvalidReply, message)
}
#[cfg(feature = "model-codecs")]
fn failed(message: &str) -> BasicError {
    error(ErrorCode::ModelFailed, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointProblem {
    Malformed,
    UnsupportedScheme,
    CredentialsInUrl,
    Fragment,
    LocalHttpNotAllowed,
    UnsafeLocalHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointMode {
    /// Remote inference requires HTTPS, even when the caller opted into HTTP.
    Remote,
    /// Explicitly approved local connections can use loopback HTTP.
    LocalLoopback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEndpoint {
    /// Canonical, credential-free URL.
    pub url: String,
    /// Origin identity used to bind credentials and forbid cross-origin hops.
    pub origin: String,
    pub is_loopback_http: bool,
}

/// Validate a configured provider endpoint before any network or credential use.
/// The caller must separately supply an installed, authorized protocol ID.
pub fn validate_endpoint(
    input: &str,
    mode: EndpointMode,
) -> Result<ValidatedEndpoint, EndpointProblem> {
    if input != input.trim() || input.chars().any(char::is_control) {
        return Err(EndpointProblem::Malformed);
    }
    let url = Url::parse(input).map_err(|_| EndpointProblem::Malformed)?;
    if url.fragment().is_some() {
        return Err(EndpointProblem::Fragment);
    }
    if !url.username().is_empty() || url.password().is_some() || explicit_userinfo(input, &url) {
        return Err(EndpointProblem::CredentialsInUrl);
    }
    if url.host().is_none() || url.cannot_be_a_base() {
        return Err(EndpointProblem::Malformed);
    }

    let is_loopback_http = match url.scheme() {
        "https" => false,
        "http" => {
            if mode != EndpointMode::LocalLoopback {
                return Err(EndpointProblem::LocalHttpNotAllowed);
            }
            match url.host() {
                Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
                Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
                _ => false,
            }
        }
        _ => return Err(EndpointProblem::UnsupportedScheme),
    };
    if url.scheme() == "http" && !is_loopback_http {
        return Err(EndpointProblem::UnsafeLocalHost);
    }

    let origin = url.origin().ascii_serialization();
    Ok(ValidatedEndpoint {
        url: url.to_string(),
        origin,
        is_loopback_http,
    })
}

/// `url` removes empty userinfo. Preserve its absence as an explicit rule,
/// so `https://@host/` cannot be normalized into a valid credential target.
fn explicit_userinfo(input: &str, parsed: &Url) -> bool {
    let Some(after_scheme) = input.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let authority = after_scheme
        .split(&['/', '?', '#'][..])
        .next()
        .unwrap_or_default();
    // Only inspect the authority segment, never an @ in a path or query.
    parsed.has_host() && authority.contains('@')
}

/// No request may reuse credentials across origins. Redirects are never
/// transparently followed. DNS resolution still needs transport-side checks.
pub fn same_credential_origin(
    authorized: &ValidatedEndpoint,
    candidate: &ValidatedEndpoint,
) -> bool {
    authorized.origin == candidate.origin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_remote_requires_https() {
        let remote =
            validate_endpoint("https://api.example.test/v1", EndpointMode::Remote).unwrap();
        assert_eq!(remote.origin, "https://api.example.test");
        assert_eq!(remote.url, "https://api.example.test/v1");
        assert!(!remote.is_loopback_http);
        assert_eq!(
            validate_endpoint("http://api.example.test/v1", EndpointMode::Remote),
            Err(EndpointProblem::LocalHttpNotAllowed)
        );
        assert_eq!(
            validate_endpoint("ftp://example.test", EndpointMode::Remote),
            Err(EndpointProblem::UnsupportedScheme)
        );
    }

    #[test]
    fn explicitly_approved_loopback_http_is_literal_ip_only() {
        for name in ["http://127.0.0.1:11434/v1", "http://[::1]:11434/v1"] {
            let endpoint = validate_endpoint(name, EndpointMode::LocalLoopback).unwrap();
            assert!(endpoint.is_loopback_http);
        }
        for name in [
            "http://localhost:11434/v1",
            "http://192.168.1.5:11434/v1",
            "http://127.0.0.2.example.com/v1",
            "http://0.0.0.0:11434/v1",
        ] {
            assert_eq!(
                validate_endpoint(name, EndpointMode::LocalLoopback),
                Err(EndpointProblem::UnsafeLocalHost)
            );
        }
    }

    #[test]
    fn credentials_and_fragments_are_never_accepted() {
        for name in [
            "https://alice:password@example.test/v1",
            "https://@example.test/v1",
            "https://user@example.test/v1",
        ] {
            assert_eq!(
                validate_endpoint(name, EndpointMode::Remote),
                Err(EndpointProblem::CredentialsInUrl)
            );
        }
        assert_eq!(
            validate_endpoint("https://example.test/v1#token", EndpointMode::Remote),
            Err(EndpointProblem::Fragment)
        );
        assert!(
            validate_endpoint("https://example.test/@user?q=@other", EndpointMode::Remote).is_ok()
        );
    }

    #[test]
    fn credential_origin_cannot_be_switched() {
        let source =
            validate_endpoint("https://api.example.test/v1", EndpointMode::Remote).unwrap();
        let same =
            validate_endpoint("https://api.example.test/v1/models", EndpointMode::Remote).unwrap();
        let foreign = validate_endpoint("https://malicious.test/v1", EndpointMode::Remote).unwrap();
        assert!(same_credential_origin(&source, &same));
        assert!(!same_credential_origin(&source, &foreign));
        let different_port =
            validate_endpoint("https://api.example.test:8443/v1", EndpointMode::Remote).unwrap();
        assert!(!same_credential_origin(&source, &different_port));
    }

    #[test]
    fn reject_malformed_urls_and_controls() {
        for name in [
            " https://example.test/v1",
            "https://example.test/\n",
            "https://",
            "file:///tmp/model",
        ] {
            assert!(validate_endpoint(name, EndpointMode::Remote).is_err());
        }
    }
}
