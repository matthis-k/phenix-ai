use phenix_core::{HasPhenixSchema, ResolvedGenerationActivation};

#[derive(phenix_sdk::PhenixValue)]
struct Request {
    value: String,
}

#[derive(phenix_sdk::PhenixValue)]
struct Response {
    value: String,
}

#[phenix_sdk::plugin("fixture.manifest.stateless")]
mod plugin {
    use super::{Request, Response};

    #[phenix(export("fixture.manifest.stateless.run@1"), terminal)]
    fn run(request: Request) -> Response {
        Response {
            value: request.value,
        }
    }

    #[phenix(export("fixture.manifest.stateless.projected@1"), terminal)]
    fn projected(
        _context: &phenix_sdk::CallContext<'_>,
        request: phenix_sdk::Project<Request>,
    ) -> Response {
        Response {
            value: request.0.value,
        }
    }

    #[phenix(export("fixture.manifest.stateless.exact@1"), terminal)]
    fn exact(request: phenix_sdk::Exact<Request>) -> Result<Response, String> {
        Ok(Response {
            value: request.0.value,
        })
    }

    #[phenix(export("fixture.manifest.stateless.ping@1"), terminal)]
    fn ping() -> Response {
        Response {
            value: "pong".into(),
        }
    }

    #[allow(dead_code)]
    #[phenix(value("fixture.manifest.stateless.status@1"), public)]
    fn status() -> u64 {
        1
    }
}

#[test]
fn stateless_plugin_uses_generic_manifest_lowering() {
    let manifest = <plugin::Plugin as phenix_sdk::StaticPluginDefinition>::manifest();

    assert_eq!(manifest.id.as_str(), "fixture.manifest.stateless");
    assert!(manifest.dependencies.is_empty());
    assert!(manifest.resource_namespaces.is_empty());
    let descriptor = <plugin::Plugin as phenix_sdk::StaticPluginDefinition>::descriptor();
    assert!(descriptor.embedded_factory.is_some());

    let components = <plugin::Plugin as phenix_sdk::StaticPluginComponents>::components();
    let values = components[0].values();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].id.as_str(), "fixture.manifest.stateless.status@1");
    assert_eq!(values[0].value_type, std::any::type_name::<u64>());
    assert_eq!(values[0].schema, u64::phenix_schema());
}

#[test]
fn stateless_plugin_runs_through_generated_factory_and_dispatch() {
    let authority = phenix_sdk::Authority::default();
    let manifest = <plugin::Plugin as phenix_sdk::StaticPluginDefinition>::manifest();
    let components = <plugin::Plugin as phenix_sdk::StaticPluginDefinition>::component_manifests();
    let component = components[0].id.clone();
    let resolved =
        phenix_core::ResolvedGeneration::resolve([manifest.clone()], components, [], &authority)
            .unwrap();

    let mut kernel =
        phenix_core::Kernel::new(phenix_core::KernelConfig::new([manifest.clone()]).unwrap());
    let graph = phenix_sdk::StaticPluginGraph::compose::<plugin::Plugin>().unwrap();
    graph.preload_embedded_factories(&mut kernel).unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    for method in ["run", "projected", "exact"] {
        let input = serde_json::to_vec(&phenix_sdk::PhenixValue::from(&Request {
            value: method.into(),
        }))
        .unwrap();
        let service =
            phenix_core::ServiceId::parse(format!("fixture.manifest.stateless.{method}@1"))
                .unwrap();
        let output = kernel
            .invoke_component(&component, &service, &input, &authority, &manifest.id)
            .unwrap();
        let value: phenix_sdk::PhenixValue = serde_json::from_slice(&output).unwrap();
        let response = Response::try_from(phenix_sdk::Project(&value)).unwrap();
        assert_eq!(response.value, method);
    }
}
