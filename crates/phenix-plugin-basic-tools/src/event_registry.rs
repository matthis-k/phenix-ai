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

/// Immutable, generation-owned catalog. Execution routes remain indices
/// into resolver-selected, attested imports; no declaration can supply one.
pub(crate) struct Catalog {
    pub(crate) tools: Vec<ToolInfo>,
    pub(crate) routes: BTreeMap<String, Option<usize>>,
    pub(crate) revision: u64,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Collecting(BTreeMap<String, Entry>),
    Published(Catalog),
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
    /// Build a fully validated, published candidate for the transitional
    /// pull adapter. This function owns the entire candidate, so any
    /// missing/forged/ambiguous contributor leaves the caller's live Registry
    /// unchanged. The same begin/register/publish invariants protect event
    /// publication; there is no second catalog type.
    pub(crate) fn from_selected(
        builtin: impl IntoIterator<Item = ToolInfo>,
        catalogs: &[Vec<ToolInfo>],
    ) -> Result<Self, BasicError> {
        if catalogs.len() > MAX_CONTRIBUTORS {
            return Err(reject("too many selected tool contributors"));
        }
        let mut candidate = Self::default();
        candidate.begin(builtin)?;
        for catalog in catalogs {
            for offered in catalog {
                candidate.register(offered.clone(), catalogs)?;
            }
        }
        let tools = candidate
            .preview()?
            .into_iter()
            .map(|entry| entry.info)
            .collect::<Vec<_>>();
        let revision = super::revision(&tools, catalogs.len());
        candidate.publish(revision)?;
        Ok(candidate)
    }

    pub(crate) fn selected(&self) -> Option<&Catalog> {
        match &self.phase {
            Phase::Published(catalog) => Some(catalog),
            _ => None,
        }
    }

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
            // Check the selected catalog in one pass. Only one exact
            // authenticated description may provide a route; neither a
            // duplicated ID nor a mismatched definition is sufficient.
            let mut matches = catalog.iter().filter(|candidate| candidate.id == info.id);
            match (matches.next(), matches.next()) {
                (None, None) => {}
                (Some(found), None) if found == &info && selected.replace(index).is_none() => {}
                _ => return Err(reject("unverified or ambiguous selected tool route")),
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

    /// Move the validated staging map into the one immutable published
    /// snapshot. The caller computes the revision BEFORE committing and
    /// enqueues its ready event before this infallible transition.
    pub(crate) fn publish(&mut self, revision: u64) -> Result<&Catalog, BasicError> {
        if !matches!(&self.phase, Phase::Collecting(_)) {
            return Err(error(ErrorCode::Conflict, "tool population cannot publish"));
        }
        let Phase::Collecting(entries) = std::mem::replace(&mut self.phase, Phase::Aborted) else {
            unreachable!("phase was collecting");
        };
        let mut tools = Vec::with_capacity(entries.len());
        let mut routes = BTreeMap::new();
        for (id, entry) in entries {
            routes.insert(id, entry.route);
            tools.push(entry.info);
        }
        self.phase = Phase::Published(Catalog {
            tools,
            routes,
            revision,
        });
        Ok(self.selected().expect("catalog just published"))
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
        let published = registry.publish(7).unwrap();
        assert_eq!(published.tools.len(), 2);
        assert_eq!(published.tools[0].id, "fixture.reverse");
        assert_eq!(published.routes["fixture.reverse"], Some(1));
        assert_eq!(published.routes["uppercase"], None);
        assert_eq!(published.revision, 7);
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
        assert_eq!(registry.publish(1).unwrap().tools.len(), 2);
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
        assert!(registry.publish(1).is_err());
    }

    #[test]
    fn published_catalog_is_single_authority_and_failure_is_terminal() {
        let mut graph = Registry::default();
        assert!(graph.selected().is_none());
        graph.begin([info("uppercase")]).unwrap();
        assert!(graph.selected().is_none());
        let result = graph.publish(19).unwrap();
        assert_eq!(result.revision, 19);
        assert_eq!(result.tools[0].id, "uppercase");
        assert_eq!(graph.selected().unwrap().routes["uppercase"], None);
        assert!(graph.begin([info("length")]).is_err());
        assert!(graph.publish(2).is_err());

        // Direct pull admission uses begin/register/publish on this same
        // Registry type; a published generation cannot switch to graph mode.
        let mut pull = Registry::default();
        pull.begin([info("uppercase")]).unwrap();
        pull.publish(1).unwrap();
        assert_eq!(pull.selected().unwrap().revision, 1);
        assert!(pull.begin([info("length")]).is_err());

        let mut aborted = Registry::default();
        aborted.begin([info("uppercase")]).unwrap();
        aborted.abort().unwrap();
        assert!(aborted.selected().is_none());
        assert!(aborted.begin([info("length")]).is_err());
        assert!(aborted.publish(1).is_err());
    }

    #[test]
    fn legacy_candidate_rejects_forgery_without_publishing_partial_state() {
        let intrinsic = info("uppercase");
        let external = info("fixture.reverse");
        let invalid = vec![
            vec![external.clone()],
            vec![external.clone()],
        ];
        assert_eq!(
            Registry::from_selected([intrinsic.clone()], &invalid)
                .err()
                .unwrap()
                .code,
            ErrorCode::InvalidInput
        );
        let mut malformed = external.clone();
        malformed.arguments_schema = "[]".into();
        assert_eq!(
            Registry::from_selected([intrinsic.clone()], &[vec![malformed]])
                .err()
                .unwrap()
                .code,
            ErrorCode::InvalidInput
        );

        let good = Registry::from_selected([intrinsic], &[vec![external]]).unwrap();
        let snapshot = good.selected().unwrap();
        assert_eq!(snapshot.routes["fixture.reverse"], Some(0));
        assert_eq!(snapshot.routes["uppercase"], None);
        assert!(snapshot.revision > 0);
    }

    #[test]
    fn no_external_contributors_can_publish_builtin_tools() {
        let mut registry = Registry::default();
        registry.begin([info("uppercase"), info("length")]).unwrap();
        let catalog = registry.publish(1).unwrap();
        assert_eq!(
            catalog
                .tools
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["length", "uppercase"]
        );
    }
}
