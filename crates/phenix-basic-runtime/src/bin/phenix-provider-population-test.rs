//! Native product test: existing graph event dispatch registers provider data.
//! This is a test binary, not an alternative kernel scheduler or plugin API.
use phenix_contract::{KernelEvent, Portable};
use phenix_kernel::{
    graph::{GraphCompletion, GraphStatus},
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use phenix_provider_contracts::{ProviderPage, ProviderQuery};
use serde_json::json;

const REGISTRY: &str = "phenix.provider.registry@1";
const POPULATE: &str = "phenix.provider.populate@1";
const STARTUP: &str = "phenix.provider.startup@1";
const SETTLEMENT: &str = "phenix.provider.settlement@1";
const CATALOG: &str = "phenix.provider.catalog@1";
const RESOURCE: &str = "phenix.providers.user";
const HOST: &str = "phenix.provider.host";
const CONTRIBUTOR: &str = "phenix.provider.openai";
const STANDARDS: &str = "phenix.provider.standards";

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

/// A graph event with zero contributors still admits the declaring owner's
/// settled hook. The provider domain then rejects an empty catalog, and
/// neither a ready event nor a selected catalog may escape that failed wave.
fn verify_empty_population(host_artifact: &str, definitions: &str) -> Result<(), String> {
    let resource_grant = |caller: &str, interface: &str, operation: &str| {
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
        "artifacts": [host_artifact],
        "definitions": [definitions],
        "slots": [],
        "grants": [
            resource_grant("root", REGISTRY, "begin"),
            resource_grant("root", CATALOG, "list"),
            resource_grant(HOST, STARTUP, "begin"),
            resource_grant(HOST, SETTLEMENT, "complete"),
        ],
        "excluded": [],
        "substitutions": [],
        "authority": {
            "operations": [
                format!("{REGISTRY}/begin"),
                format!("{STARTUP}/begin"),
                format!("{SETTLEMENT}/complete"),
                format!("{CATALOG}/list"),
            ],
            "resources": [RESOURCE],
        },
        "resource": RESOURCE,
    }))
    .map_err(|e| format!("empty-composition JSON: {e}"))?;
    let kernel = Prepared::prepare(composition)
        .map_err(|e| format!("empty offline preparation: {e:?}"))?
        .load()
        .map_err(|e| format!("empty native load: {e:?}"))?;
    // The manager has no contributing plugin; runtime steps must still show
    // its on-begin handler followed by the owner-only settled handler.
    let mut execution = kernel
        .begin_kernel_event(KernelEvent::Begin)
        .map_err(|e| format!("empty kernel.begin: {e:?}"))?;
    let mut done = false;
    for _ in 0..32 {
        if execution
            .pump(|_| {})
            .map_err(|e| format!("empty pump: {e:?}"))?
            == GraphStatus::Finished
        {
            done = true;
            break;
        }
    }
    if !done || execution.completion() != Some(GraphCompletion::Domain) {
        return Err(format!(
            "empty settlement should reject an empty catalog: {:?}",
            execution.completion()
        ));
    }
    if execution.steps().len() != 2
        || execution.steps()[0].node != "phenix.provider.host/on-begin"
        || execution.steps()[1].node != "phenix.provider.host/on-populated"
        || !matches!(&execution.steps()[1].outcome, Outcome::Domain(_))
    {
        return Err(format!(
            "owner settlement not admitted after zero subscribers: {:?}",
            execution.steps()
        ));
    }
    drop(execution);
    let selected = selection(CATALOG, "list", HOST);
    let root = kernel
        .admit_root(selected.clone())
        .map_err(|e| format!("{e:?}"))?;
    let listed = root
        .invoke_selected(
            &selected,
            &ProviderQuery {
                scope: "user".into(),
            }
            .to_value(),
        )
        .map_err(|e| format!("{e:?}"))?;
    if !matches!(listed, Outcome::Domain(_)) {
        return Err("failed empty population leaked a selected catalog".into());
    }
    drop(root);
    kernel
        .shutdown()
        .map_err(|e| format!("empty shutdown: {e:?}"))?;
    Ok(())
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "usage: phenix-provider-population-test PROVIDERS OPENAI STANDARDS DEFINITIONS".into(),
        );
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
        "artifacts": [&args[1], &args[2], &args[3]],
        "definitions": [&args[4]],
        "slots": [{
            "caller": CONTRIBUTOR,
            "slot": "Registry",
            "provider": HOST,
            "export": "Registry",
            "index": 0,
        }, {
            "caller": STANDARDS,
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
            grant(STANDARDS, POPULATE, "register"),
            grant(HOST, STARTUP, "begin"),
            grant(HOST, SETTLEMENT, "complete"),
            grant(CONTRIBUTOR, REGISTRY, "register"),
            grant(STANDARDS, REGISTRY, "register"),
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
                format!("{SETTLEMENT}/complete"),
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
    // processes OpenAI and the five other standard declarations in one wave.
    let mut execution = kernel
        .begin_kernel_event(KernelEvent::Begin)
        .map_err(|e| format!("kernel.begin: {e:?}"))?;
    // Pump just the manager's begin handler: its custom event is queued,
    // but the provider registration wave has not executed yet.
    if execution.pump(|_| {}).map_err(|e| format!("{e:?}"))? == GraphStatus::Finished {
        return Err("kernel.begin finished before the populate event was dispatched".into());
    }
    let catalog = selection(CATALOG, "list", HOST);
    let before = execution
        .invoke_selected(&catalog, &scope.to_value())
        .map_err(|e| format!("{e:?}"))?;
    match before {
        Outcome::Domain(value) if format!("{value:?}").contains("Unavailable") => {}
        Outcome::Domain(value) => {
            return Err(format!("unexpected staging error: {value:?}"));
        }
        Outcome::Success(_) => return Err("staged catalog leaked before publication".into()),
    }

    let mut finished = false;
    for _ in 0..32 {
        if execution.pump(|_| {}).map_err(|e| format!("{e:?}"))? == GraphStatus::Finished {
            finished = true;
            break;
        }
    }
    if !finished || !matches!(execution.completion(), Some(GraphCompletion::Success)) {
        return Err("provider population graph failed to complete successfully".into());
    }
    let steps = execution.steps().len();
    drop(execution);

    if steps != 4 {
        return Err(format!(
            "expected begin, two independent contributors, and settlement, got {steps}"
        ));
    }
    // The provider manager, not an external controller, publishes the
    // immutable catalog on the generic settlement trigger.
    let listed: ProviderPage = invoke(&kernel, catalog, scope)?;
    let ids = listed
        .providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect::<Vec<_>>();
    let openai = listed
        .providers
        .iter()
        .find(|provider| provider.id == "openai");
    if listed.revision != 1
        || ids
            != [
                "anthropic",
                "openai",
                "opencode-go",
                "opencode-zen",
                "openrouter",
                "zai",
            ]
        || openai.is_none_or(|provider| provider.connections.len() != 2)
    {
        return Err(format!("incorrect published provider snapshot: {listed:?}"));
    }
    kernel.shutdown().map_err(|e| format!("shutdown: {e:?}"))?;
    verify_empty_population(&args[1], &args[4])?;
    println!(
        "{}",
        json!({
            "format": "phenix.provider-population-product.v1",
            "graph_steps": steps,
            "providers": listed.providers.len(),
            "revision": listed.revision,
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
