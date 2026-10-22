//! Provider-owned Sign in with ChatGPT registration boundary.
//!
//! Pure, deterministic URL and callback validation. The ProviderAuth adapter
//! MUST still generate unpredictable state/nonce/PKCE with a maintained OAuth2
//! library, bind the local listener before returning the browser challenge,
//! verify the returned ID token with OIDC/JWKS, enforce granted scopes, and
//! store/refresh credentials under an exact launcher-authorized account.
//! Nothing here authenticates a user or grants model execution.

use url::Url;

pub const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
pub const RESOURCE: &str = "https://api.openai.com/v1";
pub const REQUIRED_SCOPE: &str = "chatgpt.tokens.use.direct";
const REQUESTED_SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const NEW_CLIENT: &str = "dynamic_agent_client";

/// Only the *token endpoint's granted scope* is authoritative. The optional
/// scope in the browser callback is untrusted and MUST NOT be passed here.
pub fn require_token_granted_plan_scope(granted: &str) -> Result<(), RegistrationError> {
    if granted.len() > 8192
        || !granted.is_ascii()
        || granted.bytes().any(|b| b.is_ascii_control() && b != b' ')
    {
        return Err(RegistrationError::ScopeDenied);
    }
    if granted
        .split_ascii_whitespace()
        .any(|scope| scope == REQUIRED_SCOPE)
    {
        Ok(())
    } else {
        Err(RegistrationError::ScopeDenied)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidHost,
    InvalidRedirect,
    InvalidAttempt,
    InvalidCallback,
    InvalidState,
    ClientMismatch,
    AuthorizationDenied,
    ScopeDenied,
}

/// Registration is not an account identity. An issued client ID is bound to
/// the verified account only after OIDC validation by the provider.
pub enum Registration<'a> {
    First {
        app_name: &'a str,
    },
    Returning {
        issued_client_id: &'a str,
        id_token_hint: Option<&'a str>,
        login_hint: Option<&'a str>,
    },
}

/// All random values must originate from the OAuth adapter, not this helper.
/// A provider must retain the pending attempt and accept its callback once.
pub struct AuthorizationAttempt<'a> {
    pub registration: Registration<'a>,
    pub host_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub pkce_challenge: &'a str,
}

/// Not Debug/Clone: the code is a short-lived secret. Consume it exactly once
/// to construct the authorization-code exchange form, then discard the form.
pub struct VerifiedCallback {
    code: String,
    issued_client_id: String,
    redirect_uri: String,
}

impl VerifiedCallback {
    pub fn issued_client_id(&self) -> &str {
        &self.issued_client_id
    }

    /// The token endpoint must be used by a provider-owned transport with TLS
    /// verification, no redirects and no secret-bearing logs or journal data.
    pub fn exchange_form(self, code_verifier: &str) -> Result<String, RegistrationError> {
        if !valid_pkce_verifier(code_verifier) {
            return Err(RegistrationError::InvalidAttempt);
        }
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("grant_type", "authorization_code")
            .append_pair("client_id", &self.issued_client_id)
            .append_pair("code", &self.code)
            .append_pair("code_verifier", code_verifier)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("resource", RESOURCE);
        Ok(form.finish())
    }
}

fn valid_random(value: &str) -> bool {
    (24..=256).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid_pkce_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

fn valid_issued_client(value: &str) -> bool {
    value != NEW_CLIENT
        && !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn valid_host_id(value: &str) -> bool {
    let Some(uuid) = value.strip_prefix("urn:uuid:") else {
        return false;
    };
    uuid.len() == 36
        && uuid.bytes().enumerate().all(|(index, b)| {
            if [8, 13, 18, 23].contains(&index) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn redirect(uri: &str) -> Result<Url, RegistrationError> {
    // Do not let URL normalization accept empty userinfo, localhost aliases,
    // a zero listener port, or a path that differs from the issued callback.
    if !uri.starts_with("http://127.0.0.1:") {
        return Err(RegistrationError::InvalidRedirect);
    }
    let url = Url::parse(uri).map_err(|_| RegistrationError::InvalidRedirect)?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none_or(|port| port == 0)
        || url.path() != "/auth/callback"
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(RegistrationError::InvalidRedirect);
    }
    Ok(url)
}

impl AuthorizationAttempt<'_> {
    fn check(&self) -> Result<Url, RegistrationError> {
        if !valid_host_id(self.host_id) {
            return Err(RegistrationError::InvalidHost);
        }
        if !valid_random(self.state)
            || !valid_random(self.nonce)
            || self.pkce_challenge.len() != 43
            || !self
                .pkce_challenge
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(RegistrationError::InvalidAttempt);
        }
        match &self.registration {
            Registration::First { app_name } => {
                if app_name.trim().is_empty()
                    || app_name.len() > 96
                    || app_name.chars().any(char::is_control)
                {
                    return Err(RegistrationError::InvalidAttempt);
                }
            }
            Registration::Returning {
                issued_client_id,
                id_token_hint,
                login_hint,
            } => {
                if !valid_issued_client(issued_client_id)
                    || id_token_hint.is_some_and(|v| v.len() > 8192 || v.is_empty())
                    || login_hint.is_some_and(|v| v.len() > 320 || v.is_empty())
                {
                    return Err(RegistrationError::InvalidAttempt);
                }
            }
        }
        redirect(self.redirect_uri)
    }

    pub fn authorization_url(&self) -> Result<String, RegistrationError> {
        self.check()?;
        let mut url = Url::parse(AUTHORIZE_URL).expect("static SIWC authorization URL");
        let mut params = url.query_pairs_mut();
        let client_id = match &self.registration {
            Registration::First { app_name } => {
                params.append_pair("agent_name_hint", app_name);
                NEW_CLIENT
            }
            Registration::Returning {
                issued_client_id,
                id_token_hint,
                login_hint,
            } => {
                if let Some(token) = id_token_hint {
                    params.append_pair("id_token_hint", token);
                }
                if let Some(email) = login_hint {
                    params.append_pair("login_hint", email);
                }
                issued_client_id
            }
        };
        params
            .append_pair("client_id", client_id)
            .append_pair("ext_agent_host_id", self.host_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", self.redirect_uri)
            .append_pair("scope", REQUESTED_SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", self.state)
            .append_pair("nonce", self.nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", self.pkce_challenge);
        drop(params);
        Ok(url.into())
    }

    /// Consumes the pending attempt. Matching callback parameters proves only
    /// correlation, never authentication or ChatGPT plan entitlement.
    pub fn accept_callback(self, callback_uri: &str) -> Result<VerifiedCallback, RegistrationError> {
        let expected = self.check()?;
        if callback_uri.len() > 16 * 1024 {
            return Err(RegistrationError::InvalidCallback);
        }
        let actual = Url::parse(callback_uri).map_err(|_| RegistrationError::InvalidCallback)?;
        // Compare the raw callback authority/path as well as Url's parsed
        // fields: normalization must not hide empty userinfo or a substituted
        // callback host. The exact listener URL was fixed at authorization.
        if !callback_uri.starts_with(&format!("{}?", self.redirect_uri)) {
            return Err(RegistrationError::InvalidCallback);
        }
        if actual.scheme() != expected.scheme()
            || actual.host_str() != expected.host_str()
            || actual.port() != expected.port()
            || actual.path() != expected.path()
            || actual.fragment().is_some()
        {
            return Err(RegistrationError::InvalidCallback);
        }
        let mut code = None;
        let mut state = None;
        let mut client_id = None;
        let mut denied = false;
        for (key, value) in actual.query_pairs() {
            match key.as_ref() {
                "code" if code.replace(value.into_owned()).is_some() => {
                    return Err(RegistrationError::InvalidCallback);
                }
                "state" if state.replace(value.into_owned()).is_some() => {
                    return Err(RegistrationError::InvalidCallback);
                }
                "client_id" if client_id.replace(value.into_owned()).is_some() => {
                    return Err(RegistrationError::InvalidCallback);
                }
                "error" => {
                    if denied {
                        return Err(RegistrationError::InvalidCallback);
                    }
                    denied = true;
                }
                _ => {}
            }
        }
        if state.as_deref() != Some(self.state) {
            return Err(RegistrationError::InvalidState);
        }
        if denied {
            return Err(RegistrationError::AuthorizationDenied);
        }
        let code = code
            .filter(|code| !code.is_empty() && code.len() <= 8192)
            .ok_or(RegistrationError::InvalidCallback)?;
        let issued_client_id = match self.registration {
            Registration::First { .. } => client_id
                .filter(|id| valid_issued_client(id))
                .ok_or(RegistrationError::ClientMismatch)?,
            Registration::Returning {
                issued_client_id, ..
            } => {
                if client_id.as_deref().is_some_and(|id| id != issued_client_id) {
                    return Err(RegistrationError::ClientMismatch);
                }
                issued_client_id.to_owned()
            }
        };
        Ok(VerifiedCallback {
            code,
            issued_client_id,
            redirect_uri: self.redirect_uri.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "urn:uuid:123e4567-e89b-12d3-a456-426614174000";
    const CALLBACK: &str = "http://127.0.0.1:1455/auth/callback";
    const STATE: &str = "abcdefghijklmnopqrstuvwxyz0123456789";
    const NONCE: &str = "zyxwvutsrqponmlkjihgfedcba9876543210";
    const CHALLENGE: &str = "abcdefghijklmnopqrstuvwxyz0123456789abcdefg";
    const VERIFIER: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";

    fn start(registration: Registration<'static>) -> AuthorizationAttempt<'static> {
        AuthorizationAttempt {
            registration,
            host_id: HOST,
            redirect_uri: CALLBACK,
            state: STATE,
            nonce: NONCE,
            pkce_challenge: CHALLENGE,
        }
    }

    fn first() -> AuthorizationAttempt<'static> {
        start(Registration::First { app_name: "Phenix AI" })
    }

    #[test]
    fn dynamic_registration_keeps_issued_client_id_for_exchange() {
        let uri = first().authorization_url().unwrap();
        let params: std::collections::BTreeMap<_, _> =
            Url::parse(&uri).unwrap().query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], NEW_CLIENT);
        assert_eq!(params["agent_name_hint"], "Phenix AI");
        assert_eq!(params["ext_agent_host_id"], HOST);
        assert_eq!(params["scope"], REQUESTED_SCOPES);
        assert_eq!(params["redirect_uri"], CALLBACK);
        assert_eq!(params["code_challenge_method"], "S256");

        let returned = first()
            .accept_callback(&format!(
                "{CALLBACK}?state={STATE}&code=short-lived-code&client_id=oaiapp_issued"
            ))
            .unwrap();
        assert_eq!(returned.issued_client_id(), "oaiapp_issued");
        let form = returned.exchange_form(VERIFIER).unwrap();
        let fields: std::collections::BTreeMap<_, _> =
            url::form_urlencoded::parse(form.as_bytes()).into_owned().collect();
        assert_eq!(fields["client_id"], "oaiapp_issued");
        assert_eq!(fields["code"], "short-lived-code");
        assert_eq!(fields["resource"], RESOURCE);
        assert_eq!(fields["redirect_uri"], CALLBACK);
        assert_eq!(fields["code_verifier"], VERIFIER);
        assert!(!fields.contains_key("client_secret"));
    }

    #[test]
    fn initial_callback_cannot_exchange_dynamic_placeholder_or_missing_id() {
        for uri in [
            format!("{CALLBACK}?state={STATE}&code=c"),
            format!("{CALLBACK}?state={STATE}&code=c&client_id={NEW_CLIENT}"),
            format!("{CALLBACK}?state={STATE}&code=c&client_id=bad%0Aid"),
        ] {
            assert!(matches!(
                first().accept_callback(&uri),
                Err(RegistrationError::ClientMismatch)
            ));
        }
    }

    #[test]
    fn returning_account_reuses_only_its_issued_client_id() {
        fn returning() -> AuthorizationAttempt<'static> {
            start(Registration::Returning {
                issued_client_id: "oaiapp_saved",
                id_token_hint: Some("saved-id-token"),
                login_hint: Some("user@example.test"),
            })
        }
        let url = returning().authorization_url().unwrap();
        let query: std::collections::BTreeMap<_, _> =
            Url::parse(&url).unwrap().query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "oaiapp_saved");
        assert!(!query.contains_key("agent_name_hint"));
        assert_eq!(query["id_token_hint"], "saved-id-token");
        assert_eq!(query["login_hint"], "user@example.test");
        assert_eq!(
            returning()
                .accept_callback(&format!("{CALLBACK}?state={STATE}&code=c"))
                .unwrap()
                .issued_client_id(),
            "oaiapp_saved"
        );
        assert!(matches!(
            returning().accept_callback(&format!(
                "{CALLBACK}?state={STATE}&code=c&client_id=oaiapp_other"
            )),
            Err(RegistrationError::ClientMismatch)
        ));
    }

    #[test]
    fn wrong_state_denial_and_replayed_fields_fail_closed() {
        for (uri, failure) in [
            (
                format!("{CALLBACK}?state=wrong&code=c&client_id=oaiapp_issued"),
                RegistrationError::InvalidState,
            ),
            (
                format!("{CALLBACK}?state={STATE}&state={STATE}&code=c&client_id=oaiapp_issued"),
                RegistrationError::InvalidCallback,
            ),
            (
                format!("{CALLBACK}?state={STATE}&code=c&code=c&client_id=oaiapp_issued"),
                RegistrationError::InvalidCallback,
            ),
            (
                format!("{CALLBACK}?state={STATE}&error=access_denied"),
                RegistrationError::AuthorizationDenied,
            ),
        ] {
            match first().accept_callback(&uri) {
                Err(problem) => assert_eq!(problem, failure),
                Ok(_) => panic!("unverified callback accepted"),
            }
        }
    }

    #[test]
    fn only_exact_loopback_authorization_callback_is_accepted() {
        for uri in [
            "http://localhost:1455/auth/callback",
            "http://127.0.0.1:1455/callback",
            "http://127.0.0.1:9999/auth/callback",
            "https://127.0.0.1:1455/auth/callback",
            "http://127.0.0.2:1455/auth/callback",
            "http://127.0.0.1:1455/auth/callback#fragment",
            "http://@127.0.0.1:1455/auth/callback",
            "http://127.0.0.1:0/auth/callback",
        ] {
            assert!(first().accept_callback(&format!("{uri}?state={STATE}&code=c"))
                .is_err(), "{uri}");
        }
        assert!(matches!(
            AuthorizationAttempt {
                redirect_uri: "http://localhost:1455/auth/callback",
                ..first()
            }
            .authorization_url(),
            Err(RegistrationError::InvalidRedirect)
        ));
    }

    #[test]
    fn token_scopes_must_include_exact_plan_permission() {
        assert_eq!(
            require_token_granted_plan_scope(
                "openid email resource.invoke chatgpt.tokens.use.direct offline_access"
            ),
            Ok(())
        );
        for granted in [
            "openid email resource.invoke",
            "openid chatgpt.tokens.use.direct.extra",
            "openid chatgpt.tokens.use",
            "chatgpt.tokens.use.direct\nother",
            "",
        ] {
            assert_eq!(
                require_token_granted_plan_scope(granted),
                Err(RegistrationError::ScopeDenied),
                "{granted:?}"
            );
        }
    }

    #[test]
    fn malformed_randomness_host_and_pkce_parameters_fail_before_browser() {
        assert!(matches!(
            AuthorizationAttempt {
                state: "short",
                ..first()
            }
            .authorization_url(),
            Err(RegistrationError::InvalidAttempt)
        ));
        assert!(matches!(
            AuthorizationAttempt {
                host_id: "not-a-host-id",
                ..first()
            }
            .authorization_url(),
            Err(RegistrationError::InvalidHost)
        ));
        assert!(matches!(
            first().accept_callback(&format!(
                "{CALLBACK}?state={STATE}&code=c&client_id=oaiapp_issued"
            )).unwrap().exchange_form("not-pkce"),
            Err(RegistrationError::InvalidAttempt)
        ));
    }
}
