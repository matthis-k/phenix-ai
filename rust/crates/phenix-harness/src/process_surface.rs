use phenix_core::{
    Authority, ConfigContribution, ConfigContributionSource, ConfigNamespace, ConfigurationFrontendId,
    PhenixValue, PluginId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};

const PROCESS_CLI_FRONTEND: &str = "phenix-config-process-cli";
const PROCESS_CLI_PRECEDENCE: i32 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessArgumentValueKind {
    Bool,
    String,
    U64,
    I64,
    F64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessConfigBinding {
    pub namespace: ConfigNamespace,
    pub contract_version: u64,
    pub field: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessArgumentContribution {
    pub owner: PluginId,
    pub long_name: String,
    pub description: String,
    pub value_kind: ProcessArgumentValueKind,
    pub binding: ProcessConfigBinding,
}

impl ProcessArgumentContribution {
    pub fn validate(&self) -> Result<(), ProcessSurfaceError> {
        validate_long_name(&self.long_name)?;
        if self.binding.contract_version == 0 {
            return Err(ProcessSurfaceError::InvalidContribution {
                argument: self.long_name.clone(),
                message: "configuration contract version must be positive".into(),
            });
        }
        if self.binding.field.is_empty()
            || !self.binding.field.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
            })
        {
            return Err(ProcessSurfaceError::InvalidContribution {
                argument: self.long_name.clone(),
                message: "configuration field must be a non-empty simple path".into(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessSurfaceMetadata {
    #[serde(default)]
    pub arguments: Vec<ProcessArgumentContribution>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedProcessArguments {
    pub values: BTreeMap<String, PhenixValue>,
    pub contributions: Vec<ConfigContribution>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessSurfaceError {
    InvalidContribution { argument: String, message: String },
    DuplicateArgument { argument: String, owners: Vec<PluginId> },
    UnknownArgument(String),
    MissingValue(String),
    InvalidValue {
        argument: String,
        expected: ProcessArgumentValueKind,
        value: String,
    },
    DuplicateValue(String),
    ConflictingBinding {
        namespace: ConfigNamespace,
        field: String,
    },
}

impl Display for ProcessSurfaceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidContribution { argument, message } => {
                write!(formatter, "invalid process argument --{argument}: {message}")
            }
            Self::DuplicateArgument { argument, owners } => write!(
                formatter,
                "process argument --{argument} is contributed by multiple loaded plugins: {}",
                owners
                    .iter()
                    .map(PluginId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::UnknownArgument(argument) => {
                write!(formatter, "unknown process argument: {argument}")
            }
            Self::MissingValue(argument) => {
                write!(formatter, "process argument {argument} requires a value")
            }
            Self::InvalidValue {
                argument,
                expected,
                value,
            } => write!(
                formatter,
                "invalid value {value:?} for {argument}; expected {expected:?}"
            ),
            Self::DuplicateValue(argument) => {
                write!(formatter, "process argument {argument} was provided more than once")
            }
            Self::ConflictingBinding { namespace, field } => write!(
                formatter,
                "multiple process arguments bind conflicting values to {}:{field}",
                namespace.as_str()
            ),
        }
    }
}

impl std::error::Error for ProcessSurfaceError {}

#[derive(Clone, Debug, Default)]
pub struct ProcessSurface {
    contributions: Vec<ProcessArgumentContribution>,
}

impl ProcessSurface {
    pub fn add(&mut self, contribution: ProcessArgumentContribution) -> Result<(), ProcessSurfaceError> {
        contribution.validate()?;
        self.contributions.push(contribution);
        Ok(())
    }

    pub fn extend(
        &mut self,
        metadata: ProcessSurfaceMetadata,
    ) -> Result<(), ProcessSurfaceError> {
        for argument in metadata.arguments {
            self.add(argument)?;
        }
        Ok(())
    }

    pub fn active_arguments<'a>(
        &'a self,
        loaded_plugins: &'a BTreeSet<PluginId>,
    ) -> Result<BTreeMap<&'a str, &'a ProcessArgumentContribution>, ProcessSurfaceError> {
        let mut active = BTreeMap::<&str, &ProcessArgumentContribution>::new();
        for contribution in &self.contributions {
            if !loaded_plugins.contains(&contribution.owner) {
                continue;
            }
            if let Some(existing) = active.insert(&contribution.long_name, contribution) {
                let mut owners = vec![existing.owner.clone(), contribution.owner.clone()];
                owners.sort();
                owners.dedup();
                return Err(ProcessSurfaceError::DuplicateArgument {
                    argument: contribution.long_name.clone(),
                    owners,
                });
            }
        }
        Ok(active)
    }

    pub fn parse(
        &self,
        loaded_plugins: &BTreeSet<PluginId>,
        arguments: &[String],
    ) -> Result<ParsedProcessArguments, ProcessSurfaceError> {
        let active = self.active_arguments(loaded_plugins)?;
        let mut values = BTreeMap::new();
        let mut grouped = BTreeMap::<(ConfigNamespace, u64), BTreeMap<String, PhenixValue>>::new();
        let mut index = 0;
        while index < arguments.len() {
            let raw = &arguments[index];
            let Some(long_name) = raw.strip_prefix("--") else {
                return Err(ProcessSurfaceError::UnknownArgument(raw.clone()));
            };
            let contribution = active
                .get(long_name)
                .copied()
                .ok_or_else(|| ProcessSurfaceError::UnknownArgument(raw.clone()))?;
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| ProcessSurfaceError::MissingValue(raw.clone()))?;
            if value.starts_with("--") {
                return Err(ProcessSurfaceError::MissingValue(raw.clone()));
            }
            let parsed = parse_value(raw, contribution.value_kind, value)?;
            if values.insert(long_name.to_owned(), parsed.clone()).is_some() {
                return Err(ProcessSurfaceError::DuplicateValue(raw.clone()));
            }

            let key = (
                contribution.binding.namespace.clone(),
                contribution.binding.contract_version,
            );
            let fields = grouped.entry(key).or_default();
            if fields
                .insert(contribution.binding.field.clone(), parsed)
                .is_some()
            {
                return Err(ProcessSurfaceError::ConflictingBinding {
                    namespace: contribution.binding.namespace.clone(),
                    field: contribution.binding.field.clone(),
                });
            }
            index += 2;
        }

        let revision_payload = serde_json::to_vec(&values)
            .expect("canonical process argument values serialize");
        let source_revision = format!("sha256:{:x}", Sha256::digest(revision_payload));
        let frontend = ConfigurationFrontendId::parse(PROCESS_CLI_FRONTEND)
            .expect("static process CLI frontend id is valid");
        let contributions = grouped
            .into_iter()
            .map(|((namespace, contract_version), fields)| ConfigContribution {
                source: ConfigContributionSource {
                    frontend: frontend.clone(),
                    source_identity: "process-argv".into(),
                    source_revision: source_revision.clone(),
                },
                namespace,
                contract_version,
                precedence: PROCESS_CLI_PRECEDENCE,
                value: serde_json::Value::Object(
                    fields
                        .into_iter()
                        .map(|(field, value)| (field, serde_json::Value::from(value)))
                        .collect(),
                )
                .into(),
                requested_authority: Authority::default(),
            })
            .collect();

        Ok(ParsedProcessArguments {
            values,
            contributions,
        })
    }

    pub fn help(
        &self,
        loaded_plugins: &BTreeSet<PluginId>,
    ) -> Result<Vec<String>, ProcessSurfaceError> {
        Ok(self
            .active_arguments(loaded_plugins)?
            .values()
            .map(|argument| {
                format!(
                    "--{} <{:?}>\t{} [{}]",
                    argument.long_name,
                    argument.value_kind,
                    argument.description,
                    argument.owner
                )
            })
            .collect())
    }
}

fn validate_long_name(value: &str) -> Result<(), ProcessSurfaceError> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(ProcessSurfaceError::InvalidContribution {
            argument: value.to_owned(),
            message: "long option name contains unsupported characters".into(),
        });
    }
    Ok(())
}

fn parse_value(
    argument: &str,
    kind: ProcessArgumentValueKind,
    value: &str,
) -> Result<PhenixValue, ProcessSurfaceError> {
    let invalid = || ProcessSurfaceError::InvalidValue {
        argument: argument.to_owned(),
        expected: kind,
        value: value.to_owned(),
    };
    match kind {
        ProcessArgumentValueKind::Bool => value.parse::<bool>().map(PhenixValue::Bool).map_err(|_| invalid()),
        ProcessArgumentValueKind::String => Ok(PhenixValue::String(value.to_owned())),
        ProcessArgumentValueKind::U64 => value.parse::<u64>().map(PhenixValue::U64).map_err(|_| invalid()),
        ProcessArgumentValueKind::I64 => value.parse::<i64>().map(PhenixValue::I64).map_err(|_| invalid()),
        ProcessArgumentValueKind::F64 => value.parse::<f64>().map(PhenixValue::F64).map_err(|_| invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn contribution(owner: &str) -> ProcessArgumentContribution {
        ProcessArgumentContribution {
            owner: plugin(owner),
            long_name: "plugin-handled-value".into(),
            description: "Fixture plugin value".into(),
            value_kind: ProcessArgumentValueKind::U64,
            binding: ProcessConfigBinding {
                namespace: ConfigNamespace::parse("fixture.process@1").unwrap(),
                contract_version: 1,
                field: "value".into(),
            },
        }
    }

    #[test]
    fn loaded_plugin_extends_the_process_surface() {
        let mut surface = ProcessSurface::default();
        surface.add(contribution("fixture.plugin")).unwrap();
        let loaded = BTreeSet::from([plugin("fixture.plugin")]);
        let parsed = surface
            .parse(
                &loaded,
                &["--plugin-handled-value".into(), "7".into()],
            )
            .unwrap();

        assert_eq!(
            parsed.values.get("plugin-handled-value"),
            Some(&PhenixValue::U64(7))
        );
        assert_eq!(parsed.contributions.len(), 1);
        assert_eq!(
            parsed.contributions[0].namespace.as_str(),
            "fixture.process@1"
        );
    }

    #[test]
    fn absent_plugin_removes_its_argument_from_the_viable_surface() {
        let mut surface = ProcessSurface::default();
        surface.add(contribution("fixture.plugin")).unwrap();
        let error = surface
            .parse(
                &BTreeSet::new(),
                &["--plugin-handled-value".into(), "7".into()],
            )
            .unwrap_err();
        assert_eq!(
            error,
            ProcessSurfaceError::UnknownArgument("--plugin-handled-value".into())
        );
    }

    #[test]
    fn duplicate_loaded_owners_are_rejected() {
        let mut surface = ProcessSurface::default();
        surface.add(contribution("fixture.first")).unwrap();
        surface.add(contribution("fixture.second")).unwrap();
        let loaded = BTreeSet::from([plugin("fixture.first"), plugin("fixture.second")]);
        assert!(matches!(
            surface.active_arguments(&loaded),
            Err(ProcessSurfaceError::DuplicateArgument { .. })
        ));
    }
}
