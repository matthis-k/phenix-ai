//! Reusable OAuth/OIDC identity verification strategy.
//!
//! A provider supplies issuer, issued client ID, nonce, and a JWKS fetched
//! from its trusted, configured HTTPS origin. No provider-specific endpoint
//! knowledge, secret storage, model codec or kernel policy lives here.
//! Callback correlation and a token response are never identity evidence.

use openidconnect::core::{
    CoreIdToken, CoreIdTokenVerifier, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
};
use openidconnect::{ClientId, IssuerUrl, Nonce};
use serde_json::Value;

const MAX_JWKS_BODY: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OidcError {
    InvalidConfiguration,
    InvalidJwks,
    InvalidIdToken,
    IncorrectAccount,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedSubject(String);

impl VerifiedSubject {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reusable public-client OIDC signature, issuer, audience, expiry and nonce
/// gate. The caller must fetch JWKS from a trusted provider-declared endpoint
/// using TLS and no redirects: **no URL supplied by the JWT is consulted**.
/// This strategy does not decide access-token scope or account storage policy.
pub fn verify_signed_oidc_identity(
    jwt: &str,
    jwks_body: &[u8],
    issuer: &str,
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
) -> Result<VerifiedSubject, OidcError> {
    verify_signed_oidc_identity_with_nonce(
        jwt,
        jwks_body,
        issuer,
        issued_client_id,
        original_nonce,
        expected_subject,
        false,
    )
}

/// Refresh responses may omit nonce by OIDC Core section 12.2. If they do
/// include one, it must still match the original login nonce. A signed
/// same-subject ID token, not an OAuth response or client-supplied identifier,
/// is required for accepting an updated refresh generation.
pub fn verify_refreshed_signed_oidc_identity(
    jwt: &str,
    jwks_body: &[u8],
    issuer: &str,
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: &str,
) -> Result<VerifiedSubject, OidcError> {
    if expected_subject.is_empty() {
        return Err(OidcError::InvalidConfiguration);
    }
    verify_signed_oidc_identity_with_nonce(
        jwt,
        jwks_body,
        issuer,
        issued_client_id,
        original_nonce,
        Some(expected_subject),
        true,
    )
}

fn verify_signed_oidc_identity_with_nonce(
    jwt: &str,
    jwks_body: &[u8],
    issuer: &str,
    issued_client_id: &str,
    original_nonce: &str,
    expected_subject: Option<&str>,
    allow_absent_nonce: bool,
) -> Result<VerifiedSubject, OidcError> {
    if jwt.is_empty()
        || jwt.len() > 16 * 1024
        || issued_client_id.is_empty()
        || issued_client_id.len() > 256
        || original_nonce.len() < 24
        || original_nonce.len() > 256
        || !issuer.starts_with("https://")
        || issuer.chars().any(char::is_control)
    {
        return Err(OidcError::InvalidConfiguration);
    }
    if jwks_body.is_empty() || jwks_body.len() > MAX_JWKS_BODY {
        return Err(OidcError::InvalidJwks);
    }
    let jwks: CoreJsonWebKeySet =
        serde_json::from_slice(jwks_body).map_err(|_| OidcError::InvalidJwks)?;
    let id_token: CoreIdToken = serde_json::from_value(Value::String(jwt.to_owned()))
        .map_err(|_| OidcError::InvalidIdToken)?;
    let verifier = CoreIdTokenVerifier::new_public_client(
        ClientId::new(issued_client_id.to_owned()),
        IssuerUrl::new(issuer.to_owned()).map_err(|_| OidcError::InvalidConfiguration)?,
        jwks,
    )
    .set_allowed_algs([CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256]);
    let claims = if allow_absent_nonce {
        id_token.claims(&verifier, |nonce: Option<&Nonce>| {
            if nonce.is_some_and(|value| value.secret() != original_nonce) {
                Err("refreshed ID token nonce differs from login".to_owned())
            } else {
                Ok(())
            }
        })
    } else {
        id_token.claims(&verifier, &Nonce::new(original_nonce.to_owned()))
    }
    .map_err(|_| OidcError::InvalidIdToken)?;
    let subject = claims.subject().as_str();
    if subject.is_empty() || subject.len() > 256 {
        return Err(OidcError::InvalidIdToken);
    }
    if expected_subject.is_some_and(|current| current != subject) {
        return Err(OidcError::IncorrectAccount);
    }
    Ok(VerifiedSubject(subject.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Public-only RS256 verification fixture, signed offline with a 2048-bit
    // test RSA key. No private signing key is checked into the repository.
    // The fixed expiry is 2100-01-01 to keep the positive case deterministic.
    const SIGNED_JWKS: &str = r#"{"keys":[{"kty":"RSA","alg":"RS256","use":"sig","kid":"phenix-fixture-rs256-1","n":"tM2hQYSNSJNoI3jy7UxEpbdO8-64l8-iZKcAH-PKiohkiDTZJVmkH4YBUjNRYk6MrwsJe5a_7G7NecOjLkBMo43Jfng8aiu-IZJZRMdmjLMNXlxrDcBfiRvjQxi7s9I1QgNOLwSIydWoQaxiJfqaqLYTB6ObY7BgCw0McARk7k0YGZaV6B6w6wVqe4pX1L9Ebm6cgxjJ0AysSz7q0JZ2qLZxDT_ntLFji1ML34GJ_ZqEY6ZKrjK2jKtDgeZjqfUI7sXlJMKzrIXLGxXgVldxjJjVkZK6z8LVju-5KRtMI4aEE1SY4KO1MMnxxgKj1aONifGyN3eBWCv-4RS-KtiHUQ","e":"AQAB"}]}"#;
    const SIGNED_ID_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6InBoZW5peC1maXh0dXJlLXJzMjU2LTEifQ.eyJpc3MiOiJodHRwczovL2F1dGguZXhhbXBsZS50ZXN0Iiwic3ViIjoidXNlci1maXh0dXJlIiwiYXVkIjoiaXNzdWVkLWNsaWVudCIsImV4cCI6NDEwMjQ0NDgwMCwiaWF0IjoxNzYwMDAwMDAwLCJub25jZSI6InRlc3Qtbm9uY2UtdGVzdC1ub25jZS10ZXN0LW5vbmNlLXRlc3Qtbm9uY2UifQ.bvvzb_vJ667uVfKM52WTZGsgqcYY39O_wwZ_2RVBhnB0TjVhAqdhRvDKfUJy2dtMSk3hby40aFV5PVtVuCc8gxiwoct4N81OlLNrHE3nZLA6bKwd5sLis9RFM5HU1wZZ9AdQmnzwlNggCmHrUD2u-lIS7xhN24ZMkHUns-4AqIyM1Cz50MwILVFrF82O1-G4RVm2L7rYU8e0HXj5YwJ3F7LreDpISmfkACFAke8i4cwvHmwcEzv36pAnYJFXbFHycCrse-oQT_-HW7J0RbXXzLDaK1om-E4aIDL_n6RFcZYuph6O3OcRybRByOo4yuEYZfkb-AHUiBLFGoqKTLW-wg";
    const EXPIRED_ID_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6InBoZW5peC1maXh0dXJlLXJzMjU2LTEifQ.eyJpc3MiOiJodHRwczovL2F1dGguZXhhbXBsZS50ZXN0Iiwic3ViIjoidXNlci1maXh0dXJlIiwiYXVkIjoiaXNzdWVkLWNsaWVudCIsImV4cCI6MTYwMDAwMDAwMCwiaWF0IjoxNTk5OTkwMDAwLCJub25jZSI6InRlc3Qtbm9uY2UtdGVzdC1ub25jZS10ZXN0LW5vbmNlLXRlc3Qtbm9uY2UifQ.jp9t67hj4x7pXD5eB32ctrbdSuSf90HL2aOMrjZNss-hWfHNXcRG1qd9wL6RHoDeSbTEf5ywrdalmFVNGFdeGgGKX72wGybAsJxIqevq1r7Ac1oFgKLoQIzRdutb6CuLi8VFPfh-BJNGzBjCdLv1LkdvXzAORyZY0t1m5DwhZ6-nY6is4sNBZDSgEV2SspMQnrGo-Sk-sp0yd-4tm0bU3jFjpSUyKasaAbRxppb0O4n2b4F0DX2syz_aLkbXS8CyvXDkrhz86awSGvcy1LnMymYK0vW5CLNRiaBSSMMrM74hvZfGnxTgepJnhmDc3yKLoJWna-cpEs2catFXmtVhrw";
    const TEST_NONCE: &str = "test-nonce-test-nonce-test-nonce-test-nonce";

    // Signed offline with a throwaway RSA key; only the public JWK and the
    // signed fixture are retained. OIDC Core 12.2 permits this missing nonce
    // on a refresh but never on an initial interactive browser login.
    const NONCELESS_REFRESH_JWKS: &str = r#"{"keys":[{"kty":"RSA","alg":"RS256","use":"sig","kid":"p1-refresh-nonce-free","n":"3C28SR7IBYMsRGc9lsNOXZdjDrXrtg_UhLbc_-TxJd0ykm0YU7jkIXWekFuzqpDD6Y0Z1JFY4-DPjsOYa1ZBQtrIFis5yqMtx2UZ7A0l9jrLnWWgufzyo83tquHdipITDbQpK6RyUVSQsYoiDuVK7k19S8KLhsvIfTmkqhUpOeZS31bk54aDzeasUjNddhlHsUKOjR_FdoKlsSLnpbllsMrGOqiNlLgobOrwD_DH8e3dBsMJXcUZv5fDn0YcDByz05keokRvI-8VKhlKliu5KWWP1UBw-XMIDzt5HDZZt2gmAHLKPqypsD7PftmTOwCSMeM6G2jeHmuX_tsR7ierfQ","e":"AQAB"}]}"#;
    const NONCELESS_REFRESH_JWT: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6InAxLXJlZnJlc2gtbm9uY2UtZnJlZSJ9.eyJpc3MiOiJodHRwczovL2F1dGguZXhhbXBsZS50ZXN0Iiwic3ViIjoidXNlci1maXh0dXJlIiwiYXVkIjoiaXNzdWVkLWNsaWVudCIsImV4cCI6NDEwMjQ0NDgwMCwiaWF0IjoxNzkwMDMyNTMyfQ.YDiZYiNIsqwykf6G6pV97tu1lcykNO5G_fO9NDWmIz6mSyClJ6Lt2hxo6HJnIrqA20u0jZff5g9pK93Ct1eeJfAdQ4T4Ve1BL6u0a81JyyOMbjFgeOWus1BPlRpFDqFTcfk8qqR12pf2KSxufEX9qMa_SMn2JfLG0yAlBegmdeWHgbixDvFQlWoabp5oNgiSgPEqNu3yC2AQ4MDv3CjUYMWWK3o_OLSq1PzsaF1nwlVl8VbsBX-ilNLTFreNSFN1mmfSr8LSIHLWtLSPUbIsLLrlKpOIK9zuysczlrhDBb1rw__UFI2_PtofUFI4NIyZplI31LGKXwFg58Yee4xdfQ";

    #[test]
    fn refresh_allows_absent_nonce_only_for_same_signed_subject_and_client() {
        let verify = |issuer, client, subject| {
            verify_refreshed_signed_oidc_identity(
                NONCELESS_REFRESH_JWT,
                NONCELESS_REFRESH_JWKS.as_bytes(),
                issuer,
                client,
                TEST_NONCE,
                subject,
            )
        };
        assert_eq!(
            verify("https://auth.example.test", "issued-client", "user-fixture")
                .unwrap()
                .as_str(),
            "user-fixture"
        );
        assert_eq!(
            verify_signed_oidc_identity(
                NONCELESS_REFRESH_JWT,
                NONCELESS_REFRESH_JWKS.as_bytes(),
                "https://auth.example.test",
                "issued-client",
                TEST_NONCE,
                Some("user-fixture"),
            ),
            Err(OidcError::InvalidIdToken)
        );
        assert_eq!(
            verify("https://auth.example.test", "issued-client", "different-user"),
            Err(OidcError::IncorrectAccount)
        );
        assert_eq!(
            verify("https://auth.example.test", "wrong-client", "user-fixture"),
            Err(OidcError::InvalidIdToken)
        );
        assert_eq!(
            verify("https://other.example.test", "issued-client", "user-fixture"),
            Err(OidcError::InvalidIdToken)
        );
    }

    #[test]
    fn signed_id_token_verifies_issuer_audience_nonce_expiry_and_account() {
        let verified = verify_signed_oidc_identity(
            SIGNED_ID_TOKEN,
            SIGNED_JWKS.as_bytes(),
            "https://auth.example.test",
            "issued-client",
            TEST_NONCE,
            Some("user-fixture"),
        )
        .expect("the signed RS256 fixture must verify cryptographically");
        assert_eq!(verified.as_str(), "user-fixture");

        for (issuer, client, nonce) in [
            (
                "https://different.example.test",
                "issued-client",
                TEST_NONCE,
            ),
            ("https://auth.example.test", "different-client", TEST_NONCE),
            (
                "https://auth.example.test",
                "issued-client",
                "other-nonce-other-nonce-other-nonce",
            ),
        ] {
            assert_eq!(
                verify_signed_oidc_identity(
                    SIGNED_ID_TOKEN,
                    SIGNED_JWKS.as_bytes(),
                    issuer,
                    client,
                    nonce,
                    None,
                ),
                Err(OidcError::InvalidIdToken)
            );
        }
        assert_eq!(
            verify_signed_oidc_identity(
                SIGNED_ID_TOKEN,
                SIGNED_JWKS.as_bytes(),
                "https://auth.example.test",
                "issued-client",
                TEST_NONCE,
                Some("another-user"),
            ),
            Err(OidcError::IncorrectAccount)
        );
        assert_eq!(
            verify_signed_oidc_identity(
                EXPIRED_ID_TOKEN,
                SIGNED_JWKS.as_bytes(),
                "https://auth.example.test",
                "issued-client",
                TEST_NONCE,
                None,
            ),
            Err(OidcError::InvalidIdToken)
        );
        // A well-formed token body with a modified signature is not identity.
        let mut forged = SIGNED_ID_TOKEN.to_owned();
        forged.pop();
        forged.push(if SIGNED_ID_TOKEN.ends_with('A') {
            'B'
        } else {
            'A'
        });
        assert_eq!(
            verify_signed_oidc_identity(
                &forged,
                SIGNED_JWKS.as_bytes(),
                "https://auth.example.test",
                "issued-client",
                TEST_NONCE,
                None,
            ),
            Err(OidcError::InvalidIdToken)
        );
    }

    #[test]
    fn invalid_tokens_never_produce_verified_subjects() {
        for issuer in ["https://auth.example.test", "https://auth.openai.com"] {
            assert_eq!(
                verify_signed_oidc_identity(
                    "unsigned-or-forged-token",
                    br#"{"keys":[]}"#,
                    issuer,
                    "issued-client",
                    "randomnoncerandomnoncerandomnonce",
                    None,
                ),
                Err(OidcError::InvalidIdToken)
            );
        }
    }

    #[test]
    fn invalid_provider_verification_parameters_fail_before_jwt() {
        assert_eq!(
            verify_signed_oidc_identity(
                "jwt",
                br#"{"keys":[]}"#,
                "http://host.test",
                "issued-client",
                "randomnoncerandomnoncerandomnonce",
                None,
            ),
            Err(OidcError::InvalidConfiguration)
        );
        assert_eq!(
            verify_signed_oidc_identity(
                "jwt",
                b"not jwks",
                "https://issuer.example.test",
                "issued-client",
                "randomnoncerandomnoncerandomnonce",
                None,
            ),
            Err(OidcError::InvalidJwks)
        );
    }
}
