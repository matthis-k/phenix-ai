use phenix_core::{
    ComponentInterface, DurableSchema, EventEnvelope, EventFailurePolicy, EventSubscription,
    EventTypeId, GraphReconciler, InterfaceId, InterfaceSchema, Kernel, PluginId, ResolvedHarness,
    ResolvedHarnessActivation, ResourceNamespace, ServiceId, SubscriptionId, SubscriptionSpec,
    TransactionOp,
};
use phenix_sdk::{
    Authority, Call, CapabilityId, Emit, Required, StaticComponentBehavior,
    StaticComponentRuntimeDispatch, StaticPluginComponentDispatch, StaticPluginComponents,
    StaticPluginConfiguration, StaticPluginDefinition, StaticPluginLifecycle,
    StaticPluginResources,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Settings {
    retries: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Request {
    prompt: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Response {
    value: String,
}

#[phenix_sdk::interface("fixture.attribute-gate.models@1")]
struct Models;

#[phenix_sdk::interface("fixture.attribute-gate.internal@1")]
struct Internal;

#[phenix_sdk::interface("fixture.attribute-gate.dependency@1")]
struct Dependency;

struct DependencyContract;

impl ComponentInterface for DependencyContract {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.attribute-gate.dependency@1").unwrap()
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<Request, Response>()
    }
}

impl phenix_sdk::SdkContract for DependencyContract {
    type Interface = DependencyContract;
}

struct UndeclaredContract;

impl ComponentInterface for UndeclaredContract {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse("fixture.attribute-gate.undeclared@1").unwrap()
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<Request, Response>()
    }
}

impl phenix_sdk::SdkContract for UndeclaredContract {
    type Interface = UndeclaredContract;
}

#[phenix_sdk::plugin("fixture.attribute-gate.sessions")]
mod sessions {
    use super::{Dependency, Internal, Request, Response};

    #[phenix(export(Internal), terminal)]
    fn internal(request: Request) -> Response {
        Response {
            value: format!("terminal:{}", request.prompt),
        }
    }

    #[phenix(export(Dependency), terminal)]
    fn dependency(request: Request) -> Response {
        Response {
            value: format!("dependency:{}", request.prompt),
        }
    }
}

fn authority(capability: &str) -> Authority {
    Authority::new([CapabilityId::parse(capability).unwrap()])
}

fn plugin_authority() -> Authority {
    Authority::new([
        CapabilityId::parse("plugin.run").unwrap(),
        CapabilityId::parse("kernel.persistence.schema").unwrap(),
        CapabilityId::parse("kernel.persistence.write").unwrap(),
        CapabilityId::parse("models.invoke").unwrap(),
        CapabilityId::parse("events.observe").unwrap(),
    ])
}

fn runtime_authority() -> Authority {
    Authority::new([
        CapabilityId::parse("plugin.run").unwrap(),
        CapabilityId::parse("kernel.persistence.schema").unwrap(),
        CapabilityId::parse("kernel.persistence.write").unwrap(),
        CapabilityId::parse("models.invoke").unwrap(),
        CapabilityId::parse("models.serve").unwrap(),
        CapabilityId::parse("models.layer").unwrap(),
        CapabilityId::parse("events.observe").unwrap(),
    ])
}

#[allow(dead_code)]
#[phenix_sdk::component]
struct Api {
    #[phenix(import, authority = authority("models.invoke"))]
    models: Required<Call<Dependency, Request, Response>>,

    #[phenix(event("fixture.attribute-gate.completed"))]
    completed: Emit<Response>,

    observed: AtomicUsize,
}

#[allow(dead_code)]
#[phenix_sdk::component]
impl Api {
    #[phenix(
        export(Models),
        public,
        terminal,
        authority = authority("models.serve")
    )]
    async fn run(&self, request: Request) -> Response {
        Response {
            value: request.prompt,
        }
    }

    #[phenix(export(Internal))]
    fn internal(&self, request: Request) -> Response {
        Response {
            value: request.prompt,
        }
    }

    #[phenix(
        layer(Internal, priority = 17, authority = authority("models.layer"))
    )]
    async fn policy(&self, _context: &phenix_sdk::LayerContext<'_, '_>, _request: Request) {}

    #[phenix(
        listen("fixture.attribute-gate.observed"),
        authority = authority("events.observe")
    )]
    async fn observed(
        &self,
        context: &phenix_sdk::EventContext<'_, '_>,
        response: Response,
    ) -> Result<(), String> {
        if context.emitter() != &sessions::Plugin::plugin_id() {
            return Err("listener lost emitter provenance".into());
        }
        if context.causality_id() != 41 {
            return Err("listener lost causality provenance".into());
        }
        let generation = context
            .graph_generation()
            .ok_or_else(|| "listener lost graph generation".to_owned())?;
        if context.plugin_context().plugin.id != &Plugin::plugin_id() {
            return Err("listener lost owner identity".into());
        }
        if context.kernel.cancellation_token().is_none() {
            return Err("listener has no cancellation scope".into());
        }
        let task_scope = context
            .kernel
            .task_scope()
            .ok_or_else(|| "listener has no task scope".to_owned())?;
        if task_scope.graph_generation() != generation
            || task_scope.plugin() != Some(&Plugin::plugin_id())
        {
            return Err("listener task scope lost owner or generation".into());
        }
        let requested_task_authority = Authority::new([
            CapabilityId::parse("models.invoke").unwrap(),
            CapabilityId::parse("models.serve").unwrap(),
        ]);
        let expected_task_authority = context.authority().attenuate(&requested_task_authority);
        let task = task_scope.spawn(&requested_task_authority, |token| {
            (token.graph_generation().clone(), token.authority().clone())
        });
        let (task_generation, task_authority) = task
            .join()
            .map_err(|_| "listener task panicked".to_owned())?;
        if &task_generation != generation || task_authority != expected_task_authority {
            return Err("listener task escaped generation or authority".into());
        }

        if context
            .sdk
            .require::<UndeclaredContract>()
            .invoke::<_, Response>(&Request {
                prompt: "undeclared".into(),
            })
            .is_ok()
        {
            return Err("listener invoked an undeclared import".into());
        }
        if context
            .kernel
            .transact_durable(
                &ResourceNamespace::parse("fixture.attribute-gate.foreign").unwrap(),
                &[TransactionOp::Put {
                    key: "forbidden".into(),
                    value: Vec::new(),
                }],
            )
            .is_ok()
        {
            return Err("listener wrote a foreign resource namespace".into());
        }

        let dependency = context
            .sdk
            .require::<DependencyContract>()
            .invoke::<_, Response>(&Request {
                prompt: response.value,
            })
            .map_err(|error| error.to_string())?;
        context
            .kernel
            .transact_durable(
                &ResourceNamespace::parse("fixture.attribute-gate.state").unwrap(),
                &[TransactionOp::Put {
                    key: "listener-import".into(),
                    value: dependency.value.into_bytes(),
                }],
            )
            .map_err(|error| error.to_string())?;
        self.observed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    #[phenix(value("fixture.attribute-gate.status@1"), public)]
    fn status(&self) -> u64 {
        1
    }

    #[phenix(value("fixture.attribute-gate.internal-status@1"))]
    fn internal_status(&self) -> u64 {
        0
    }
}

struct Store;

#[phenix_sdk::resource(schema = 1)]
impl Store {}

#[allow(dead_code)]
#[phenix_sdk::plugin(
    id = "fixture.attribute-gate",
    version = 7,
    authority = plugin_authority()
)]
struct Plugin {
    #[phenix(dep)]
    sessions: sessions::Plugin,

    #[phenix(component)]
    api: Api,

    #[phenix(resource)]
    state: phenix_sdk::Durable<Store>,

    #[phenix(config)]
    config: Settings,
}

#[allow(dead_code)]
#[phenix_sdk::plugin]
impl Plugin {
    #[phenix(start)]
    fn start(&mut self, context: &phenix_sdk::PluginContext<'_, '_, ()>) -> Result<(), String> {
        context
            .kernel
            .register_durable_schema(&DurableSchema::new(
                ResourceNamespace::parse("fixture.attribute-gate.state").unwrap(),
                1,
            ))
            .map_err(|error| error.to_string())
    }

    #[phenix(stop)]
    fn stop(&mut self, _context: &phenix_sdk::PluginContext<'_, '_, ()>) -> Result<(), String> {
        if self.api.observed.load(Ordering::Relaxed) == 1 {
            Ok(())
        } else {
            Err(format!(
                "generated listener mutated component state {} times",
                self.api.observed.load(Ordering::Relaxed)
            ))
        }
    }
}

fn plugin() -> Plugin {
    Plugin {
        sessions: sessions::Plugin,
        api: Api {
            models: Required::default(),
            completed: Emit::default(),
            observed: AtomicUsize::new(0),
        },
        state: phenix_sdk::Durable::default(),
        config: Settings { retries: 3 },
    }
}

#[test]
fn attribute_only_plugin_builds_graph_and_manifest_without_parallel_wiring() {
    fn assert_generated_dispatch<T: StaticPluginComponentDispatch>() {}
    fn assert_generated_runtime_dispatch<T: StaticComponentRuntimeDispatch>() {}
    assert_generated_dispatch::<Plugin>();
    assert_generated_runtime_dispatch::<Api>();

    let instance = plugin().__phenix_into_plugin_instance();
    drop(instance);

    let graph = phenix_sdk::StaticPluginGraph::compose::<Plugin>().unwrap();
    let ids = graph
        .ids()
        .map(phenix_sdk::PluginId::as_str)
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"fixture.attribute-gate.sessions"));
    assert!(ids.contains(&"fixture.attribute-gate"));

    let descriptor = graph
        .descriptor(&Plugin::plugin_id())
        .expect("root plugin descriptor exists");
    assert_eq!(descriptor.maximum_authority, plugin_authority());

    let components = <Plugin as StaticPluginComponents>::components();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), "fixture.attribute-gate.api");
    assert_eq!(components[0].imports().len(), 1);
    assert_eq!(
        components[0].imports()[0].authority,
        authority("models.invoke")
    );
    let component_manifest =
        components[0].manifest_with_authority(&Plugin::plugin_id(), &descriptor.maximum_authority);
    assert_eq!(component_manifest.maximum_authority, plugin_authority());
    assert_eq!(
        component_manifest.imports[0].authority,
        authority("models.invoke")
    );
    let services = components[0].services();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].required_authority, authority("models.layer"));

    let behavior = <Api as StaticComponentBehavior>::exports();
    assert_eq!(behavior.len(), 2);
    assert!(behavior[0].public);
    assert!(behavior[0].terminal);
    assert_eq!(behavior[0].required_authority, authority("models.serve"));
    assert!(!behavior[1].public);

    let layers = <Api as StaticComponentBehavior>::layers();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].required_authority, authority("models.layer"));

    let listeners = <Api as StaticComponentBehavior>::listeners();
    assert_eq!(listeners.len(), 1);
    assert_eq!(listeners[0].required_authority, authority("events.observe"));

    let public_callables = behavior
        .iter()
        .filter(|export| export.public)
        .map(|export| export.interface.as_str())
        .collect::<Vec<_>>();
    assert_eq!(public_callables, ["fixture.attribute-gate.models@1"]);

    let public_values = <Api as StaticComponentBehavior>::values()
        .into_iter()
        .filter(|value| value.public)
        .map(|value| value.id.to_string())
        .collect::<Vec<_>>();
    assert_eq!(public_values, ["fixture.attribute-gate.status@1"]);

    let resources = <Plugin as StaticPluginResources>::resources();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].id.as_str(), "fixture.attribute-gate.state");
    assert_eq!(resources[0].schema.version, 1);

    let config = <Plugin as StaticPluginConfiguration>::configuration()
        .expect("config field generates configuration metadata");
    assert_eq!(config.field, "config");

    let lifecycle = Plugin::lifecycle();
    assert_eq!(lifecycle.start, Some("start"));
    assert_eq!(lifecycle.stop, Some("stop"));

    let manifest = <Plugin as StaticPluginDefinition>::manifest();
    assert_eq!(manifest.id.as_str(), "fixture.attribute-gate");
    assert_eq!(manifest.version, 7);
    assert_eq!(manifest.services.len(), 1);
    assert_eq!(
        manifest.services[0].required_authority,
        authority("models.layer")
    );
    assert_eq!(manifest.maximum_authority, plugin_authority());
    assert_eq!(
        manifest.resource_namespaces[0].as_str(),
        "fixture.attribute-gate.state"
    );
}

#[test]
fn attribute_only_plugin_activates_generated_runtime_without_parallel_wiring() {
    let graph = phenix_sdk::StaticPluginGraph::compose::<Plugin>().unwrap();
    let manifests = [
        <sessions::Plugin as StaticPluginDefinition>::manifest(),
        <Plugin as StaticPluginDefinition>::manifest(),
    ];
    let sessions_component =
        <sessions::Plugin as StaticPluginDefinition>::component_manifests().remove(0);
    let sessions_component_id = sessions_component.id.clone();
    let components = [
        sessions_component,
        <Plugin as StaticPluginDefinition>::component_manifests().remove(0),
    ];
    let resolved = ResolvedHarness::resolve(
        manifests,
        components,
        std::iter::empty(),
        &runtime_authority(),
    )
    .unwrap();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    graph.preload_embedded_factories(&mut kernel).unwrap();
    graph
        .preload_embedded_instance::<Plugin>(&mut kernel, plugin().__phenix_into_plugin_instance())
        .unwrap();
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel.activate_all().unwrap();

    let generation = kernel
        .graph_generation()
        .expect("resolved runtime has a generation")
        .clone();
    let events = kernel.events();
    let (seen_diagnostics, diagnostics) = mpsc::channel::<EventEnvelope>();
    events
        .install_generation_subscriptions(
            generation,
            [EventSubscription {
                spec: SubscriptionSpec {
                    id: SubscriptionId::parse("fixture.attribute-gate/diagnostics").unwrap(),
                    owner: PluginId::parse("fixture.attribute-gate.diagnostic-probe").unwrap(),
                    event_type: EventTypeId::parse("kernel.structural_value_mismatch").unwrap(),
                    event_version: 1,
                    dependencies: Vec::new(),
                    failure_policy: EventFailurePolicy::FailDelivery,
                    required_authority: Authority::default(),
                    maximum_authority: Authority::default(),
                    kernel_policy_revision: 0,
                },
                handler: Arc::new(move |event: &EventEnvelope, _: &Authority| {
                    seen_diagnostics
                        .send(event.clone())
                        .map_err(|error| error.to_string())
                }),
            }],
        )
        .unwrap();

    let request = Request {
        prompt: "async-export".into(),
    };
    let input = serde_json::to_vec(&phenix_sdk::PhenixValue::from(&request)).unwrap();
    let output = kernel
        .invoke_component(
            &phenix_sdk::ComponentId::parse("fixture.attribute-gate.api").unwrap(),
            &ServiceId::parse("fixture.attribute-gate.models@1").unwrap(),
            &input,
            &authority("models.serve"),
            &Plugin::plugin_id(),
        )
        .unwrap();
    let output: phenix_sdk::PhenixValue = serde_json::from_slice(&output).unwrap();
    let output = Response::try_from(&output).unwrap();
    assert_eq!(output.value, "async-export");

    let request = Request {
        prompt: "async-layer".into(),
    };
    let input = serde_json::to_vec(&phenix_sdk::PhenixValue::from(&request)).unwrap();
    let output = kernel
        .invoke_component(
            &sessions_component_id,
            &ServiceId::parse("fixture.attribute-gate.internal@1").unwrap(),
            &input,
            &authority("models.layer"),
            &sessions::Plugin::plugin_id(),
        )
        .unwrap();
    let output: phenix_sdk::PhenixValue = serde_json::from_slice(&output).unwrap();
    let output = Response::try_from(&output).unwrap();
    assert_eq!(output.value, "terminal:async-layer");

    let response = Response {
        value: "observed".into(),
    };
    let payload = serde_json::to_vec(&phenix_sdk::PhenixValue::from(&response)).unwrap();
    let event = EventEnvelope {
        event_type: EventTypeId::parse("fixture.attribute-gate.observed").unwrap(),
        version: 1,
        emitter: sessions::Plugin::plugin_id(),
        causality_id: 41,
        kernel_policy_revision: 0,
        payload,
    };
    let listener_authority = Authority::new([
        CapabilityId::parse("events.observe").unwrap(),
        CapabilityId::parse("models.invoke").unwrap(),
        CapabilityId::parse("kernel.persistence.write").unwrap(),
    ]);
    let report = events.dispatch(&event, &listener_authority).unwrap();
    assert_eq!(report.delivered.len(), 1);
    assert!(report.failures.is_empty());
    assert!(report.warnings.is_empty());

    let attenuated_report = events
        .dispatch(&event, &authority("events.observe"))
        .unwrap();
    assert!(attenuated_report.delivered.is_empty());
    assert_eq!(attenuated_report.failures.len(), 1);
    assert_eq!(attenuated_report.warnings.len(), 1);
    assert!(attenuated_report.failures[0]
        .1
        .contains("kernel.persistence.write"));

    let mismatched = Request {
        prompt: "wrong-shape".into(),
    };
    let mismatch_event = EventEnvelope {
        causality_id: 42,
        payload: serde_json::to_vec(&phenix_sdk::PhenixValue::from(&mismatched)).unwrap(),
        ..event.clone()
    };
    let mismatch_report = events
        .dispatch(&mismatch_event, &authority("events.observe"))
        .unwrap();
    assert!(mismatch_report.delivered.is_empty());
    assert_eq!(mismatch_report.failures.len(), 1);
    assert_eq!(mismatch_report.warnings.len(), 1);

    let diagnostic = diagnostics.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(diagnostic.emitter, Plugin::plugin_id());
    assert_eq!(diagnostic.causality_id, 42);
    let diagnostic_payload: serde_json::Value =
        serde_json::from_slice(&diagnostic.payload).unwrap();
    assert_eq!(
        diagnostic_payload["event"],
        serde_json::json!("fixture.attribute-gate.observed")
    );
    assert_eq!(
        diagnostic_payload["listener"],
        serde_json::json!("observed")
    );
    assert_eq!(
        diagnostic_payload["direction"],
        serde_json::json!("listener")
    );
    let without_plugin = ResolvedHarness::resolve(
        [<sessions::Plugin as StaticPluginDefinition>::manifest()],
        [<sessions::Plugin as StaticPluginDefinition>::component_manifests().remove(0)],
        std::iter::empty(),
        &runtime_authority(),
    )
    .unwrap();
    GraphReconciler::new(resolved)
        .activate_candidate_on_kernel(&mut kernel, without_plugin)
        .unwrap();
    let report = events
        .dispatch(&event, &authority("events.observe"))
        .unwrap();
    assert!(report.delivered.is_empty());
    assert!(report.warnings.is_empty());
}

#[test]
fn attribute_only_gate_has_no_parallel_static_factory_wiring() {
    let source = include_str!("plugin_attribute_only_gate.rs");
    let register_factory = ["register_embedded_", "factory"].concat();
    let preload_factory = ["kernel.preload_embedded_", "factory"].concat();

    assert!(!source.contains(&register_factory));
    assert!(!source.contains(&preload_factory));
}
