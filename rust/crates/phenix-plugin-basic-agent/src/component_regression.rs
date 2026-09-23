use crate::{
    basic_context_component_manifest, basic_context_manifest, basic_model_component_manifest,
    basic_model_factory, basic_model_manifest, basic_skills_component_manifest,
    basic_skills_manifest, basic_tools_component_manifest, basic_tools_manifest,
    BasicContextInterface, BasicSkillsInterface, BasicToolsInterface,
};
use phenix_core::{
    Authority, CapabilityId, ComponentExport, ComponentGraphError, ComponentId, ComponentImport,
    ComponentInterface, ComponentManifest, InterfaceId, Kernel, KernelConfig, ModelId, PhenixValue,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, Project,
    ResolvedComponentGraph, ResolvedHarness, ResolvedHarnessActivation, ResolvedHarnessError,
    ServiceContribution, ServiceId, ServiceRole,
};
use phenix_sdk::{
    model_inference_service, ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse,
};
use std::collections::BTreeMap;

const CONSUMER_SERVICE: &str = "fixture.basic-consumer@1";

struct Consumer;

impl PluginInstance for Consumer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let request: ModelInferenceRequest =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let response = host
            .invoke_import::<ModelInferenceInterface>(
                &component("fixture.consumer"),
                &PhenixValue::from(&request),
            )
            .map_err(|error| error.to_string())?;
        let response = ModelInferenceResponse::try_from(Project(&response))
            .map_err(|error| error.to_string())?;
        serde_json::to_vec(&response).map_err(|error| error.to_string())
    }
}

struct Replacement;

impl PluginInstance for Replacement {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&PhenixValue::from(&ModelInferenceResponse {
            output: b"replacement".to_vec().into(),
            provider_metadata: BTreeMap::from([(
                "provider".into(),
                serde_json::json!("fixture.replacement").into(),
            )]),
            usage: Default::default(),
            tool_calls: Vec::new(),
        }))
        .map_err(|error| error.to_string())
    }
}

fn component(value: &str) -> ComponentId {
    ComponentId::parse(value).unwrap()
}

fn service(value: &str) -> ServiceId {
    ServiceId::parse(value).unwrap()
}

fn consumer_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("fixture.consumer").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: service(CONSUMER_SERVICE),
            priority: 1,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn consumer_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: component("fixture.consumer"),
        owner: consumer_manifest().id,
        imports: vec![ComponentImport {
            interface: ModelInferenceInterface::interface_id(),
            schema: ModelInferenceInterface::schema(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn replacement_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("fixture.replacement").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: model_inference_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn replacement_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: component("fixture.replacement"),
        owner: replacement_manifest().id,
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: ModelInferenceInterface::interface_id(),
            schema: ModelInferenceInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn passive_manifest(id: &str, authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(id).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: Vec::new(),
        maximum_authority: authority,
    }
}

fn passive_component(
    id: &str,
    owner: &str,
    imports: Vec<ComponentImport>,
    exports: Vec<ComponentExport>,
    authority: Authority,
) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: component(id),
        owner: PluginId::parse(owner).unwrap(),
        imports,
        exports,
        maximum_authority: authority,
    }
}

fn assert_basic_interface_is_replaceable(
    basic_manifest: PluginManifest,
    basic_component: ComponentManifest,
    interface: InterfaceId,
    suffix: &str,
) {
    let extra = CapabilityId::parse(format!("fixture.{suffix}.extra")).unwrap();
    let consumer_authority = Authority::new([extra.clone()]);
    let consumer_plugin = format!("fixture.{suffix}-consumer");
    let consumer_component_id = format!("fixture.{suffix}-consumer");
    let replacement_plugin = format!("fixture.{suffix}-replacement");
    let replacement_component_id = format!("fixture.{suffix}-replacement");
    let consumer = passive_component(
        &consumer_component_id,
        &consumer_plugin,
        vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: consumer_authority.clone(),
        }],
        Vec::new(),
        consumer_authority.clone(),
    );

    let missing = ResolvedComponentGraph::compile(
        [passive_manifest(
            &consumer_plugin,
            consumer_authority.clone(),
        )],
        [consumer.clone()],
        &consumer_authority,
    )
    .unwrap_err();
    assert!(matches!(
        missing,
        ComponentGraphError::MissingRequiredImport {
            component: missing_component,
            interface: missing_interface,
        } if missing_component == component(&consumer_component_id)
            && missing_interface == interface
    ));

    let replacement = passive_component(
        &replacement_component_id,
        &replacement_plugin,
        Vec::new(),
        vec![ComponentExport {
            interface: interface.clone(),
            schema: Default::default(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        Authority::default(),
    );
    let graph = ResolvedComponentGraph::compile(
        [
            basic_manifest,
            passive_manifest(&consumer_plugin, consumer_authority.clone()),
            passive_manifest(&replacement_plugin, Authority::default()),
        ],
        [basic_component, consumer, replacement],
        &consumer_authority,
    )
    .unwrap();

    let handle = graph
        .import_handle(&component(&consumer_component_id), &interface)
        .unwrap()
        .unwrap();
    assert_eq!(handle.exporter(), &component(&replacement_component_id));
    assert_eq!(
        handle.owning_plugin(),
        &PluginId::parse(replacement_plugin).unwrap()
    );
    assert!(
        !handle.effective_authority().permits(&extra),
        "replacement binding must not inherit authority the provider does not own"
    );
}

#[test]
fn omitting_basic_and_replacement_model_leaves_required_import_unresolved() {
    let consumer = consumer_manifest();
    let error = ResolvedHarness::resolve(
        [consumer.clone()],
        [consumer_component()],
        [],
        &Authority::default(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ResolvedHarnessError::ComponentGraph(ComponentGraphError::MissingRequiredImport {
            component: missing_component,
            interface: missing_interface,
        }) if missing_component == component("fixture.consumer")
            && missing_interface == ModelInferenceInterface::interface_id()
    ));
}

#[test]
fn replacement_component_satisfies_the_same_basic_model_import_without_consumer_changes() {
    let basic = basic_model_manifest();
    let replacement = replacement_manifest();
    let consumer = consumer_manifest();
    let manifests = [basic.clone(), replacement.clone(), consumer.clone()];
    let resolved = ResolvedHarness::resolve(
        manifests.clone(),
        [
            basic_model_component_manifest(),
            replacement_component(),
            consumer_component(),
        ],
        [],
        &Authority::default(),
    )
    .unwrap();
    let binding = resolved
        .component_graph()
        .import_handle(
            &component("fixture.consumer"),
            &ModelInferenceInterface::interface_id(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(binding.exporter(), &component("fixture.replacement"));

    let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(basic.id, basic_model_factory)
        .unwrap();
    kernel
        .register_embedded_factory(replacement.id, || Box::new(Replacement))
        .unwrap();
    kernel
        .register_embedded_factory(consumer.id, || Box::new(Consumer))
        .unwrap();
    kernel.activate_all().unwrap();

    let request = ModelInferenceRequest {
        model: ModelId::parse("same-request").unwrap(),
        input: b"hello".to_vec().into(),
        options: BTreeMap::new(),
        tools: Vec::new(),
        continuation: Vec::new(),
    };
    let output = kernel
        .invoke(
            &service(CONSUMER_SERVICE),
            &serde_json::to_vec(&request).unwrap(),
            &Authority::default(),
            None,
        )
        .unwrap();
    let response: ModelInferenceResponse = serde_json::from_slice(&output).unwrap();
    assert_eq!(response.output.as_ref(), b"replacement");
}

#[test]
fn basic_tool_skill_and_context_defaults_are_replaceable_through_the_same_typed_contracts() {
    assert_basic_interface_is_replaceable(
        basic_tools_manifest(),
        basic_tools_component_manifest(),
        BasicToolsInterface::interface_id(),
        "tools",
    );
    assert_basic_interface_is_replaceable(
        basic_skills_manifest(),
        basic_skills_component_manifest(),
        BasicSkillsInterface::interface_id(),
        "skills",
    );
    assert_basic_interface_is_replaceable(
        basic_context_manifest(),
        basic_context_component_manifest(),
        BasicContextInterface::interface_id(),
        "context",
    );
}
