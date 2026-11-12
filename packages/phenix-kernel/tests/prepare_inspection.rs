mod support;
use phenix_graph::{compose::NetworkPatch, process::LocalProcessDispatcher};
use phenix_kernel::{BuildOptions, CandidateArtifacts, Contribution, Kernel, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::Arc;

struct InspectionProbe;
impl PluginInstance for InspectionProbe {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        for id in ["a", "b"] {
            into.add(Contribution {
                id: id.into(),
                requires: vec![],
                routes: vec![],
                patch: NetworkPatch::new([]),
                prepare: Some(Box::new(|context| {
                    assert!(context.bind_import("undeclared").is_err());
                    let visible = context
                        .inspect_network()
                        .ok()
                        .map(|compiled| compiled.processes().len());
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.export(visible)?;
                    Ok(artifacts)
                })),
            })?;
        }
        Ok(())
    }
}

#[test]
fn full_preparation_inspection_is_default_denied_and_granted_per_exact_contribution() {
    let snapshot = support::snapshot("plugin", InspectionProbe);
    for (grants, expected) in [
        (vec![], [None, None]),
        (vec![Key::new("other", "a")], [None, None]),
        (vec![Key::new("plugin", "missing")], [None, None]),
        (vec![Key::new("plugin", "a")], [Some(0), None]),
        (vec![Key::new("plugin", "b")], [None, Some(0)]),
        (
            vec![Key::new("plugin", "a"), Key::new("plugin", "b")],
            [Some(0), Some(0)],
        ),
    ] {
        let mut options = BuildOptions::default();
        options.inspect_network.extend(grants);
        let kernel = Kernel::build_with_options(
            "inspection",
            [snapshot.clone()],
            Arc::new(LocalProcessDispatcher),
            options,
        )
        .unwrap();
        for (id, expected_value) in ["a", "b"].into_iter().zip(expected) {
            assert_eq!(
                kernel.export::<Option<usize>>(&Key::new("plugin", id)),
                Some(&expected_value),
            );
        }
    }
}
