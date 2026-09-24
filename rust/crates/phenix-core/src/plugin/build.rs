use crate::{Authority, PhenixValue, PluginArtifact};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginBuildPlanError {
    EmptyValue(&'static str),
    ContainsNul(&'static str),
    InvalidEnvironmentName,
    InvalidRelativePath(&'static str),
    EmptySteps,
}

impl Display for PluginBuildPlanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyValue(field) => write!(f, "{field} must not be empty"),
            Self::ContainsNul(field) => write!(f, "{field} must not contain NUL"),
            Self::InvalidEnvironmentName => f.write_str("invalid build environment variable name"),
            Self::InvalidRelativePath(field) => {
                write!(f, "{field} must be a normalized relative path")
            }
            Self::EmptySteps => f.write_str("plugin build plan must contain at least one step"),
        }
    }
}

impl Error for PluginBuildPlanError {}

macro_rules! validated_string {
    ($name:ident, $validator:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = PluginBuildPlanError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                $validator(value)?;
                Ok(Self(value.to_owned()))
            }
        }

        impl TryFrom<String> for $name {
            type Error = PluginBuildPlanError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                $validator(&value)?;
                Ok(Self(value))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

fn validate_executable(value: &str) -> Result<(), PluginBuildPlanError> {
    validate_nonempty(value, "build executable")
}

fn validate_argument(value: &str) -> Result<(), PluginBuildPlanError> {
    validate_no_nul(value, "build argument")
}

fn validate_source_identity(value: &str) -> Result<(), PluginBuildPlanError> {
    validate_nonempty(value, "build source identity")
}

fn validate_source_revision(value: &str) -> Result<(), PluginBuildPlanError> {
    validate_nonempty(value, "build source revision")
}

fn validate_environment_name(value: &str) -> Result<(), PluginBuildPlanError> {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return Err(PluginBuildPlanError::InvalidEnvironmentName);
    };
    if !(first.is_ascii_alphabetic() || first == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(PluginBuildPlanError::InvalidEnvironmentName);
    }
    Ok(())
}

fn validate_nonempty(value: &str, field: &'static str) -> Result<(), PluginBuildPlanError> {
    if value.trim().is_empty() {
        return Err(PluginBuildPlanError::EmptyValue(field));
    }
    validate_no_nul(value, field)
}

fn validate_no_nul(value: &str, field: &'static str) -> Result<(), PluginBuildPlanError> {
    if value.contains('\0') {
        return Err(PluginBuildPlanError::ContainsNul(field));
    }
    Ok(())
}

validated_string!(BuildExecutable, validate_executable);
validated_string!(BuildArgument, validate_argument);
validated_string!(BuildSourceIdentity, validate_source_identity);
validated_string!(BuildSourceRevision, validate_source_revision);
validated_string!(BuildEnvironmentName, validate_environment_name);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BuildWorkingDirectory(String);

impl BuildWorkingDirectory {
    pub fn root() -> Self {
        Self(".".into())
    }
}

impl AsRef<str> for BuildWorkingDirectory {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for BuildWorkingDirectory {
    type Err = PluginBuildPlanError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_relative_path(value, true, "build working directory")?;
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for BuildWorkingDirectory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BuildArtifactOutput(String);

impl AsRef<str> for BuildArtifactOutput {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for BuildArtifactOutput {
    type Err = PluginBuildPlanError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_relative_path(value, false, "build artifact output")?;
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for BuildArtifactOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

fn validate_relative_path(
    value: &str,
    allow_root: bool,
    field: &'static str,
) -> Result<(), PluginBuildPlanError> {
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\\')
        || value.as_bytes().get(1) == Some(&b':')
    {
        return Err(PluginBuildPlanError::InvalidRelativePath(field));
    }
    if allow_root && value == "." {
        return Ok(());
    }
    if value
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(PluginBuildPlanError::InvalidRelativePath(field));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BuildEnvironment(BTreeMap<BuildEnvironmentName, String>);

impl BuildEnvironment {
    pub fn new(
        values: impl IntoIterator<Item = (BuildEnvironmentName, String)>,
    ) -> Result<Self, PluginBuildPlanError> {
        let values: BTreeMap<_, _> = values.into_iter().collect();
        if values.values().any(|value| value.contains('\0')) {
            return Err(PluginBuildPlanError::ContainsNul("build environment value"));
        }
        Ok(Self(values))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BuildEnvironmentName, &str)> {
        self.0.iter().map(|(name, value)| (name, value.as_str()))
    }
}

impl<'de> Deserialize<'de> for BuildEnvironment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeMap::<BuildEnvironmentName, String>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginBuildSource {
    pub identity: BuildSourceIdentity,
    pub revision: BuildSourceRevision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginBuildStep {
    pub executable: BuildExecutable,
    pub argv: Vec<BuildArgument>,
    pub working_directory: BuildWorkingDirectory,
    #[serde(default)]
    pub environment: BuildEnvironment,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PluginBuildPlan {
    source: PluginBuildSource,
    steps: Vec<PluginBuildStep>,
    artifact_output: BuildArtifactOutput,
    configuration: BTreeMap<String, PhenixValue>,
    requested_authority: Authority,
}

impl PluginBuildPlan {
    pub fn new(
        source: PluginBuildSource,
        steps: Vec<PluginBuildStep>,
        artifact_output: BuildArtifactOutput,
        configuration: BTreeMap<String, PhenixValue>,
        requested_authority: Authority,
    ) -> Result<Self, PluginBuildPlanError> {
        if steps.is_empty() {
            return Err(PluginBuildPlanError::EmptySteps);
        }
        Ok(Self {
            source,
            steps,
            artifact_output,
            configuration,
            requested_authority,
        })
    }

    pub fn source(&self) -> &PluginBuildSource {
        &self.source
    }

    pub fn steps(&self) -> &[PluginBuildStep] {
        &self.steps
    }

    pub fn artifact_output(&self) -> &BuildArtifactOutput {
        &self.artifact_output
    }

    pub fn configuration(&self) -> &BTreeMap<String, PhenixValue> {
        &self.configuration
    }

    pub fn requested_authority(&self) -> &Authority {
        &self.requested_authority
    }
}

impl<'de> Deserialize<'de> for PluginBuildPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WirePlan {
            source: PluginBuildSource,
            steps: Vec<PluginBuildStep>,
            artifact_output: BuildArtifactOutput,
            #[serde(default)]
            configuration: BTreeMap<String, PhenixValue>,
            #[serde(default)]
            requested_authority: Authority,
        }

        let plan = WirePlan::deserialize(deserializer)?;
        Self::new(
            plan.source,
            plan.steps,
            plan.artifact_output,
            plan.configuration,
            plan.requested_authority,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "state", content = "artifact", rename_all = "snake_case")]
pub enum PluginArtifactInput {
    Ready(PluginArtifact),
    Build(PluginBuildPlan),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_plan_deserialization_preserves_argv_and_validates_paths_and_environment() {
        let plan: PluginBuildPlan = serde_json::from_value(serde_json::json!({
            "source": {"identity": "git:fixture", "revision": "abc123"},
            "steps": [{
                "executable": "compiler",
                "argv": ["--define=x; touch /tmp/nope", "$(ignored)"],
                "working_directory": "source/plugin",
                "environment": {"BUILD_MODE": "release;still-a-value"}
            }],
            "artifact_output": "dist/plugin.wasm",
            "configuration": {},
            "requested_authority": []
        }))
        .unwrap();

        assert_eq!(plan.steps()[0].executable.as_ref(), "compiler");
        assert_eq!(
            plan.steps()[0].argv[0].as_ref(),
            "--define=x; touch /tmp/nope"
        );
        assert_eq!(plan.steps()[0].working_directory.as_ref(), "source/plugin");
        assert_eq!(
            plan.steps()[0]
                .environment
                .iter()
                .next()
                .map(|(name, value)| (name.as_ref(), value)),
            Some(("BUILD_MODE", "release;still-a-value"))
        );

        let mut invalid = serde_json::to_value(plan).unwrap();
        invalid["artifact_output"] = "../plugin.wasm".into();
        assert!(serde_json::from_value::<PluginBuildPlan>(invalid).is_err());
    }
}
