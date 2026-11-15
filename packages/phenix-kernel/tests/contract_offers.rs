mod support;
use phenix_graph::{
    compose::{Edit, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{BuildOptions, ContractDefinition, Kernel, Key};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct Plugin {
    module: bool,
    claim: bool,
    missing_path: bool,
    late_edit: bool,
}
impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        into.add(ContractDefinition {
            id: "definition".into(),
            contract: support::contract("example/service@1"),
        })?;
        support::contribution("consumer", NetworkPatch::new([]))
            .import("request", Key::new("provider", "definition"))
            .register(into)?;
        support::contribution(
            "main",
            support::patch("service", self.module, self.claim, 1, "example/service@1"),
        )
        .offer(
            "offer",
            if self.missing_path {
                "absent"
            } else {
                "service"
            },
            Key::new("provider", "definition"),
        )
        .register(into)?;
        if self.late_edit {
            support::contribution(
                "late",
                NetworkPatch::new([Edit::Replace {
                    path: "service".into(),
                    definition: support::definition(self.module, false, 2, "example/service@1"),
                }]),
            )
            .after([Key::new("provider", "main")])
            .register(into)?;
        }
        Ok(())
    }
}

#[test]
fn node_and_module_offers_require_final_claimed_units_and_valid_host_selection() {
    for module in [false, true] {
        for failure in [
            None,
            Some("claim"),
            Some("path"),
            Some("selection"),
            Some("late"),
        ] {
            let mut options = BuildOptions::default();
            support::grant(
                &mut options,
                Key::new("provider", "main"),
                [phenix_graph::compose::Footprint::Component(
                    "service".into(),
                )],
            );
            if failure == Some("late") {
                support::grant(
                    &mut options,
                    Key::new("provider", "late"),
                    [phenix_graph::compose::Footprint::Component(
                        "service".into(),
                    )],
                );
            }
            let snapshot = support::snapshot(
                "provider",
                Plugin {
                    module,
                    claim: failure != Some("claim"),
                    missing_path: failure == Some("path"),
                    late_edit: failure == Some("late"),
                },
            );
            if failure == Some("selection") {
                options.imports.selections.insert(
                    Key::new("provider", "request"),
                    Key::new("provider", "absent"),
                );
            }
            let result = Kernel::build_with_options(
                "host",
                [snapshot],
                Arc::new(LocalProcessDispatcher),
                options,
            );
            if let Some(failure) = failure {
                let error = result.err().unwrap().to_string();
                let expected = match failure {
                    "path" | "selection" => "absent",
                    _ => "claim",
                };
                assert!(
                    error.contains(expected),
                    "module={module}, case={failure}: {error}"
                );
            } else {
                result.unwrap();
            }
        }
    }
}

struct ForeignPlugin {
    module: bool,
    replace: bool,
}
impl PluginInstance for ForeignPlugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        if !self.replace {
            return support::contribution("thief", NetworkPatch::new([]))
                .after([Key::new("provider", "main")])
                .offer("stolen", "service", Key::new("provider", "definition"))
                .register(into);
        }
        support::contribution(
            "patch",
            NetworkPatch::new([Edit::Replace {
                path: "service".into(),
                definition: support::definition(self.module, true, 10, "example/service@1"),
            }]),
        )
        .after([Key::new("provider", "main")])
        .register(into)
    }
}
#[test]
fn offers_cannot_launder_foreign_nodes_modules_or_replacement_generations() {
    let mut catalog = PluginCatalog::new();
    let provider = catalog
        .register("provider", |config: Plugin| Ok(config))
        .unwrap();
    let foreign = catalog
        .register("foreign", |config: ForeignPlugin| Ok(config))
        .unwrap();
    for module in [false, true] {
        for replace in [false, true] {
            let mut plugins = catalog.instance();
            let mut options = BuildOptions::default();
            options.imports.selections.insert(
                Key::new("provider", "request"),
                Key::new("provider", "offer"),
            );
            support::grant(
                &mut options,
                Key::new("provider", "main"),
                [phenix_graph::compose::Footprint::Component(
                    "service".into(),
                )],
            );
            if replace {
                support::grant(
                    &mut options,
                    Key::new("foreign", "patch"),
                    [phenix_graph::compose::Footprint::Component(
                        "service".into(),
                    )],
                );
            }
            plugins
                .load(
                    &provider,
                    Plugin {
                        module,
                        claim: true,
                        missing_path: false,
                        late_edit: false,
                    },
                )
                .unwrap();
            plugins
                .load(&foreign, ForeignPlugin { module, replace })
                .unwrap();
            let snapshots: Vec<_> = plugins.snapshots().collect();
            let error = Kernel::build_with_options(
                "host",
                snapshots,
                Arc::new(LocalProcessDispatcher),
                options,
            )
            .err()
            .unwrap();
            assert!(error.to_string().contains("origin mismatch"), "{error}");
        }
    }
}
