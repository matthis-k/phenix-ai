#![forbid(unsafe_code)]

use phenix_core::{
    ComponentManifest, ModelInferenceRequest, ModelInferenceResponse, PluginInstance,
    PluginManifest,
};
use phenix_sdk::StaticPluginDefinition;
use std::collections::BTreeMap;

pub const BASIC_MODEL_PLUGIN: &str = "phenix.basic-model";
pub const BASIC_MODEL_COMPONENT: &str = "phenix.basic-model";

#[phenix_sdk::plugin("phenix.basic-model")]
mod plugin {
    use super::{BTreeMap, ModelInferenceRequest, ModelInferenceResponse, BASIC_MODEL_PLUGIN};

    #[phenix(export("phenix.models.inference@1"), terminal, priority = 10)]
    fn infer(request: ModelInferenceRequest) -> ModelInferenceResponse {
        ModelInferenceResponse {
            output: request.input,
            provider_metadata: BTreeMap::from([
                (
                    "provider".into(),
                    serde_json::json!(BASIC_MODEL_PLUGIN).into(),
                ),
                ("model".into(), serde_json::json!(request.model).into()),
                (
                    "implementation".into(),
                    serde_json::json!("deterministic-echo").into(),
                ),
            ]),
            usage: Default::default(),
            tool_calls: Vec::new(),
        }
    }
}

pub use plugin::Plugin;

#[must_use]
pub fn basic_model_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn basic_model_component_manifest() -> ComponentManifest {
    Plugin::component_manifests()
        .into_iter()
        .next()
        .expect("basic model has one generated component")
}

#[must_use]
pub fn basic_model_factory() -> Box<dyn PluginInstance> {
    let factory = Plugin::descriptor()
        .embedded_factory
        .expect("stateless basic model has a generated embedded factory");
    factory()
}
