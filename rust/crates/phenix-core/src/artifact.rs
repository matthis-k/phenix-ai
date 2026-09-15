use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

const SHA256_PREFIX: &str = "sha256:";
const SHA256_HEX_LENGTH: usize = 64;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ArtifactRevision(String);

impl ArtifactRevision {
    #[must_use]
    pub fn from_content(content: &[u8]) -> Self {
        Self(format!("{SHA256_PREFIX}{:x}", Sha256::digest(content)))
    }
}

impl AsRef<str> for ArtifactRevision {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for ArtifactRevision {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactRevisionParseError;

impl Display for ArtifactRevisionParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("artifact revision must be sha256 followed by 64 lowercase hexadecimal digits")
    }
}

impl Error for ArtifactRevisionParseError {}

impl FromStr for ArtifactRevision {
    type Err = ArtifactRevisionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(digest) = value.strip_prefix(SHA256_PREFIX) else {
            return Err(ArtifactRevisionParseError);
        };
        if digest.len() != SHA256_HEX_LENGTH
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ArtifactRevisionParseError);
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for ArtifactRevision {
    type Error = ArtifactRevisionParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl<'de> Deserialize<'de> for ArtifactRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_is_the_canonical_sha256_content_identity() {
        let revision = ArtifactRevision::from_content(b"fixture");

        assert_eq!(
            revision.as_ref(),
            "sha256:f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d"
        );
        assert_eq!(revision.to_string().parse(), Ok(revision));
    }

    #[test]
    fn parsing_and_deserialization_reject_noncanonical_revisions() {
        for invalid in [
            "f16b903569804677cefb5bd7f50cfc83342da38badef5c4e1c1e401b7c158221",
            "sha256:fixture",
            "sha256:F16B903569804677CEFB5BD7F50CFC83342DA38BADEF5C4E1C1E401B7C158221",
        ] {
            assert!(invalid.parse::<ArtifactRevision>().is_err());
            assert!(serde_json::from_value::<ArtifactRevision>(invalid.into()).is_err());
        }
    }
}
