//! Native product test: existing graph event dispatch registers provider data.
//! This is a test binary, not an alternative kernel scheduler or plugin API.
use phenix_contract::{KernelEvent, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, Selection},
    GraphCompletion, GraphStatus, Kernel, Outcome,
};
use phenix_provider_contracts::{ProviderPage, ProviderQuery};
use serde_json::json;

const REGISTRY: &str = "phenix.provider.registry@1";
const POPULATE: &str = "phenix.provider.populate@1";
const STARTUP: &str = "phenix.provider.startup@1";
const CATALOG: &str = "phenix.provider.catalog@1";
const RESOURCE: &str = "phenix.providers.user";
const HOST: &str = "phenix.provider.host";
const CONTRIBUTOR: &str = "phenix.provider.openai";

fn selection(contract: &str, operation: &str, provider: &str) -> Selection {
    Selection {
        interface: contract.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}

fn invoke<T: Portable>(
    kernel: &Kernel,
    selected: Selection,
    request: impl Portable,
) -> Result<T, String> {
    let root = kernel
        .admit_root(selected.clone())
        .map_err(|e| format!("{e:?}"))?;
    let reply = root
        .invoke_selected(&selected, &request.to_value())
        .map_err(|e| format!("{e:?}"))?;
    match reply {
        Outcome::Success(value) => T::from_value(value).map_err(|e| format!("{e:?}")),
        Outcome::Domain(error) => Err(format!("unexpected domain error: {error:?}")),
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: phenix-provider-population-test PROVIDERS OPENAI DEFINITIONS".into());
    }
    let grant = |caller: &str, interface: &str, operation: &str| {
        json!({
            "caller": caller,
            "interface": interface,
            "operation": operation,
            "resources": [RESOURCE],
        })
    };
    let composition: Composition = serde_json::from_value(json!({
        "format": "phenix.composition.v3",
        "entry": {
            "provider": HOST,
            "interface": REGISTRY,
            "operation": "begin",
        },
        "artifacts": [&args[1], &args[2]],
        "definitions": [&args[3]],
        "slots": [{
            "caller": CONTRIBUTOR,
            "slot": "Registry",
            "provider": HOST,
            "export": "Registry",
            "index": 0,
        }],
        "grants": [
            grant("root", REGISTRY, "begin"),
            grant("root", REGISTRY, "publish"),
            grant("root", REGISTRY, "abort"),
            grant("root", POPULATE, "register"),
            grant("root", CATALOG, "list"),
            grant(CONTRIBUTOR, POPULATE, "register"),
            grant(HOST, STARTUP, "begin"),
            grant(CONTRIBUTOR, REGISTRY, "register"),
        ],
        "excluded": [],
        "substitutions": [],
        "authority": {
            "operations": [
                format!("{REGISTRY}/begin"),
                format!("{REGISTRY}/register"),
                format!("{REGISTRY}/publish"),
                format!("{REGISTRY}/abort"),
                format!("{POPULATE}/register"),
                format!("{STARTUP}/begin"),
                format!("{CATALOG}/list"),
            ],
            "resources": [RESOURCE],
        },
        "resource": RESOURCE,
    }))
    .map_err(|e| format!("composition JSON: {e}"))?;
    let kernel = Prepared::prepare(composition)
        .map_err(|e| format!("offline preparation: {e:?}"))?
        .load()
        .map_err(|e| format!("native load: {e:?}"))?;
    let scope = ProviderQuery {
        scope: "user".into(),
    };
    // One trusted kernel begin event invokes the manager's onBegin handler.
    // The manager emits the custom populate event, and this same graph root
    // processes OpenAI's onPopulate subscriber in a following wave.
    let mut execution = kernel
        .begin_kernel_event(KernelEvent::Begin)
        .map_err(|e| format!("kernel.begin: {e:?}"))?;
    let mut finished = false;
    for _ in 0..32 {
        if execution.pump(|_| {}).map_err(|e| format!("{e:?}"))?
            == GraphStatus::Finished
        {
            finished = true;
            break;
        }
    }
    if !finished || !matches!(execution.completion(), Some(GraphCompletion::Success)) {
        return Err("provider population graph failed to complete successfully".into());
    }
    let steps = execution.steps().len();
    drop(execution);

    // Staged state is not an independently selectable catalog.
    let catalog = selection(CATALOG, "list", HOST);
    let root = kernel
        .admit_root(catalog.clone())
        .map_err(|e| format!("{e:?}"))?;
    let before = root
        .invoke_selected(&catalog, &scope.to_value())
        .map_err(|e| format!("{e:?}"))?;
    match before {
        Outcome::Domain(value) => {
            if !format!("{value:?}").contains("Unavailable") {
                return Err(format!(
                    "pre-publication catalog was not unavailable: {value:?}"
                ));
            }
        }
        Outcome::Success(_) => return Err("staged catalog leaked before publication".into()),
    }
    drop(root);

    if steps != 2 {
        return Err(format!("expected begin + populate graph nodes, got {steps}"));
    }

    let published: ProviderPage =
        invoke(&kernel, selection(REGISTRY, "publish", HOST), scope.clone())?;
    if published.revision != 1
        || published.providers.len() != 1
        || published.providers[0].connections.len() != 2
    {
        return Err(format!(
            "incorrect published provider snapshot: {published:?}"
        ));
    }
    let listed: ProviderPage = invoke(&kernel, catalog, scope)?;
    if listed != published {
        return Err("published catalog is not the host's selected catalog".into());
    }
    kernel.shutdown().map_err(|e| format!("shutdown: {e:?}"))?;
    println!(
        "{}",
        json!({
            "format": "phenix.provider-population-product.v1",
            "graph_steps": steps,
            "providers": published.providers.len(),
            "revision": published.revision,
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
