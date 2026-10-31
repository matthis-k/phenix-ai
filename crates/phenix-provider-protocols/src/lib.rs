//! Shared provider connection validation. Network authority stays in Phenix.
//!
//! URL validation alone is not connection authority. Runtime HTTP transports
//! resolve and authorize addresses before dispatch and pin the result in curl.
//! Do not treat an approved URL as permission to connect or use credentials.

use std::net::{IpAddr, ToSocketAddrs};
use url::Url;

#[cfg(feature = "oauth-loopback")]
pub mod oauth_loopback;
#[cfg(feature = "oauth-loopback")]
pub mod oauth_oidc;
#[cfg(feature = "oauth2-pkce")]
pub mod oauth_pkce;
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
    ResolutionFailed,
    UnsafeAddress,
}

/// An immutable curl DNS-cache entry. Keep the original URL for Host and TLS SNI.
#[derive(Debug)]
pub struct ResolvedDestination {
    pub url: String,
    pub resolve_entry: String,
}

/// Resolve once, reject the entire answer if any address is prohibited, and pin
/// the authorized addresses. A fresh curl handle must disable proxies/redirects.
pub fn resolve_destination(
    endpoint: &str,
    mode: EndpointMode,
) -> Result<ResolvedDestination, EndpointProblem> {
    resolve_destination_with(endpoint, mode, |host, port| {
        (host, port)
            .to_socket_addrs()
            .map(|answers| answers.map(|answer| answer.ip()).collect())
            .map_err(|_| EndpointProblem::ResolutionFailed)
    })
}

fn resolve_destination_with(
    endpoint: &str,
    mode: EndpointMode,
    resolve: impl FnOnce(&str, u16) -> Result<Vec<IpAddr>, EndpointProblem>,
) -> Result<ResolvedDestination, EndpointProblem> {
    let checked = validate_endpoint(endpoint, mode)?;
    let url = Url::parse(&checked.url).map_err(|_| EndpointProblem::Malformed)?;
    let port = url
        .port_or_known_default()
        .ok_or(EndpointProblem::Malformed)?;
    let literal = match url.host().ok_or(EndpointProblem::Malformed)? {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    };
    let host = url.host_str().ok_or(EndpointProblem::Malformed)?;
    let addresses = match literal {
        Some(ip) => vec![ip],
        None => resolve(host, port)?,
    };
    if addresses.is_empty() {
        return Err(EndpointProblem::ResolutionFailed);
    }
    if addresses.iter().any(|ip| {
        !(public_address(*ip)
            || (mode == EndpointMode::LocalLoopback && literal == Some(*ip) && ip.is_loopback()))
    }) {
        return Err(EndpointProblem::UnsafeAddress);
    }
    let address = addresses
        .into_iter()
        .map(|ip| match ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(ResolvedDestination {
        url: checked.url,
        resolve_entry: format!("{host}:{port}:{address}"),
    })
}

fn public_address(ip: IpAddr) -> bool {
    // IANA IPv4/IPv6 Special-Purpose Address Registries, updated 2025-10-09:
    // https://www.iana.org/assignments/iana-ipv4-special-registry/
    // https://www.iana.org/assignments/iana-ipv6-special-registry/
    // Honor globally reachable exceptions. Our stricter native-unicast policy
    // additionally excludes IPv6 translation, Teredo, 6to4 and ORCHID/DETs.
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, d] = ip.octets();
            if [a, b, c] == [192, 0, 0] && matches!(d, 9 | 10) {
                return true;
            }
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            let ietf_global =
                (s[..6] == [0x2001, 1, 0, 0, 0, 0] && s[6] == 0 && matches!(s[7], 1..=3))
                    || s[..2] == [0x2001, 3]
                    || s[..3] == [0x2001, 4, 0x112];
            // Only native global unicast; exclude mapped/compatible, NAT64,
            // local, multicast, transition, documentation and special ranges.
            (s[0] & 0xe000) == 0x2000
                && (!(s[0] == 0x2001 && s[1] < 0x200) || ietf_global)
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
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
    fn resolved_policy_rejects_special_and_mixed_answers() {
        for address in [
            "0.0.0.0",
            "10.1.2.3",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.88.99.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:8.8.8.8",
            "64:ff9b::a00:1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "2002:808:808::1",
            "3fff::1",
        ] {
            let ip = address.parse().unwrap();
            assert!(!public_address(ip), "{address}");
            for answers in [vec![ip], vec!["8.8.8.8".parse().unwrap(), ip]] {
                assert!(
                    matches!(
                        resolve_destination_with(
                            "https://provider.invalid/models",
                            EndpointMode::LocalLoopback,
                            |_, _| Ok(answers)
                        ),
                        Err(EndpointProblem::UnsafeAddress)
                    ),
                    "{address}"
                );
            }
        }
        for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_address(address.parse().unwrap()));
        }
        let destination =
            resolve_destination_with("https://provider.invalid", EndpointMode::Remote, |_, _| {
                Ok(vec![
                    "2606:4700:4700::1111".parse().unwrap(),
                    "1.1.1.1".parse().unwrap(),
                ])
            })
            .unwrap();
        assert_eq!(
            destination.resolve_entry,
            "provider.invalid:443:[2606:4700:4700::1111],1.1.1.1"
        );
    }

    #[test]
    fn iana_boundaries_and_native_unicast_exceptions() {
        for (first, last) in [
            ("0.0.0.0", "0.255.255.255"),
            ("10.0.0.0", "10.255.255.255"),
            ("100.64.0.0", "100.127.255.255"),
            ("127.0.0.0", "127.255.255.255"),
            ("169.254.0.0", "169.254.255.255"),
            ("172.16.0.0", "172.31.255.255"),
            ("192.0.2.0", "192.0.2.255"),
            ("192.88.99.0", "192.88.99.255"),
            ("192.168.0.0", "192.168.255.255"),
            ("198.18.0.0", "198.19.255.255"),
            ("198.51.100.0", "198.51.100.255"),
            ("203.0.113.0", "203.0.113.255"),
        ] {
            let first = u32::from(first.parse::<std::net::Ipv4Addr>().unwrap());
            let last = u32::from(last.parse::<std::net::Ipv4Addr>().unwrap());
            for address in [first, last] {
                assert!(!public_address(std::net::Ipv4Addr::from(address).into()));
            }
            for address in [first.checked_sub(1), last.checked_add(1)]
                .into_iter()
                .flatten()
            {
                assert!(public_address(std::net::Ipv4Addr::from(address).into()));
            }
        }
        for last in 0..=255 {
            let ip = std::net::Ipv4Addr::new(192, 0, 0, last);
            assert_eq!(public_address(ip.into()), matches!(last, 9 | 10), "{ip}");
        }
        for address in [
            "192.0.1.0",
            "192.0.255.255",
            "192.31.196.1",
            "192.52.193.1",
            "192.175.48.1",
            "2001:1::1",
            "2001:1::2",
            "2001:1::3",
            "2001:3::",
            "2001:3:ffff:ffff:ffff:ffff:ffff:ffff",
            "2001:4:112::",
            "2001:4:112:ffff:ffff:ffff:ffff:ffff",
            "2001:200::",
            "2620:4f:8000::1",
            "3ffe:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "3fff:1000::",
            "223.255.255.255",
        ] {
            assert!(public_address(address.parse().unwrap()), "{address}");
        }
        for address in [
            "::8.8.8.8",
            "::ffff:0:8.8.8.8",
            "64:ff9b:1::1",
            "100:0:0:1::1",
            "2001:1::",
            "2001:1::4",
            "2001:2:ffff::1",
            "2001:4:111:ffff::1",
            "2001:4:113::",
            "2001:20::1",
            "2001:30::1",
            "2001:1ff:ffff::1",
            "2002:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            "3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff",
            "4000::",
            "5f00::1",
        ] {
            let ip = address.parse().unwrap();
            assert!(!public_address(ip), "{address}");
            for answers in [
                vec![ip, "1.1.1.1".parse().unwrap()],
                vec!["2606:4700::1111".parse().unwrap(), ip],
            ] {
                assert!(
                    matches!(
                        resolve_destination_with(
                            "https://provider.invalid",
                            EndpointMode::Remote,
                            |_, _| Ok(answers)
                        ),
                        Err(EndpointProblem::UnsafeAddress)
                    ),
                    "{address}"
                );
            }
        }
    }

    #[test]
    fn resolution_is_a_single_immutable_snapshot_and_literals_skip_dns() {
        let mut calls = 0;
        let destination = resolve_destination_with(
            "https://provider.invalid:8443/models",
            EndpointMode::Remote,
            |host, port| {
                calls += 1;
                assert_eq!((host, port), ("provider.invalid", 8443));
                Ok(vec!["8.8.8.8".parse().unwrap()])
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(destination.resolve_entry, "provider.invalid:8443:8.8.8.8");
        // A later DNS change cannot mutate the entry handed to curl; the next
        // request must authorize again and reject the changed answer.
        assert!(matches!(
            resolve_destination_with(
                "https://provider.invalid:8443/models",
                EndpointMode::Remote,
                |_, _| Ok(vec!["127.0.0.1".parse().unwrap()])
            ),
            Err(EndpointProblem::UnsafeAddress)
        ));
        assert_eq!(destination.resolve_entry, "provider.invalid:8443:8.8.8.8");
        for endpoint in ["http://127.0.0.1:1234", "https://[::1]:1234"] {
            assert!(resolve_destination_with(
                endpoint,
                EndpointMode::LocalLoopback,
                |_, _| panic!("literal must not resolve")
            )
            .is_ok());
            assert!(resolve_destination(endpoint, EndpointMode::Remote).is_err());
        }
        assert!(matches!(
            resolve_destination_with("https://provider.invalid", EndpointMode::Remote, |_, _| Ok(
                vec![]
            )),
            Err(EndpointProblem::ResolutionFailed)
        ));
    }

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
