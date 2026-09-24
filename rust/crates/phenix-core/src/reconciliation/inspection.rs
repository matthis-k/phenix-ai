use crate::{
    GraphGenerationId, GraphReconciler, ReconciliationPreview, ResolvedHarness,
    ResolvedHarnessError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateResolutionInspection {
    Resolved(ReconciliationPreview),
    Rejected {
        active_generation: GraphGenerationId,
        reason: ResolvedHarnessError,
    },
}

impl GraphReconciler {
    /// Converts canonical candidate resolution into inspectable development-mode evidence.
    ///
    /// Rejected candidates preserve the active generation and expose the exact resolver
    /// error. This method never activates or otherwise mutates the active graph.
    pub fn inspect_candidate_resolution(
        &self,
        candidate: Result<ResolvedHarness, ResolvedHarnessError>,
    ) -> CandidateResolutionInspection {
        match candidate {
            Ok(candidate) => {
                CandidateResolutionInspection::Resolved(self.preview_candidate(&candidate))
            }
            Err(reason) => CandidateResolutionInspection::Rejected {
                active_generation: self.active().generation().clone(),
                reason,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Authority, ComponentGraphError, ComponentId, ComponentManifest, PluginId};

    #[test]
    fn invalid_candidate_reports_rejection_without_mutating_the_active_generation() {
        let active = ResolvedHarness::resolve([], [], [], &Authority::default()).unwrap();
        let active_generation = active.generation().clone();
        let reconciler = GraphReconciler::new(active);
        let component = ComponentId::parse("fixture.component").unwrap();
        let owner = PluginId::parse("fixture.missing-owner").unwrap();

        let candidate = GraphReconciler::resolve_candidate(
            [],
            [ComponentManifest {
                listeners: Vec::new(),
                id: component.clone(),
                owner: owner.clone(),
                imports: Vec::new(),
                exports: Vec::new(),
                maximum_authority: Authority::default(),
            }],
            [],
            &Authority::default(),
        );
        let inspection = reconciler.inspect_candidate_resolution(candidate);

        assert_eq!(reconciler.active().generation(), &active_generation);
        assert_eq!(
            inspection,
            CandidateResolutionInspection::Rejected {
                active_generation,
                reason: ResolvedHarnessError::ComponentGraph(
                    ComponentGraphError::UnknownOwningPlugin {
                        component,
                        plugin: owner,
                    },
                ),
            }
        );
    }

    #[test]
    fn valid_candidate_reports_the_same_diff_and_transition_plan_as_preview() {
        let active = ResolvedHarness::resolve([], [], [], &Authority::default()).unwrap();
        let reconciler = GraphReconciler::new(active);
        let candidate =
            GraphReconciler::resolve_candidate([], [], [], &Authority::default()).unwrap();
        let expected = reconciler.preview_candidate(&candidate);

        assert_eq!(
            reconciler.inspect_candidate_resolution(Ok(candidate)),
            CandidateResolutionInspection::Resolved(expected)
        );
    }
}
