use crate::{
    ApiTokenScheme, ApiTokenSource, Auth, AuthKind, EnvironmentVariable,
    EnvironmentVariableParseError, HeaderName, HeaderNameParseError, Token, TokenParseError,
};
use std::collections::BTreeMap;

pub use crate::{
    ApiTokenScheme as ApiTokenMethod, ApiTokenSource as ApiToken, Auth as Credential,
    AuthDescriptor as CredentialDescriptor,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Definition {
    pub api_token: Option<ApiTokenMethod>,
    pub oauth: Option<OAuthMethod>,
}

impl Definition {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            api_token: None,
            oauth: None,
        }
    }

    #[must_use]
    pub const fn api_token(api_token: ApiTokenMethod) -> Self {
        Self {
            api_token: Some(api_token),
            oauth: None,
        }
    }

    #[must_use]
    pub const fn oauth(oauth: OAuthMethod) -> Self {
        Self {
            api_token: None,
            oauth: Some(oauth),
        }
    }

    #[must_use]
    pub fn with_api_token(mut self, api_token: ApiTokenMethod) -> Self {
        self.api_token = Some(api_token);
        self
    }

    #[must_use]
    pub fn with_oauth(mut self, oauth: OAuthMethod) -> Self {
        self.oauth = Some(oauth);
        self
    }

    pub(crate) fn kinds(&self) -> Vec<AuthKind> {
        let mut kinds = Vec::with_capacity(2);
        if self.api_token.is_some() {
            kinds.push(AuthKind::ApiToken);
        }
        if self.oauth.is_some() {
            kinds.push(AuthKind::OAuth);
        }
        kinds
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.api_token.is_none() && self.oauth.is_none()
    }
}

impl From<ApiTokenMethod> for Definition {
    fn from(value: ApiTokenMethod) -> Self {
        Self::api_token(value)
    }
}

impl From<OAuthMethod> for Definition {
    fn from(value: OAuthMethod) -> Self {
        Self::oauth(value)
    }
}

/// Interactive authorization-code OAuth policy for one provider.
///
/// Protocol mechanics (PKCE/state, callback verification, token exchange and
/// refresh) are owned by the provider SDK. Providers supply only endpoint and
/// presentation policy. Browser launching remains a frontend responsibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthMethod {
    pub(crate) authorization_url: String,
    pub(crate) token_url: String,
    pub(crate) client_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) authorization_params: BTreeMap<String, String>,
    pub(crate) callback_ports: Vec<u16>,
    pub(crate) static_headers: BTreeMap<String, String>,
    pub(crate) account_id_header: Option<String>,
    pub(crate) account_id_claim_paths: Vec<Vec<String>>,
}

impl OAuthMethod {
    pub fn authorization_code(
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let authorization_url = authorization_url.into();
        let token_url = token_url.into();
        let client_id = client_id.into();
        if authorization_url.trim().is_empty() || token_url.trim().is_empty() {
            return Err("OAuth authorization and token URLs must not be empty");
        }
        if client_id.trim().is_empty() {
            return Err("OAuth client id must not be empty");
        }
        url::Url::parse(&authorization_url).map_err(|_| "invalid OAuth authorization URL")?;
        url::Url::parse(&token_url).map_err(|_| "invalid OAuth token URL")?;
        Ok(Self {
            authorization_url,
            token_url,
            client_id,
            scopes: Vec::new(),
            authorization_params: BTreeMap::new(),
            callback_ports: vec![1455, 1457],
            static_headers: BTreeMap::new(),
            account_id_header: None,
            account_id_claim_paths: Vec::new(),
        })
    }

    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        let scope = scope.into();
        if !scope.trim().is_empty() {
            self.scopes.push(scope);
        }
        self
    }

    #[must_use]
    pub fn with_authorization_param(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.authorization_params.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_callback_ports(mut self, ports: impl IntoIterator<Item = u16>) -> Self {
        self.callback_ports = ports.into_iter().collect();
        self
    }

    #[must_use]
    pub fn with_static_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.static_headers.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_account_id_header(
        mut self,
        header: impl Into<String>,
        claim_paths: impl IntoIterator<Item = Vec<String>>,
    ) -> Self {
        self.account_id_header = Some(header.into());
        self.account_id_claim_paths = claim_paths.into_iter().collect();
        self
    }
}

impl ApiTokenScheme {
    #[must_use]
    pub const fn bearer() -> Self {
        Self::Bearer
    }

    pub fn header(name: impl Into<String>) -> Result<Self, HeaderNameParseError> {
        Ok(Self::Header {
            name: HeaderName::parse(name)?,
        })
    }
}

impl ApiTokenSource {
    pub fn literal(token: impl Into<String>) -> Result<Self, TokenParseError> {
        Ok(Self::Literal {
            token: Token::parse(token)?,
        })
    }

    pub fn env(variable: impl Into<String>) -> Result<Self, EnvironmentVariableParseError> {
        Ok(Self::Environment {
            variable: EnvironmentVariable::parse(variable)?,
        })
    }
}

impl Auth {
    #[must_use]
    pub fn api_token(source: ApiTokenSource) -> Self {
        Self::ApiToken { source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_composes_auth_methods_without_duplicate_state() {
        let oauth = OAuthMethod::authorization_code(
            "https://example.com/authorize",
            "https://example.com/token",
            "client",
        )
        .unwrap();
        let definition = Definition::api_token(ApiTokenMethod::bearer()).with_oauth(oauth);

        assert_eq!(
            definition.kinds(),
            vec![AuthKind::ApiToken, AuthKind::OAuth]
        );
    }

    #[test]
    fn oauth_policy_is_provider_data_not_provider_runtime_code() {
        let oauth = OAuthMethod::authorization_code(
            "https://example.com/authorize",
            "https://example.com/token",
            "client",
        )
        .unwrap()
        .with_scope("openid")
        .with_authorization_param("audience", "example")
        .with_callback_ports([1455, 1457])
        .with_static_header("originator", "phenix")
        .with_account_id_header(
            "Example-Account-ID",
            [vec!["account_id".to_owned()]],
        );
        assert_eq!(oauth.scopes, vec!["openid"]);
        assert_eq!(oauth.callback_ports, vec![1455, 1457]);
        assert_eq!(oauth.account_id_header.as_deref(), Some("Example-Account-ID"));
    }

    #[test]
    fn api_token_credentials_parse_at_construction() {
        assert!(matches!(
            ApiToken::env("OPENAI_API_KEY").unwrap(),
            ApiTokenSource::Environment { .. }
        ));
        assert!(ApiToken::env("not-valid").is_err());
        assert!(matches!(
            ApiToken::literal("secret").unwrap(),
            ApiTokenSource::Literal { .. }
        ));
    }
}
