//! Native product: kernel.begin → tools.populate → reverse plugin hook
//! → owner settlement → selected tool invocation; no second scheduler.
use phenix_basic_contracts::{ToolCall, ToolPage, ToolQuery, ToolRequest, ToolResult};
use phenix_contract::{KernelEvent, Portable};
use phenix_kernel::{
    graph::{GraphCompletion, GraphStatus},
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use serde_json::json;

const MANAGER: &str = "phenix.basic.tools";
const CONTRIBUTOR: &str = "phenix.tool.fixture-reverse";
const RESOURCE: &str = "phenix.session.tool-session";
const CATALOG: &str = "phenix.basic.catalog@1";
const TOOL: &str = "phenix.basic.tool@1";
const EXECUTOR: &str = "phenix.basic.tool-registrations@1";
const REGISTRY: &str = "phenix.tools.registry@1";
const POPULATE: &str = "phenix.tools.populate@1";
const STARTUP: &str = "phenix.tools.startup@1";
const SETTLEMENT: &str = "phenix.tools.settlement@1";

fn selected(interface: &str, operation: &str) -> Selection {
    Selection {
        provider: MANAGER.into(),
        interface: interface.into(),
        operation: operation.into(),
    }
}

fn invoke<T: Portable>(
    kernel: &Kernel,
    select: Selection,
    request: impl Portable,
) -> Result<T, String> {
    let root = kernel
        .admit_root(select.clone())
        .map_err(|failure| format!("{failure:?}"))?;
    let value = root
        .invoke_selected(&select, &request.to_value())
        .map_err(|failure| format!("{failure:?}"))?;
    match value {
        Outcome::Success(value) => T::from_value(value).map_err(|failure| format!("{failure:?}")),
        Outcome::Domain(error) => Err(format!("unexpected domain rejection: {error:?}")),
    }
}

fn test() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: phenix-tool-population-test MANAGER FIXTURE DEFINITIONS".into());
    }
    let grant = |caller: &str, interface: &str, operation: &str| {
        json!({
            "caller": caller,
            "interface": interface,
            "operation": operation,
            "resources": [RESOURCE],
        })
    };
    let spec: Composition = serde_json::from_value(json!({
        "format": "phenix.composition.v3",
        "entry": {"provider": MANAGER, "interface": CATALOG, "operation": "list"},
        "artifacts": [&args[1], &args[2]],
        "definitions": [&args[3]],
        "slots": [
            {"caller": MANAGER, "slot": "Contributors", "provider": CONTRIBUTOR,
             "export": "Registrations", "index": 0},
            {"caller": CONTRIBUTOR, "slot": "Registry", "provider": MANAGER,
             "export": "Registry", "index": 0}
        ],
        "grants": [
            grant("root", CATALOG, "list"),
            grant("root", TOOL, "execute"),
            grant(MANAGER, STARTUP, "begin"),
            grant(MANAGER, SETTLEMENT, "complete"),
            grant(MANAGER, EXECUTOR, "list"),
            grant(MANAGER, EXECUTOR, "execute"),
            grant(CONTRIBUTOR, REGISTRY, "register"),
            grant(CONTRIBUTOR, POPULATE, "register")
        ],
        "authority": {
            "operations": [
                format!("{CATALOG}/list"),
                format!("{TOOL}/execute"),
                format!("{STARTUP}/begin"),
                format!("{SETTLEMENT}/complete"),
                format!("{EXECUTOR}/list"),
                format!("{EXECUTOR}/execute"),
                format!("{REGISTRY}/register"),
                format!("{POPULATE}/register")
            ],
            "resources": [RESOURCE]
        },
        "resource": RESOURCE,
        "excluded": [],
        "substitutions": []
    }))
    .map_err(|failure| format!("composition syntax: {failure}"))?;
    let kernel = Prepared::prepare(spec)
        .map_err(|failure| format!("offline preparation: {failure:?}"))?
        .load()
        .map_err(|failure| format!("native plugin load: {failure:?}"))?;
    let mut graph = kernel
        .begin_kernel_event(KernelEvent::Begin)
        .map_err(|failure| format!("kernel.begin: {failure:?}"))?;
    let mut finished = false;
    for _ in 0..32 {
        if graph.pump(|_| {}).map_err(|failure| format!("graph wave: {failure:?}"))?
            == GraphStatus::Finished
        {
            finished = true;
            break;
        }
    }
    if !finished || !matches!(graph.completion(), Some(GraphCompletion::Success)) {
        return Err("tool-population graph did not settle successfully".into());
    }
    let steps = graph.steps().len();
    drop(graph);
    if steps != 3 {
        return Err(format!("expected manager begin, contributor, settlement; got {steps}"));
    }

    let catalog: ToolPage = invoke(
        &kernel,
        selected(CATALOG, "list"),
        ToolQuery {
            session: "tool-session".into(),
            query: "".into(),
            limit: 32,
            cursor: None,
        },
    )?;
    let ids = catalog
        .tools
        .iter()
        .map(|tool| tool.id.as_str())
        .collect::<Vec<_>>();
    if ids != ["fail", "fixture.reverse", "length", "uppercase"] || catalog.revision == 0 {
        return Err(format!("incorrect atomically published tool catalog: {catalog:?}"));
    }
    let result: ToolResult = invoke(
        &kernel,
        selected(TOOL, "execute"),
        ToolRequest {
            session: "tool-session".into(),
            run: "one".into(),
            call: ToolCall {
                id: "reverse-1".into(),
                tool: "fixture.reverse".into(),
                arguments: r#""abß""#.into(),
            },
        },
    )?;
    if result.text != "ßba" {
        return Err(format!("wrong authenticated selected executor: {result:?}"));
    }
    kernel.shutdown().map_err(|failure| format!("shutdown: {failure:?}"))?;
    println!(
        "{}",
        json!({
            "format": "phenix.tool-population-product.v1",
            "graph_steps": steps,
            "catalog_tools": ids.len(),
            "revision": catalog.revision,
            "selected_execution": true
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = test() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
