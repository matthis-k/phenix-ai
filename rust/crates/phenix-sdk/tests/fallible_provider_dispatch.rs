use phenix_core::{
    Authority, InvocationOutcome, Kernel, LayerPolicy, LayerResult, PhenixValue, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, Project, ResolvedGeneration,
    ResolvedGenerationActivation, ServiceContribution, ServiceId, ServiceRole,
};
use phenix_sdk::{
    HasPhenixSchema, StaticComponentBehavior, StaticPluginDefinition, StaticPluginGraph,
};
use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

const SERVICE: &str = "fixture.fallible-provider.run@1";
const LAYER_PLUGIN: &str = "fixture.fallible-provider.layer";

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Request {
    outcome: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Response {
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
enum DomainFailure {
    Conflict { resource: String },
    Disconnected { provider: String },
}

impl Display for DomainFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider rejected request")
    }
}

#[phenix_sdk::plugin("fixture.fallible-provider")]
mod provider {
    use super::*;

    #[phenix(export("fixture.fallible-provider.run@1"), public, terminal)]
    pub fn run(request: Request) -> Result<Response, DomainFailure> {
        match request.outcome.as_str() {
            "conflict" => Err(DomainFailure::Conflict {
                resource: "workspace".into(),
            }),
            "disconnected" => Err(DomainFailure::Disconnected {
                provider: "remote".into(),
            }),
            value => Ok(Response {
                value: value.to_owned(),
            }),
        }
    }
}

fn service() -> ServiceId {
    ServiceId::parse(SERVICE).unwrap()
}

fn layer_id() -> PluginId {
    PluginId::parse(LAYER_PLUGIN).unwrap()
}

fn layer_manifest() -> PluginManifest {
    PluginManifest {
        id: layer_id(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Layer,
            service: service(),
            priority: 10,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

struct PassthroughLayer;

impl PluginInstance for PassthroughLayer {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_layer(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        host.continue_service(input, host.authority())
            .map(LayerResult::Handled)
            .map_err(|error| error.to_string())
    }
}

fn configured_kernel(layered: bool) -> (Kernel, Authority) {
    let authority = Authority::default();
    let provider_manifest = <provider::Plugin as StaticPluginDefinition>::manifest();
    let components = <provider::Plugin as StaticPluginDefinition>::component_manifests();
    let mut manifests = vec![provider_manifest.clone()];
    let mut layer_policies = BTreeMap::new();

    if layered {
        let layer = layer_manifest();
        layer_policies.insert(
            service(),
            vec![LayerPolicy {
                plugin: layer.id.clone(),
                priority: 10,
                required: true,
                enabled: true,
            }],
        );
        manifests.push(layer);
    }

    let resolved = ResolvedGeneration::resolve_with_layer_policies(
        manifests,
        components,
        std::iter::empty(),
        layer_policies,
        &authority,
    )
    .unwrap();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    StaticPluginGraph::compose::<provider::Plugin>()
        .unwrap()
        .preload_embedded_factories(&mut kernel)
        .unwrap();
    if layered {
        kernel
            .register_embedded_factory(layer_id(), || Box::new(PassthroughLayer))
            .unwrap();
    }
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();
    (kernel, authority)
}

fn active_kernel() -> (Kernel, Authority) {
    configured_kernel(false)
}

fn invoke(kernel: &mut Kernel, authority: &Authority, outcome: &str) -> InvocationOutcome {
    let input = serde_json::to_vec(&PhenixValue::from(&Request {
        outcome: outcome.into(),
    }))
    .unwrap();
    let output = kernel
        .invoke_component(
            &provider::Plugin::component_id(),
            &service(),
            &input,
            authority,
            &provider::Plugin::plugin_id(),
        )
        .unwrap();
    let value: PhenixValue = serde_json::from_slice(&output).unwrap();
    InvocationOutcome::from_transport_value(value)
}

#[test]
fn generated_fallible_export_declares_domain_error_schema() {
    let exports = <provider::Component as StaticComponentBehavior>::exports();

    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].schema.error(), &DomainFailure::phenix_schema());
}

#[test]
fn generated_provider_dispatch_preserves_structural_domain_errors() {
    let (mut kernel, authority) = active_kernel();

    let conflict = invoke(&mut kernel, &authority, "conflict");
    let disconnected = invoke(&mut kernel, &authority, "disconnected");

    let InvocationOutcome::DomainError(conflict) = conflict else {
        panic!("conflict was not lowered as a domain error");
    };
    let InvocationOutcome::DomainError(disconnected) = disconnected else {
        panic!("disconnect was not lowered as a domain error");
    };

    let conflict = DomainFailure::try_from(Project(&conflict)).unwrap();
    let disconnected = DomainFailure::try_from(Project(&disconnected)).unwrap();

    assert_eq!(
        conflict,
        DomainFailure::Conflict {
            resource: "workspace".into(),
        }
    );
    assert_eq!(
        disconnected,
        DomainFailure::Disconnected {
            provider: "remote".into(),
        }
    );
    assert_eq!(conflict.to_string(), disconnected.to_string());
    assert_ne!(conflict, disconnected);
}

#[test]
fn structural_domain_error_survives_layer_dispatch() {
    let (mut kernel, authority) = configured_kernel(true);

    let outcome = invoke(&mut kernel, &authority, "conflict");
    let InvocationOutcome::DomainError(value) = outcome else {
        panic!("layer collapsed the domain error into a runtime failure");
    };
    assert_eq!(
        DomainFailure::try_from(Project(&value)).unwrap(),
        DomainFailure::Conflict {
            resource: "workspace".into(),
        }
    );

    let provenance = kernel.service_invocation_provenance();
    let invocation = provenance.last().expect("layered invocation recorded");
    assert_eq!(invocation.participants.len(), 2);
    assert_eq!(invocation.participants[0].plugin, layer_id());
    assert_eq!(invocation.participants[0].role, ServiceRole::Layer);
    assert_eq!(
        invocation.participants[1].plugin,
        <provider::Plugin as StaticPluginDefinition>::manifest().id
    );
    assert_eq!(invocation.participants[1].role, ServiceRole::Terminal);
}

#[test]
fn generated_provider_dispatch_keeps_success_as_a_bare_value() {
    let (mut kernel, authority) = active_kernel();

    let outcome = invoke(&mut kernel, &authority, "ok");

    let InvocationOutcome::Success(value) = outcome else {
        panic!("success was not lowered as a success outcome");
    };
    assert_eq!(
        Response::try_from(Project(&value)).unwrap(),
        Response { value: "ok".into() }
    );
}
