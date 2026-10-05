use phenix_core::{Kernel, ResolvedGeneration, ResolvedGenerationActivation, ServiceId};
use phenix_sdk::{
    Authority, HasPhenixSchema, StaticPluginComponents, StaticPluginDefinition,
    StaticPluginPublicProjection,
};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Request {
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Response {
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct Changed {
    value: String,
}

#[phenix_sdk::interface("fixture.public-projection.call@1")]
struct PublicCall;

#[phenix_sdk::interface("fixture.public-projection.private@1")]
struct PrivateCall;

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(export(PublicCall), public, terminal)]
    fn public_call(&self, request: Request) -> Response {
        Response {
            value: request.value,
        }
    }

    #[phenix(export(PrivateCall))]
    fn private_call(&self, request: Request) -> Response {
        Response {
            value: request.value,
        }
    }

    #[phenix(value("fixture.public-projection.status@1"), public)]
    fn status(&self) -> u64 {
        1
    }

    #[phenix(value("fixture.public-projection.internal@1"))]
    fn internal(&self) -> u64 {
        0
    }
}

#[phenix_sdk::plugin("fixture.public-projection")]
struct Plugin {
    #[phenix(component)]
    api: Api,
}

#[phenix_sdk::plugin(root, id = "fixture.root-public")]
struct RootPlugin {
    calls: AtomicUsize,
}

#[phenix_sdk::plugin]
impl RootPlugin {
    #[phenix(expose(name = "run"))]
    async fn execute(&self, request: Request) -> Response {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Response {
            value: request.value,
        }
    }

    #[phenix(on_event("fixture.root-public.changed"))]
    async fn changed(&self, _context: &phenix_sdk::EventContext<'_, '_>, _event: Changed) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }

    fn helper(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct CountRequest {
    amount: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct CountResponse {
    count: u64,
}

fn recursive_authority() -> Authority {
    Authority::new([phenix_sdk::PermissionId::parse("fixture.recursive.read").unwrap()])
}

#[phenix_sdk::interface("fixture.recursive-public/public/status@1")]
struct ExplicitStatus;

#[phenix_sdk::expose]
struct Counter {
    count: AtomicU64,
}

#[phenix_sdk::expose]
impl Counter {
    #[phenix(expose(name = "add"))]
    fn increment(&self, request: CountRequest) -> CountResponse {
        let count = self.count.fetch_add(request.amount, Ordering::Relaxed) + request.amount;
        CountResponse { count }
    }

    #[phenix(expose(authority = recursive_authority()))]
    async fn read(&self, _request: CountRequest) -> CountResponse {
        CountResponse {
            count: self.count.load(Ordering::Relaxed),
        }
    }

    fn hidden(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

#[phenix_sdk::expose]
struct Branch {
    #[phenix(expose(name = "leaf"))]
    counter: Counter,
    hidden: Counter,
}

#[phenix_sdk::expose]
impl Branch {
    #[phenix(expose(name = "branch_state"))]
    fn state(&self, _request: CountRequest) -> CountResponse {
        CountResponse {
            count: self.counter.count.load(Ordering::Relaxed),
        }
    }

    fn hidden(&self) -> u64 {
        self.hidden.count.load(Ordering::Relaxed)
    }
}

#[phenix_sdk::component]
struct Worker;

#[phenix_sdk::component]
impl Worker {}

#[phenix_sdk::plugin(
    root,
    id = "fixture.recursive-public",
    authority = recursive_authority(),
    remap(from = "status", to = "health"),
    remap(from = "branch", to = "tree"),
    remap(from = "branch/leaf/add", to = "api/increment")
)]
struct RecursivePlugin {
    #[phenix(expose(name = "branch"))]
    nested: Branch,
    #[phenix(expose)]
    left: Counter,
    #[phenix(expose(name = "right"))]
    second: Counter,
    #[phenix(component)]
    worker: Worker,
    hidden: Counter,
    root_calls: AtomicU64,
}

#[phenix_sdk::plugin]
impl RecursivePlugin {
    #[phenix(expose)]
    fn root_sync(&self, request: CountRequest) -> CountResponse {
        let count = self.root_calls.fetch_add(request.amount, Ordering::Relaxed) + request.amount;
        CountResponse { count }
    }

    #[phenix(expose(name = "status"))]
    async fn root_async(&self, _request: CountRequest) -> CountResponse {
        CountResponse {
            count: self.root_calls.load(Ordering::Relaxed),
        }
    }

    #[phenix(export(ExplicitStatus), public)]
    fn explicit_status(&self, _request: CountRequest) -> CountResponse {
        CountResponse {
            count: self.root_calls.load(Ordering::Relaxed),
        }
    }

    fn hidden(&self) -> u64 {
        self.hidden.count.load(Ordering::Relaxed)
    }
}

#[phenix_sdk::plugin(
    root,
    id = "fixture.recursive-collision",
    remap(from = "nested/add", to = "same")
)]
struct CollisionPlugin {
    #[phenix(expose(name = "nested"))]
    counter: Counter,
}

#[phenix_sdk::plugin]
impl CollisionPlugin {
    #[phenix(expose(name = "same"))]
    fn direct(&self, request: CountRequest) -> CountResponse {
        self.counter.increment(request)
    }
}

#[test]
fn public_projection_is_derived_from_ordinary_component_contributions() {
    let projection = Plugin::public_projection();

    assert_eq!(projection.callables.len(), 1);
    assert_eq!(
        projection.callables[0].component.as_str(),
        "fixture.public-projection.api"
    );
    assert_eq!(
        projection.callables[0].interface.as_str(),
        "fixture.public-projection.call@1"
    );
    assert_eq!(projection.callables[0].path, ["public_call"]);
    assert_eq!(projection.callables[0].method, "public_call");

    assert_eq!(projection.values.len(), 1);
    assert_eq!(
        projection.values[0].component.as_str(),
        "fixture.public-projection.api"
    );
    assert_eq!(
        projection.values[0].id.as_str(),
        "fixture.public-projection.status@1"
    );
    assert_eq!(projection.values[0].method, "status");
    assert_eq!(
        projection.values[0].value_type,
        std::any::type_name::<u64>()
    );
    assert_eq!(projection.values[0].schema, u64::phenix_schema());

    let manifest = <Plugin as StaticPluginDefinition>::manifest();
    assert_eq!(manifest.id.as_str(), "fixture.public-projection");

    let plugin = Plugin { api: Api };
    let public = plugin.api.public_call(Request {
        value: "public".into(),
    });
    assert_eq!(public.value, "public");
    let private = plugin.api.private_call(Request {
        value: "private".into(),
    });
    assert_eq!(private.value, "private");
    assert_eq!(plugin.api.status(), 1);
    assert_eq!(plugin.api.internal(), 0);
}

#[test]
fn root_exposure_uses_member_name_without_an_api_redirect() {
    let projection = RootPlugin::public_projection();

    assert_eq!(projection.callables.len(), 1);
    let callable = &projection.callables[0];
    assert_eq!(callable.component.as_str(), "fixture.root-public");
    assert_eq!(
        callable.interface.as_str(),
        "fixture.root-public/public/run@1"
    );
    assert_eq!(callable.path, ["run"]);
    assert_eq!(callable.method, "execute");

    let components = <RootPlugin as StaticPluginComponents>::components();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].id.as_str(), "fixture.root-public");
    assert_eq!(components[0].exports().len(), 1);
    assert_eq!(components[0].listeners().len(), 1);
    assert_eq!(
        components[0].listeners()[0].event.as_str(),
        "fixture.root-public.changed"
    );

    let plugin = RootPlugin {
        calls: AtomicUsize::new(0),
    };
    assert_eq!(plugin.helper(), 0);
    drop(plugin.__phenix_into_plugin_instance());
}

fn recursive_plugin() -> RecursivePlugin {
    RecursivePlugin {
        nested: Branch {
            counter: Counter {
                count: AtomicU64::new(0),
            },
            hidden: Counter {
                count: AtomicU64::new(100),
            },
        },
        left: Counter {
            count: AtomicU64::new(10),
        },
        second: Counter {
            count: AtomicU64::new(20),
        },
        worker: Worker,
        hidden: Counter {
            count: AtomicU64::new(1_000),
        },
        root_calls: AtomicU64::new(0),
    }
}

fn invoke_count(kernel: &mut Kernel, path: &str, amount: u64) -> CountResponse {
    let request = CountRequest { amount };
    let input = serde_json::to_vec(&phenix_sdk::PhenixValue::from(&request)).unwrap();
    let output = kernel
        .invoke_component(
            &RecursivePlugin::component_id(),
            &ServiceId::parse(format!("fixture.recursive-public/public/{path}@1")).unwrap(),
            &input,
            &recursive_authority(),
            &RecursivePlugin::plugin_id(),
        )
        .unwrap();
    let output: phenix_sdk::PhenixValue = serde_json::from_slice(&output).unwrap();
    CountResponse::try_from(&output).unwrap()
}

#[test]
fn recursive_exposure_projects_and_dispatches_real_nested_state() {
    let projection = RecursivePlugin::public_projection();
    let paths = projection
        .callables
        .iter()
        .map(|callable| callable.path.join("/"))
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "root_sync",
            "health",
            "status",
            "tree/branch_state",
            "api/increment",
            "tree/leaf/read",
            "left/add",
            "left/read",
            "right/add",
            "right/read",
        ]
    );
    assert!(paths.iter().all(|path| !path.contains("hidden")));
    assert!(projection.callables.iter().all(|callable| callable.schema
        == phenix_core::InterfaceSchema::of::<CountRequest, CountResponse>()));
    for callable in &projection.callables {
        let expected = if callable.path.last().is_some_and(|name| name == "read") {
            recursive_authority()
        } else {
            Authority::default()
        };
        assert_eq!(callable.required_authority, expected);
    }
    assert!(projection.callables.iter().any(|callable| {
        callable.method == "explicit_status"
            && callable.interface.as_str() == "fixture.recursive-public/public/status@1"
    }));
    assert_eq!(RecursivePlugin::component_manifests().len(), 2);

    let graph = phenix_sdk::StaticPluginGraph::compose::<RecursivePlugin>().unwrap();
    let resolved = ResolvedGeneration::resolve(
        [RecursivePlugin::manifest()],
        RecursivePlugin::component_manifests(),
        std::iter::empty(),
        &recursive_authority(),
    )
    .unwrap();
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    let plugin = recursive_plugin();
    assert_eq!(plugin.hidden(), 1_000);
    assert_eq!(plugin.nested.hidden(), 100);
    assert_eq!(plugin.left.hidden(), 10);
    graph
        .preload_embedded_instance::<RecursivePlugin>(
            &mut kernel,
            plugin.__phenix_into_plugin_instance(),
        )
        .unwrap();
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(invoke_count(&mut kernel, "root_sync", 3).count, 3);
    assert_eq!(invoke_count(&mut kernel, "health", 0).count, 3);
    assert_eq!(invoke_count(&mut kernel, "status", 0).count, 3);
    assert_eq!(invoke_count(&mut kernel, "api/increment", 4).count, 4);
    assert_eq!(invoke_count(&mut kernel, "tree/leaf/read", 0).count, 4);
    assert_eq!(invoke_count(&mut kernel, "left/add", 2).count, 12);
    assert_eq!(invoke_count(&mut kernel, "left/read", 0).count, 12);
    assert_eq!(invoke_count(&mut kernel, "right/add", 5).count, 25);
    assert_eq!(invoke_count(&mut kernel, "right/read", 0).count, 25);
}

#[test]
fn final_recursive_collision_uses_component_graph_validation() {
    let error = ResolvedGeneration::resolve(
        [CollisionPlugin::manifest()],
        CollisionPlugin::component_manifests(),
        std::iter::empty(),
        &Authority::default(),
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("exports interface fixture.recursive-collision/public/same@1 more than once"));
}
