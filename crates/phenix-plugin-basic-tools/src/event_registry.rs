//! Generation-local tool declaration staging. Execution authority never
//! comes from event payloads: every external registration is checked against
//! the manager's already resolver-selected ToolRegistrations imports.
use phenix_basic_contracts::{error, BasicError, ErrorCode, ToolInfo};
use std::collections::BTreeMap;

const MAX_TOOLS: usize = 256;
const MAX_CONTRIBUTORS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) info: ToolInfo,
    /// Resolver-selected import index, never supplied by the declaring plugin.
    /// None denotes an intrinsic tool owned by the manager.
    pub(crate) route: Option<usize>,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Collecting(BTreeMap<String, Entry>),
    Published,
    Aborted,
}

/// Staging and validation belong to the tool-manager plugin, not Storage.
#[derive(Default)]
pub(crate) struct Registry {
    phase: Phase,
}

fn reject(message: &str) -> BasicError {
    error(ErrorCode::InvalidInput, message)
}

fn valid(tool: &ToolInfo) -> bool {
    !tool.id.is_empty()
        && tool.id.len() <= 128
        && tool
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !tool.description.is_empty()
        && tool.description.len() <= 4096
        && tool.arguments_schema.len() <= 16384
        && serde_json::from_str::<serde_json::Value>(&tool.arguments_schema)
            .is_ok_and(|schema| schema.is_object())
}

impl Registry {
    pub(crate) fn begin(
        &mut self,
        builtin: impl IntoIterator<Item = ToolInfo>,
    ) -> Result<(), BasicError> {
        if !matches!(self.phase, Phase::Idle) {
            return Err(error(
                ErrorCode::Conflict,
                "tool population already started",
            ));
        }
        let mut entries = BTreeMap::new();
        for info in builtin {
            if !valid(&info)
                || entries.len() >= MAX_TOOLS
                || entries
                    .insert(info.id.clone(), Entry { info, route: None })
                    .is_some()
            {
                return Err(reject("invalid or duplicate intrinsic tool"));
            }
        }
        self.phase = Phase::Collecting(entries);
        Ok(())
    }

    /// Catalogs are obtained through kernel-selected `Contributors#index`
    /// imports, never through a claim in the registration request. Every
    /// claimed tool must occur in exactly one selected contributor's catalog,
    /// with byte-for-byte equal metadata. No arbitrary executor IDs.
    pub(crate) fn register(
        &mut self,
        info: ToolInfo,
        selected_catalogs: &[Vec<ToolInfo>],
    ) -> Result<(), BasicError> {
        let Phase::Collecting(entries) = &mut self.phase else {
            return Err(error(
                ErrorCode::Conflict,
                "tool population is not collecting",
            ));
        };
        if !valid(&info)
            || selected_catalogs.len() > MAX_CONTRIBUTORS
            || entries.len() >= MAX_TOOLS
            || entries.contains_key(&info.id)
        {
            return Err(reject("invalid or conflicting tool registration"));
        }
        let mut selected = None;
        for (index, catalog) in selected_catalogs.iter().enumerate() {
            if catalog.len() > MAX_TOOLS {
                return Err(reject("oversized selected tool contributor"));
            }
            if catalog.iter().any(|candidate| candidate.id == info.id) {
                if catalog
                    .iter()
                    .filter(|candidate| candidate.id == info.id)
                    .count()
                    != 1
                    || !catalog.contains(&info)
                    || selected.replace(index).is_some()
                {
                    return Err(reject("unverified or ambiguous selected tool route"));
                }
            }
        }
        let index = selected.ok_or_else(|| reject("tool has no selected executor"))?;
        entries.insert(
            info.id.clone(),
            Entry {
                info,
                route: Some(index),
            },
        );
        Ok(())
    }

    /// A previously accepted event contribution must be verified to match
    /// the same resolver-selected executor when the owner seals the wave.
    pub(crate) fn find(&self, id: &str) -> Option<&Entry> {
        match &self.phase {
            Phase::Collecting(entries) => entries.get(id),
            _ => None,
        }
    }

    /// Inspect the complete staged candidate without publishing it. Used to
    /// compute the same immutable revision advertised by the ready event.
    pub(crate) fn preview(&self) -> Result<Vec<Entry>, BasicError> {
        match &self.phase {
            Phase::Collecting(entries) => Ok(entries.values().cloned().collect()),
            _ => Err(error(
                ErrorCode::Conflict,
                "tool population is not collecting",
            )),
        }
    }

    pub(crate) fn publish(&mut self) -> Result<Vec<Entry>, BasicError> {
        let Phase::Collecting(entries) = &self.phase else {
            return Err(error(ErrorCode::Conflict, "tool population cannot publish"));
        };
        let result = entries.values().cloned().collect();
        self.phase = Phase::Published;
        Ok(result)
    }

    pub(crate) fn abort(&mut self) -> Result<(), BasicError> {
        if !matches!(self.phase, Phase::Collecting(_)) {
            return Err(error(ErrorCode::Conflict, "tool population is not active"));
        }
        self.phase = Phase::Aborted;
        Ok(())
    }

    pub(crate) fn blocks_pull_catalog(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str) -> ToolInfo {
        ToolInfo {
            id: id.into(),
            description: format!("{id} operation"),
            arguments_schema: r#"{"type":"string"}"#.into(),
            idempotent: true,
        }
    }

    #[test]
    fn selected_route_is_derived_from_real_contributor_not_payload() {
        let external = info("fixture.reverse");
        let mut registry = Registry::default();
        registry.begin([info("uppercase")]).unwrap();
        assert!(registry.blocks_pull_catalog());
        assert_eq!(
            registry
                .register(external.clone(), &[vec![info("foreign")]])
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        registry
            .register(
                external.clone(),
                &[vec![info("foreign")], vec![external.clone()]],
            )
            .unwrap();
        let published = registry.publish().unwrap();
        assert_eq!(published.len(), 2);
        assert_eq!(published[0].info.id, "fixture.reverse");
        assert_eq!(published[0].route, Some(1));
        assert_eq!(published[1].route, None);
        assert_eq!(
            registry.register(info("after"), &[]).unwrap_err().code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn forgery_ambiguity_and_duplicates_never_mutate_snapshot() {
        let reverse = info("fixture.reverse");
        let mut registry = Registry::default();
        registry.begin([info("uppercase")]).unwrap();
        let mut forged = reverse.clone();
        forged.description.push_str(" forged");
        assert!(registry.register(forged, &[vec![reverse.clone()]]).is_err());
        assert!(registry
            .register(
                reverse.clone(),
                &[vec![reverse.clone()], vec![reverse.clone()]]
            )
            .is_err());
        registry
            .register(reverse.clone(), &[vec![reverse.clone()]])
            .unwrap();
        assert!(registry
            .register(reverse.clone(), &[vec![reverse]])
            .is_err());
        assert_eq!(registry.publish().unwrap().len(), 2);
    }

    #[test]
    fn abort_is_terminal_and_invalid_data_is_rejected() {
        let mut registry = Registry::default();
        registry.begin([info("uppercase")]).unwrap();
        let mut malformed = info("unsafe!");
        malformed.arguments_schema = "[]".into();
        assert!(registry.register(malformed, &[]).is_err());
        registry.abort().unwrap();
        assert!(registry.blocks_pull_catalog());
        assert!(registry.begin([info("retry")]).is_err());
        assert!(registry.publish().is_err());
    }

    #[test]
    fn no_external_contributors_can_publish_builtin_tools() {
        let mut registry = Registry::default();
        registry.begin([info("uppercase"), info("length")]).unwrap();
        let catalog = registry.publish().unwrap();
        assert_eq!(
            catalog
                .iter()
                .map(|e| e.info.id.as_str())
                .collect::<Vec<_>>(),
            ["length", "uppercase"]
        );
    }
}
