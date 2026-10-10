//! RFC service-only authoring: one contract, one provider, no workflow or catalog.

use phenix_core::{HasPhenixSchema, ResolvedGenerationActivation, ServiceId};
use phenix_sdk::{InterfaceMarker, StaticPluginDefinition, StaticPluginGraph};

#[phenix_sdk::contract("fixture.service-only.echo@1")]
struct Echo;

#[derive(phenix_sdk::PhenixValue)]
struct Request {
    text: String,
}

#[derive(phenix_sdk::PhenixValue)]
struct Response {
    text: String,
}

#[phenix_sdk::plugin("fixture.service-only")]
mod plugin {
    use super::{Echo, Request, Response};

    #[phenix(provide(Echo), public)]
    fn echo(request: Request) -> Response {
        Response { text: request.text }
    }
}

// A service-only root declares its authorized import with typed metadata.
#[allow(dead_code)]
#[phenix_sdk::plugin(
    id = "fixture.service-only.host",
    execution = phenix_sdk::PluginExecution::ResourceOnly
)]
struct Consumer {
    #[phenix(dep)]
    provider: plugin::Plugin,
    #[phenix(import)]
    echo: phenix_sdk::Required<phenix_sdk::Call<Echo, Request, Response>>,
}

#[test]
fn typed_provide_activates_through_canonical_kernel_dispatch_without_a_plan() {
    let authority = phenix_sdk::Authority::default();
    let manifest = <plugin::Plugin as StaticPluginDefinition>::manifest();
    let components = <plugin::Plugin as StaticPluginDefinition>::component_manifests();
    assert!(manifest.dependencies.is_empty());
    assert_eq!(components.len(), 1);
    assert!(components[0].imports.is_empty());
    assert_eq!(components[0].exports.len(), 1);
    assert_eq!(components[0].exports[0].interface, Echo::interface_id());
    assert_eq!(
        components[0].exports[0].schema.request(),
        &Request::phenix_schema()
    );
    assert_eq!(
        components[0].exports[0].schema.response(),
        &Response::phenix_schema()
    );

    let graph = StaticPluginGraph::compose::<Consumer>().unwrap();
    assert!(graph.contributions().unwrap().is_empty());
    let consumer_components = Consumer::component_manifests();
    let resolved = phenix_core::ResolvedGeneration::resolve(
        [manifest.clone(), Consumer::manifest()],
        components
            .iter()
            .cloned()
            .chain(consumer_components.iter().cloned()),
        std::iter::empty(),
        &authority,
    )
    .unwrap();
    let mut kernel = phenix_core::Kernel::new(resolved.kernel_config().clone());
    graph.preload_embedded_factories(&mut kernel).unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    let input = serde_json::to_vec(&phenix_sdk::PhenixValue::from(&Request {
        text: "hello".into(),
    }))
    .unwrap();
    let output = kernel
        .invoke_component(
            &components[0].id,
            &ServiceId::parse("fixture.service-only.echo@1").unwrap(),
            &input,
            &authority,
            &manifest.id,
        )
        .unwrap();
    // Service-only root admission must work without workflow selection.
    // This exercises the same Core host dispatch used by future plan roots.
    let root = kernel.root_execution_handle(&authority);
    assert_eq!(root.generation(), Some(resolved.generation()));
    let selected = resolved
        .component_graph()
        .import_handle(&consumer_components[0].id, &Echo::interface_id())
        .unwrap()
        .unwrap();
    assert!(resolved.workflows().is_empty());
    let through_root = root.invoke_import(selected, &input).unwrap();
    assert_eq!(through_root, output);

    let value: phenix_sdk::PhenixValue = serde_json::from_slice(&output).unwrap();
    let reply = Response::try_from(phenix_sdk::Project(&value)).unwrap();
    assert_eq!(reply.text, "hello");
}
