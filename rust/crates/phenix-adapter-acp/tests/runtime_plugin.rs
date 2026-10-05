use phenix_adapter_acp::{adapter_acp_factory, adapter_acp_manifest, Plugin, ACP_ADAPTER_PLUGIN};
use phenix_core::PluginExecution;
use phenix_sdk::StaticPluginDefinition;

#[test]
fn generated_runtime_plugin_has_only_the_adapter_identity() {
    let manifest = adapter_acp_manifest();

    assert_eq!(manifest.id.as_str(), ACP_ADAPTER_PLUGIN);
    assert!(matches!(manifest.execution, PluginExecution::Embedded));
    assert!(manifest.dependencies.is_empty());
    assert!(manifest.services.is_empty());
    assert!(manifest.resource_namespaces.is_empty());
    assert!(manifest.maximum_authority.permissions().next().is_none());

    let components = Plugin::component_manifests();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), ACP_ADAPTER_PLUGIN);
    assert!(components[0].imports.is_empty());
    assert!(components[0].exports.is_empty());
    assert!(components[0].listeners.is_empty());

    let _instance = adapter_acp_factory();
}
