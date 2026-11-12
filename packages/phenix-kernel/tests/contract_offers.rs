mod support;
use phenix_graph::{
    compose::{Edit, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{ContractDefinition, ContractRequirement, Kernel, Key, NetworkOffer};
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
        into.add(NetworkOffer {
            id: "offer".into(),
            producer: Key::new("provider", "main"),
            unit: if self.missing_path {
                "absent"
            } else {
                "service"
            }
            .into(),
            contract: Key::new("provider", "definition"),
        })?;
        into.add(ContractRequirement {
            id: "request".into(),
            consumer: Key::new("provider", "consumer"),
            contract: Key::new("provider", "definition"),
        })?;
        into.add(support::contribution("consumer", NetworkPatch::new([])))?;
        into.add(support::contribution(
            "main",
            support::patch("service", self.module, self.claim, 1, "example/service@1"),
        ))?;
        if self.late_edit {
            let mut late = support::contribution(
                "late",
                NetworkPatch::new([Edit::Replace {
                    path: "service".into(),
                    definition: support::definition(self.module, false, 2, "example/service@1"),
                }]),
            );
            late.requires.push(Key::new("provider", "main"));
            into.add(late)?;
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
            let snapshot = support::snapshot(
                "provider",
                Plugin {
                    module,
                    claim: failure != Some("claim"),
                    missing_path: failure == Some("path"),
                    late_edit: failure == Some("late"),
                },
            );
            let mut options = support::options(std::slice::from_ref(&snapshot));
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
            return into.add(NetworkOffer {
                id: "stolen".into(),
                producer: Key::new("provider", "main"),
                unit: "service".into(),
                contract: Key::new("provider", "definition"),
            });
        }
        let mut patch = support::contribution(
            "patch",
            NetworkPatch::new([Edit::Replace {
                path: "service".into(),
                definition: support::definition(self.module, true, 10, "example/service@1"),
            }]),
        );
        patch.requires.push(Key::new("provider", "main"));
        into.add(patch)
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
            let options = support::options(&snapshots);
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
