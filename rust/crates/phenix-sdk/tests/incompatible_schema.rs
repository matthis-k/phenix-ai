use phenix_core::{Authority, PluginId, ResolvedGeneration};
use phenix_sdk::{Call, Required, StaticPluginDefinition};

#[phenix_sdk::interface("fixture.incompatible.models@1")]
struct IncompatibleModels;

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct IncompatibleConsumerRequest {
    prompt: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct IncompatibleConsumerResponse {
    value: String,
    required_extra: u64,
}

#[phenix_sdk::plugin("fixture.incompatible-provider")]
mod incompatible_provider {
    use super::IncompatibleModels;

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct Request {
        prompt: String,
    }

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct Response {
        value: String,
    }

    #[phenix(export(IncompatibleModels), terminal)]
    fn models(request: Request) -> Response {
        Response {
            value: request.prompt,
        }
    }
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct IncompatibleConsumerApi {
    #[phenix(import)]
    models: Required<
        Call<IncompatibleModels, IncompatibleConsumerRequest, IncompatibleConsumerResponse>,
    >,
}

#[phenix_sdk::component]
impl IncompatibleConsumerApi {}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.incompatible-consumer")]
struct IncompatibleConsumerPlugin {
    #[phenix(component)]
    api: IncompatibleConsumerApi,
}

#[test]
fn incompatible_component_schemas_fail_before_activation() {
    let authority = Authority::default();
    let error = ResolvedGeneration::resolve(
        [
            <incompatible_provider::Plugin as StaticPluginDefinition>::manifest(),
            <IncompatibleConsumerPlugin as StaticPluginDefinition>::manifest(),
        ],
        [
            <incompatible_provider::Plugin as StaticPluginDefinition>::component_manifests()
                .remove(0),
            <IncompatibleConsumerPlugin as StaticPluginDefinition>::component_manifests().remove(0),
        ],
        std::iter::empty(),
        &authority,
    )
    .unwrap_err();

    assert!(error.to_string().contains("incompatible"));
}

#[phenix_sdk::interface("fixture.replacement.models@1")]
struct ReplacementModels;

#[phenix_sdk::plugin("fixture.replacement-provider-a")]
mod replacement_provider_a {
    use super::ReplacementModels;

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct Request {
        prompt: String,
    }

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct Response {
        value: String,
    }

    #[phenix(export(ReplacementModels), terminal)]
    fn models(_request: Request) -> Response {
        Response {
            value: "provider.a".into(),
        }
    }
}

#[phenix_sdk::plugin("fixture.replacement-provider-b")]
mod replacement_provider_b {
    use super::ReplacementModels;

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct Request {
        prompt: String,
    }

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct Response {
        value: String,
        implementation: String,
    }

    #[phenix(export(ReplacementModels), terminal)]
    fn models(_request: Request) -> Response {
        Response {
            value: "provider.b".into(),
            implementation: "replacement".into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ReplacementConsumerRequest {
    prompt: String,
    trace_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ReplacementConsumerResponse {
    value: PluginId,
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct ReplacementConsumerApi {
    #[phenix(import)]
    models:
        Required<Call<ReplacementModels, ReplacementConsumerRequest, ReplacementConsumerResponse>>,
}

#[phenix_sdk::component]
impl ReplacementConsumerApi {}

#[allow(dead_code)]
#[phenix_sdk::plugin("fixture.replacement-consumer")]
struct ReplacementConsumerPlugin {
    #[phenix(component)]
    api: ReplacementConsumerApi,
}

fn replacement_resolves<P>()
where
    P: StaticPluginDefinition
        + phenix_sdk::StaticPluginComponents
        + phenix_sdk::StaticPluginResources,
{
    let authority = Authority::default();
    ResolvedGeneration::resolve(
        [
            P::manifest(),
            <ReplacementConsumerPlugin as StaticPluginDefinition>::manifest(),
        ],
        [
            P::component_manifests().remove(0),
            <ReplacementConsumerPlugin as StaticPluginDefinition>::component_manifests().remove(0),
        ],
        std::iter::empty(),
        &authority,
    )
    .unwrap();
}

#[test]
fn consumer_contract_accepts_compatible_replacement_providers() {
    replacement_resolves::<replacement_provider_a::Plugin>();
    replacement_resolves::<replacement_provider_b::Plugin>();
}
