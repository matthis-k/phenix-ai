use bytes::Bytes;
use http::{
    header::{HeaderMap, HeaderName as HttpHeaderName, HeaderValue},
    Method, StatusCode,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt::{self, Formatter},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EndpointParseError {
    #[error("invalid provider endpoint: {0}")]
    InvalidUrl(String),
    #[error("provider endpoint scheme must be http or https, got {0}")]
    UnsupportedScheme(String),
    #[error("provider endpoint must not contain embedded credentials")]
    CredentialsNotAllowed,
    #[error("provider endpoint must not contain a query")]
    QueryNotAllowed,
    #[error("provider endpoint must not contain a fragment")]
    FragmentNotAllowed,
}

fn parse_http_url(value: &str) -> Result<Url, EndpointParseError> {
    let url =
        Url::parse(value).map_err(|error| EndpointParseError::InvalidUrl(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(EndpointParseError::UnsupportedScheme(
            url.scheme().to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(EndpointParseError::CredentialsNotAllowed);
    }
    if url.query().is_some() {
        return Err(EndpointParseError::QueryNotAllowed);
    }
    if url.fragment().is_some() {
        return Err(EndpointParseError::FragmentNotAllowed);
    }
    Ok(url)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, EndpointParseError> {
        let mut url = parse_http_url(value.as_ref())?;
        if !url.path().ends_with('/') {
            let mut path = url.path().to_owned();
            path.push('/');
            url.set_path(&path);
        }
        Ok(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn join(&self, path: &str) -> Result<Url, ProviderError> {
        Url::parse(&self.0)
            .expect("parsed provider endpoint remains valid")
            .join(path)
            .map_err(|error| ProviderError::Protocol {
                message: format!("cannot join provider endpoint path {path:?}: {error}"),
            })
    }
}

impl Serialize for Endpoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Endpoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Secret(String);

impl Secret {
    pub fn parse(value: impl Into<String>) -> Result<Self, SecretParseError> {
        let value = value.into();
        if value.is_empty() {
            Err(SecretParseError::Empty)
        } else {
            Ok(Self(value))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SecretParseError {
    #[error("secret must not be empty")]
    Empty,
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Token(String);

impl Token {
    pub fn parse(value: impl Into<String>) -> Result<Self, TokenParseError> {
        let value = value.into();
        if value.is_empty() {
            return Err(TokenParseError::Empty);
        }
        HeaderValue::from_str(&value).map_err(|_| TokenParseError::InvalidHeaderValue)?;
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TokenParseError {
    #[error("token must not be empty")]
    Empty,
    #[error("token is not valid in an HTTP header")]
    InvalidHeaderValue,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Serialize for Token {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct EnvironmentVariable(String);

impl EnvironmentVariable {
    pub fn parse(value: impl Into<String>) -> Result<Self, EnvironmentVariableParseError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let Some(first) = bytes.next() else {
            return Err(EnvironmentVariableParseError);
        };
        if !(first.is_ascii_alphabetic() || first == b'_')
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(EnvironmentVariableParseError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("environment variable name must match [A-Za-z_][A-Za-z0-9_]*")]
pub struct EnvironmentVariableParseError;

impl Serialize for EnvironmentVariable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EnvironmentVariable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApiTokenSource {
    Literal { token: Token },
    Environment { variable: EnvironmentVariable },
}

impl fmt::Debug for ApiTokenSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal { .. } => f
                .debug_struct("Literal")
                .field("token", &"<redacted>")
                .finish(),
            Self::Environment { variable } => f
                .debug_struct("Environment")
                .field("variable", variable)
                .finish(),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct HeaderName(String);

impl HeaderName {
    pub fn parse(value: impl Into<String>) -> Result<Self, HeaderNameParseError> {
        let value = value.into();
        HttpHeaderName::from_bytes(value.as_bytes()).map_err(|_| HeaderNameParseError)?;
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid HTTP header name")]
pub struct HeaderNameParseError;

impl Serialize for HeaderName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HeaderName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApiTokenScheme {
    Bearer,
    Header { name: HeaderName },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    ApiToken,
    OAuth,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Auth {
    ApiToken {
        source: ApiTokenSource,
    },
    OAuth {
        access_token: Token,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<Secret>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
    },
}

impl Auth {
    pub fn kind(&self) -> AuthKind {
        match self {
            Self::ApiToken { .. } => AuthKind::ApiToken,
            Self::OAuth { .. } => AuthKind::OAuth,
        }
    }

    pub fn descriptor(&self) -> AuthDescriptor {
        AuthDescriptor {
            kind: self.kind(),
            expires_at: match self {
                Self::OAuth { expires_at, .. } => *expires_at,
                Self::ApiToken { .. } => None,
            },
        }
    }

    pub(crate) fn is_expired(&self) -> bool {
        let Self::OAuth {
            expires_at: Some(expires_at),
            ..
        } = self
        else {
            return false;
        };
        unix_now() >= *expires_at
    }
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiToken { source } => {
                f.debug_struct("ApiToken").field("source", source).finish()
            }
            Self::OAuth {
                refresh_token,
                expires_at,
                ..
            } => f
                .debug_struct("OAuth")
                .field("access_token", &"<redacted>")
                .field(
                    "refresh_token",
                    &refresh_token.as_ref().map(|_| "<redacted>"),
                )
                .field("expires_at", expires_at)
                .finish(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthDescriptor {
    pub kind: AuthKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DurationMs(pub u64);

impl DurationMs {
    pub fn from_duration(duration: Duration) -> Self {
        Self(duration.as_millis().try_into().unwrap_or(u64::MAX))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_after: Option<DurationMs>,
}

impl RateLimitWindow {
    pub(crate) fn is_empty(&self) -> bool {
        self.limit.is_none() && self.remaining.is_none() && self.reset_after.is_none()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<RateLimitWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<RateLimitWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<DurationMs>,
}

impl RateLimits {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let requests = rate_limit_window(
            headers,
            &["ratelimit-limit", "x-ratelimit-limit-requests"],
            &["ratelimit-remaining", "x-ratelimit-remaining-requests"],
            &["ratelimit-reset", "x-ratelimit-reset-requests"],
        );
        let tokens = rate_limit_window(
            headers,
            &["x-ratelimit-limit-tokens"],
            &["x-ratelimit-remaining-tokens"],
            &["x-ratelimit-reset-tokens"],
        );
        Self {
            requests: (!requests.is_empty()).then_some(requests),
            tokens: (!tokens.is_empty()).then_some(tokens),
            retry_after: header(headers, &["retry-after"]).and_then(parse_reset),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_none() && self.tokens.is_none() && self.retry_after.is_none()
    }
}

fn rate_limit_window(
    headers: &HeaderMap,
    limit: &[&str],
    remaining: &[&str],
    reset: &[&str],
) -> RateLimitWindow {
    RateLimitWindow {
        limit: header(headers, limit).and_then(|value| value.parse().ok()),
        remaining: header(headers, remaining).and_then(|value| value.parse().ok()),
        reset_after: header(headers, reset).and_then(parse_reset),
    }
}

fn header<'a>(headers: &'a HeaderMap, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| headers.get(*name).and_then(|value| value.to_str().ok()))
}

fn parse_reset(value: &str) -> Option<DurationMs> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        if seconds > 1_000_000_000 {
            return Some(DurationMs(
                seconds.saturating_sub(unix_now()).saturating_mul(1000),
            ));
        }
        return Some(DurationMs(seconds.saturating_mul(1000)));
    }
    parse_compound_duration(value).map(DurationMs::from_duration)
}

fn parse_compound_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut total_ms = 0_u64;
    while index < bytes.len() {
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if start == index {
            return None;
        }
        let number = value[start..index].parse::<u64>().ok()?;
        let (factor, consumed) = if value[index..].starts_with("ms") {
            (1_u64, 2)
        } else if value[index..].starts_with('s') {
            (1000, 1)
        } else if value[index..].starts_with('m') {
            (60_000, 1)
        } else if value[index..].starts_with('h') {
            (3_600_000, 1)
        } else {
            return None;
        };
        total_ms = total_ms.saturating_add(number.saturating_mul(factor));
        index += consumed;
    }
    Some(Duration::from_millis(total_ms))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderError {
    #[error("authentication: {message}")]
    Authentication { message: String },
    #[error("permission: {message}")]
    Permission { message: String },
    #[error("not_found: {message}")]
    NotFound { message: String },
    #[error("rate_limited: {message}")]
    RateLimited {
        message: String,
        limits: Box<RateLimits>,
    },
    #[error("context_limit: {message}")]
    ContextLimit { message: String },
    #[error("invalid_request: {message}")]
    InvalidRequest { message: String },
    #[error("unavailable: {message}")]
    Unavailable { message: String },
    #[error("transport: {message}")]
    Transport { message: String },
    #[error("protocol: {message}")]
    Protocol { message: String },
}

impl ProviderError {
    pub fn to_wire(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parse_is_the_validation_boundary() {
        assert_eq!(
            Endpoint::parse("https://example.com/v1").unwrap().as_str(),
            "https://example.com/v1/"
        );
        assert!(matches!(
            Endpoint::parse("file:///tmp/model"),
            Err(EndpointParseError::UnsupportedScheme(_))
        ));
        assert_eq!(
            Endpoint::parse("https://user@example.com/v1").unwrap_err(),
            EndpointParseError::CredentialsNotAllowed
        );
    }

    #[test]
    fn auth_parse_rejects_impossible_runtime_tokens() {
        assert!(serde_json::from_value::<Auth>(serde_json::json!({
            "type":"api_token",
            "source":{"type":"literal","token":""}
        }))
        .is_err());
        assert!(serde_json::from_value::<Auth>(serde_json::json!({
            "type":"api_token",
            "source":{"type":"environment","variable":"1INVALID"}
        }))
        .is_err());
        assert!(serde_json::from_value::<Auth>(serde_json::json!({
            "type":"oauth",
            "access_token":"bad\nvalue"
        }))
        .is_err());
    }

    #[test]
    fn common_rate_limit_headers_are_normalized() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ratelimit-limit-requests",
            HeaderValue::from_static("100"),
        );
        headers.insert(
            "x-ratelimit-remaining-requests",
            HeaderValue::from_static("0"),
        );
        headers.insert("x-ratelimit-reset-requests", HeaderValue::from_static("1s"));
        headers.insert("x-ratelimit-limit-tokens", HeaderValue::from_static("5000"));
        headers.insert(
            "x-ratelimit-reset-tokens",
            HeaderValue::from_static("500ms"),
        );
        headers.insert("retry-after", HeaderValue::from_static("2"));
        let limits = RateLimits::from_headers(&headers);
        assert_eq!(limits.requests.as_ref().unwrap().limit, Some(100));
        assert_eq!(
            limits.requests.as_ref().unwrap().reset_after,
            Some(DurationMs(1000))
        );
        assert_eq!(
            limits.tokens.as_ref().unwrap().reset_after,
            Some(DurationMs(500))
        );
        assert_eq!(limits.retry_after, Some(DurationMs(2000)));
    }
}
