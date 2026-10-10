use phenix_sdk::{
    Authority, Call, Emit, HasPhenixSchema, Host, InterfaceMarker, Optional, PermissionId,
    Required, StaticComponentBehavior, StaticComponentImports, StaticPluginComponents,
};

#[phenix_sdk::interface("fixture.models.inference@1")]
struct ModelsInference;

#[derive(phenix_sdk::PhenixValue)]
struct ConsumerRequest;
#[derive(phenix_sdk::PhenixValue)]
struct ConsumerResponse;

const MODEL_POLICY_PRIORITY: i32 = 23;

// These declarations are consumed as macro metadata rather than called as runtime fixtures.
#[allow(dead_code)]
#[phenix_sdk::component]
struct Api {
    #[phenix(
        import,
        authority = Authority::new([PermissionId::parse("models.invoke").unwrap()])
    )]
    models: Required<Call<ModelsInference, ConsumerRequest, ConsumerResponse>>,

    #[phenix(import)]
    fallback: Optional<Call<ModelsInference, ConsumerRequest, ConsumerResponse>>,

    #[phenix(
        host,
        authority = Authority::new([PermissionId::parse("models.host").unwrap()])
    )]
    model_host: Host<ModelsInference>,

    #[phenix(event("fixture.models.emitted"))]
    emitted: Emit<ConsumerResponse>,
}

#[allow(dead_code)]
#[phenix_sdk::component]
impl Api {
    #[phenix(
        export(ModelsInference),
        public,
        terminal,
        authority = Authority::new([PermissionId::parse("models.serve").unwrap()])
    )]
    fn run(
        &self,
        _ctx: &phenix_sdk::CallContext<'_>,
        _request: ConsumerRequest,
    ) -> ConsumerResponse {
        ConsumerResponse
    }

    #[phenix(layer(ModelsInference, priority = MODEL_POLICY_PRIORITY))]
    fn model_policy(&self) {}

    #[phenix(listen("fixture.models.completed"))]
    async fn model_completed(
        &self,
        _context: &phenix_sdk::EventContext<'_, '_>,
        _event: ConsumerResponse,
    ) {
    }

    #[phenix(value("fixture.component.status@1"), public)]
    fn status(&self) -> u64 {
        1
    }
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.component-owner")]
struct Plugin {
    #[phenix(component)]
    api: Api,
}

#[test]
fn component_fields_lower_to_typed_import_export_and_plugin_metadata() {
    fn assert_generated_dispatch<T: phenix_sdk::StaticComponentDispatch>() {}
    assert_generated_dispatch::<Api>();

    let import_authority = Authority::new([PermissionId::parse("models.invoke").unwrap()]);
    let host_authority = Authority::new([PermissionId::parse("models.host").unwrap()]);
    let export_authority = Authority::new([PermissionId::parse("models.serve").unwrap()]);

    let imports = <Api as StaticComponentImports>::imports();
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].interface.as_str(), "fixture.models.inference@1");
    assert_eq!(imports[0].field, "models");
    assert!(imports[0].required);
    assert_eq!(imports[0].authority, import_authority);
    assert_eq!(imports[1].interface.as_str(), "fixture.models.inference@1");
    assert_eq!(imports[1].field, "fallback");
    assert!(!imports[1].required);
    assert_eq!(imports[1].authority, Authority::default());

    let hosts = <Api as StaticComponentImports>::hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].interface.as_str(), "fixture.models.inference@1");
    assert_eq!(hosts[0].field, "model_host");
    assert_eq!(hosts[0].authority, host_authority);

    let events = <Api as StaticComponentImports>::events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.as_str(), "fixture.models.emitted");
    assert_eq!(events[0].field, "emitted");
    assert!(events[0].payload_type.ends_with("::ConsumerResponse"));
    assert_eq!(events[0].payload_schema, ConsumerResponse::phenix_schema());

    let exports = <Api as StaticComponentBehavior>::exports();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].interface.as_str(), "fixture.models.inference@1");
    assert_eq!(
        exports[0].schema.request(),
        &ConsumerRequest::phenix_schema()
    );
    assert_eq!(
        exports[0].schema.response(),
        &ConsumerResponse::phenix_schema()
    );
    assert_eq!(exports[0].method, "run");
    assert!(exports[0].public);
    assert!(exports[0].terminal);
    assert_eq!(exports[0].required_authority, export_authority);

    let layers = <Api as StaticComponentBehavior>::layers();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].interface.as_str(), "fixture.models.inference@1");
    assert_eq!(layers[0].method, "model_policy");
    assert_eq!(layers[0].priority, MODEL_POLICY_PRIORITY);

    let listeners = <Api as StaticComponentBehavior>::listeners();
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].event.as_str(), "fixture.models.completed");
    assert_eq!(listeners[0].method, "model_completed");

    let values = <Api as StaticComponentBehavior>::values();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].id.as_str(), "fixture.component.status@1");
    assert_eq!(values[0].method, "status");
    assert!(values[0].public);
    assert_eq!(values[0].value_type, std::any::type_name::<u64>());
    assert_eq!(values[0].schema, u64::phenix_schema());

    let components = <Plugin as StaticPluginComponents>::components();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), "fixture.component-owner.api");
    assert_eq!(components[0].field, "api");

    let owner = Plugin::plugin_id();
    let manifest = components[0].manifest(&owner);
    assert_eq!(manifest.owner, owner);
    assert_eq!(manifest.imports.len(), 2);
    assert_eq!(manifest.imports[0].authority, import_authority);
    assert_eq!(manifest.exports.len(), 1);
    assert_eq!(
        manifest.exports[0].interface,
        ModelsInference::interface_id()
    );
    assert_eq!(manifest.exports[0].required_authority, export_authority);

    let services = components[0].services();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].role, phenix_core::ServiceRole::Layer);
    assert_eq!(services[0].priority, MODEL_POLICY_PRIORITY);

    let plugin_manifest = <Plugin as phenix_sdk::StaticPluginDefinition>::manifest();
    assert_eq!(plugin_manifest.services.len(), 1);
    assert_eq!(
        plugin_manifest.services[0].role,
        phenix_core::ServiceRole::Layer
    );
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.legacy-component-owner")]
struct LegacyPlugin {
    #[phenix(component, id = "legacy.component.api")]
    api: Api,
}

#[test]
fn explicit_component_identity_preserves_stable_runtime_id() {
    let components = <LegacyPlugin as StaticPluginComponents>::components();

    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), "legacy.component.api");
}

#[phenix_sdk::interface("fixture.root.models@1")]
struct RootModels;

#[derive(phenix_sdk::PhenixValue)]
struct RootRequest;

#[derive(phenix_sdk::PhenixValue)]
struct RootResponse;

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.root-fields")]
struct RootPlugin {
    #[phenix(
        import,
        authority = Authority::new([PermissionId::parse("models.invoke").unwrap()])
    )]
    models: Required<Call<RootModels, RootRequest, RootResponse>>,

    #[phenix(import)]
    optional_models: Optional<Call<RootModels, RootRequest, RootResponse>>,

    #[phenix(
        host,
        authority = Authority::new([PermissionId::parse("models.host").unwrap()])
    )]
    model_host: Host<RootModels>,

    #[phenix(event("fixture.root.emitted"))]
    emitted: Emit<RootResponse>,
}

#[test]
fn plugin_root_fields_lower_to_a_derived_root_component() {
    let import_authority = Authority::new([PermissionId::parse("models.invoke").unwrap()]);
    let host_authority = Authority::new([PermissionId::parse("models.host").unwrap()]);

    let imports = <RootPlugin as StaticComponentImports>::imports();
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].field, "models");
    assert_eq!(imports[0].interface.as_str(), "fixture.root.models@1");
    assert!(imports[0].required);
    assert_eq!(imports[0].authority, import_authority);
    assert_eq!(imports[1].field, "optional_models");
    assert_eq!(imports[1].interface.as_str(), "fixture.root.models@1");
    assert!(!imports[1].required);
    assert_eq!(imports[1].authority, Authority::default());

    let hosts = <RootPlugin as StaticComponentImports>::hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].field, "model_host");
    assert_eq!(hosts[0].interface.as_str(), "fixture.root.models@1");
    assert_eq!(hosts[0].authority, host_authority);

    let events = <RootPlugin as StaticComponentImports>::events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].field, "emitted");
    assert_eq!(events[0].event.as_str(), "fixture.root.emitted");

    let components = <RootPlugin as StaticPluginComponents>::components();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), "fixture.root-fields.root");
    assert_eq!(components[0].field, "root");

    let manifests = <RootPlugin as phenix_sdk::StaticPluginDefinition>::component_manifests();
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].imports.len(), 2);
    assert_eq!(manifests[0].imports[0].authority, import_authority);
    assert!(manifests[0].imports[0].required);
    assert!(!manifests[0].imports[1].required);
}

#[phenix_sdk::interface("fixture.attribute-incompatible.models@1")]
struct AttributeIncompatibleModels;

#[derive(phenix_sdk::PhenixValue)]
struct ProviderRequest {
    prompt: String,
}

#[derive(phenix_sdk::PhenixValue)]
struct ProviderResponse {
    value: String,
}

#[derive(phenix_sdk::PhenixValue)]
struct IncompatibleConsumerRequest {
    prompt: String,
}

#[derive(phenix_sdk::PhenixValue)]
struct IncompatibleConsumerResponse {
    value: String,
    required_extra: u64,
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct ProviderApi;

#[allow(dead_code)]
#[phenix_sdk::component]
impl ProviderApi {
    #[phenix(export(AttributeIncompatibleModels), terminal)]
    fn models(&self, request: ProviderRequest) -> ProviderResponse {
        ProviderResponse {
            value: request.prompt,
        }
    }
}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.attribute-incompatible-provider")]
struct AttributeProviderPlugin {
    #[phenix(component)]
    api: ProviderApi,
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct IncompatibleConsumerApi {
    #[phenix(import)]
    models: Required<
        Call<
            AttributeIncompatibleModels,
            IncompatibleConsumerRequest,
            IncompatibleConsumerResponse,
        >,
    >,
}

#[phenix_sdk::component]
impl IncompatibleConsumerApi {}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.attribute-incompatible-consumer")]
struct AttributeConsumerPlugin {
    #[phenix(component)]
    api: IncompatibleConsumerApi,
}

#[test]
fn attribute_only_components_reject_structurally_incompatible_provider_and_consumer() {
    let authority = Authority::default();
    let manifests = [
        <AttributeProviderPlugin as phenix_sdk::StaticPluginDefinition>::manifest(),
        <AttributeConsumerPlugin as phenix_sdk::StaticPluginDefinition>::manifest(),
    ];
    let components = [
        <AttributeProviderPlugin as phenix_sdk::StaticPluginDefinition>::component_manifests()
            .remove(0),
        <AttributeConsumerPlugin as phenix_sdk::StaticPluginDefinition>::component_manifests()
            .remove(0),
    ];

    let error = phenix_core::ResolvedGeneration::resolve(
        manifests,
        components,
        std::iter::empty(),
        &authority,
    )
    .unwrap_err();

    assert!(error.to_string().contains("incompatible"));
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct ProvidedApi {}

#[phenix_sdk::component]
impl ProvidedApi {
    #[phenix(provide(ModelsInference), public)]
    fn serve(&self, _request: ConsumerRequest) -> ConsumerResponse {
        ConsumerResponse
    }
}

#[test]
fn typed_provide_on_a_component_implies_terminal_dispatch() {
    let exports = <ProvidedApi as StaticComponentBehavior>::exports();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].interface.as_str(), "fixture.models.inference@1");
    assert!(exports[0].public);
    assert!(exports[0].terminal);
    assert_eq!(
        exports[0].schema.request(),
        &ConsumerRequest::phenix_schema()
    );
    assert_eq!(
        exports[0].schema.response(),
        &ConsumerResponse::phenix_schema()
    );
}
