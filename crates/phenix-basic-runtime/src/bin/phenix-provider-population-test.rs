//! Native product test: existing graph event dispatch registers provider data.
//! This is a test binary, not an alternative kernel scheduler or plugin API.
use phenix_contract::{Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use phenix_provider_contracts::{ProviderPage, ProviderQuery, ProvidersPopulate};
use serde_json::json;

const REGISTRY: &str = "phenix.provider.registry@1";
const POPULATE: &str = "phenix.provider.populate@1";
const CATALOG: &str = "phenix.provider.catalog@1";
const RESOURCE: &str = "phenix.providers.user";
const HOST: &str = "phenix.provider.host";
const CONTRIBUTOR: &str = "phenix.provider.populate-builtins";

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
    let root = kernel.admit_root(selected.clone()).map_err(|e| format!("{e:?}"))?;
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
        return Err("usage: phenix-provider-population-test HOST CONTRIBUTOR DEFINITIONS".into());
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
    invoke::<()>(&kernel, selection(REGISTRY, "begin", HOST), scope.clone())?;

    // Staged state is not an independently selectable catalog.
    let catalog = selection(CATALOG, "list", HOST);
    let root = kernel.admit_root(catalog.clone()).map_err(|e| format!("{e:?}"))?;
    let before = root
        .invoke_selected(&catalog, &scope.to_value())
        .map_err(|e| format!("{e:?}"))?;
    match before {
        Outcome::Domain(value) => {
            if !format!("{value:?}").contains("Unavailable") {
                return Err(format!("pre-publication catalog was not unavailable: {value:?}"));
            }
        }
        Outcome::Success(_) => return Err("staged catalog leaked before publication".into()),
    }
    drop(root);

    let steps = kernel
        .submit_action(
            "phenix.providers.populate-action",
            &ProvidersPopulate {
                scope: "user".into(),
            }
            .to_value(),
        )
        .map_err(|e| format!("graph population: {e:?}"))?;
    if steps.len() != 1 {
        return Err(format!("expected one event subscriber, got {}", steps.len()));
    }

    let published: ProviderPage = invoke(
        &kernel,
        selection(REGISTRY, "publish", HOST),
        scope.clone(),
    )?;
    if published.revision != 1 || published.providers.len() != 6 {
        return Err(format!("incorrect published provider snapshot: {published:?}"));
    }
    let listed: ProviderPage = invoke(&kernel, catalog, scope)?;
    if listed != published {
        return Err("published catalog is not the host's selected catalog".into());
    }
    kernel.shutdown().map_err(|e| format!("shutdown: {e:?}"))?;
    println!("{}", json!({
        "format": "phenix.provider-population-product.v1",
        "graph_steps": steps.len(),
        "providers": published.providers.len(),
        "revision": published.revision,
    }));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
