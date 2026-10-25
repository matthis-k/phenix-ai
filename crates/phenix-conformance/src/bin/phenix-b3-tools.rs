//! External ABI gate for the independently packaged Basic catalog/tool plugin.
use phenix_basic_contracts::{
    BasicError, ErrorCode, Scope, ToolCall, ToolPage, ToolQuery, ToolRequest, ToolResult,
};
use phenix_contract::{Authority, Code, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Kernel, Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selection(key: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: "phenix.basic.tools".into(),
    }
}

fn invoke(kernel: &Kernel, key: &str, request: Value) -> Result<Outcome, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(key))?;
    root.invoke(&request)
}

fn success<T: Portable>(result: Outcome) -> T {
    match result {
        Outcome::Success(value) => T::from_value(value).unwrap(),
        Outcome::Domain(value) => panic!("unexpected domain error: {value:?}"),
    }
}

fn domain(result: Outcome) -> BasicError {
    match result {
        Outcome::Domain(value) => BasicError::from_value(value).unwrap(),
        Outcome::Success(value) => panic!("unexpected success: {value:?}"),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "BASIC_TOOLS BASIC_DEFINITIONS OUTPUT");
    let output = Path::new(&args[3]);
    fs::create_dir_all(output).unwrap();
    let catalog = "phenix.basic.catalog@1/list";
    let execute = "phenix.basic.tool@1/execute";
    let count = "phenix.basic.tool@1/count";
    let startup = "phenix.tools.startup@1/begin";
    let settlement = "phenix.tools.settlement@1/complete";
    let resource = "phenix.session.s1";
    let resource_set = BTreeSet::from([resource.to_owned()]);
    let policy = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: vec![args[1].clone()],
        definitions: vec![args[2].clone()],
        entry: selection(catalog),
        slots: vec![],
        grants: [catalog, execute, count]
            .iter()
            .map(|key| {
                let target = selection(key);
                ScopedGrant {
                    caller: "root".into(),
                    interface: target.interface,
                    operation: target.operation,
                    resources: resource_set.clone(),
                }
            })
            .chain([startup, settlement].iter().map(|key| {
                let target = selection(key);
                ScopedGrant {
                    caller: "phenix.basic.tools".into(),
                    interface: target.interface,
                    operation: target.operation,
                    resources: resource_set.clone(),
                }
            }))
            .collect(),
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [catalog, execute, count, startup, settlement]
                .iter()
                .map(|key| (*key).to_owned())
                .collect(),
            resources: resource_set,
        },
        resource: Some(resource.into()),
    };
    let kernel = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let query = ToolQuery {
        session: "s1".into(),
        query: String::new(),
        cursor: None,
        limit: 1,
    };
    let first: ToolPage = success(invoke(&kernel, catalog, query.to_value()).unwrap());
    assert_eq!(first.tools[0].id, "fail");
    assert_eq!(first.revision, 1);
    let next: ToolPage = success(
        invoke(
            &kernel,
            catalog,
            ToolQuery {
                cursor: first.next.clone(),
                ..query.clone()
            }
            .to_value(),
        )
        .unwrap(),
    );
    assert_eq!(next.tools[0].id, "length");
    let stale = ToolQuery {
        query: "uppercase".into(),
        cursor: first.next,
        ..query.clone()
    };
    assert_eq!(
        domain(invoke(&kernel, catalog, stale.to_value()).unwrap()).code,
        ErrorCode::InvalidCursor
    );

    let call = |id: &str, tool: &str, arguments: &str| ToolRequest {
        session: "s1".into(),
        run: "r1".into(),
        call: ToolCall {
            id: id.into(),
            tool: tool.into(),
            arguments: arguments.into(),
        },
    };
    let uppercase: ToolResult = success(
        invoke(
            &kernel,
            execute,
            call("c1", "uppercase", r#""hello""#).to_value(),
        )
        .unwrap(),
    );
    assert_eq!(uppercase.text, "HELLO");
    let length: ToolResult =
        success(invoke(&kernel, execute, call("c2", "length", r#""aß""#).to_value()).unwrap());
    assert_eq!(length.text, "2");
    assert_eq!(
        domain(
            invoke(
                &kernel,
                execute,
                call("c3", "fail", r#""hello""#).to_value()
            )
            .unwrap()
        )
        .code,
        ErrorCode::ToolFailed
    );
    let total: u64 = success(
        invoke(
            &kernel,
            count,
            Scope {
                session: "s1".into(),
            }
            .to_value(),
        )
        .unwrap(),
    );
    assert_eq!(total, 3);

    for (tool, arguments, expected) in [
        ("missing", r#""x""#, ErrorCode::NotFound),
        ("uppercase", "not-json", ErrorCode::InvalidInput),
    ] {
        assert_eq!(
            domain(
                invoke(
                    &kernel,
                    execute,
                    call("invalid", tool, arguments).to_value()
                )
                .unwrap()
            )
            .code,
            expected
        );
    }
    let after: u64 = success(
        invoke(
            &kernel,
            count,
            Scope {
                session: "s1".into(),
            }
            .to_value(),
        )
        .unwrap(),
    );
    assert_eq!(after, total, "rejected tool caused a side effect");
    let mut cross = call("x1", "uppercase", r#""denied""#);
    cross.session = "s2".into();
    assert_eq!(
        domain(invoke(&kernel, execute, cross.to_value()).unwrap()).code,
        ErrorCode::Denied
    );
    let after_cross: u64 = success(
        invoke(
            &kernel,
            count,
            Scope {
                session: "s1".into(),
            }
            .to_value(),
        )
        .unwrap(),
    );
    assert_eq!(after_cross, total);
    drop(kernel);

    let mut denied = policy;
    denied.grants.retain(|grant| grant.operation != "execute");
    let kernel = Prepared::prepare(denied).unwrap().load().unwrap();
    assert_eq!(
        invoke(
            &kernel,
            execute,
            call("denied", "uppercase", r#""hello""#).to_value()
        )
        .unwrap_err()
        .code,
        Code::Denied
    );
    let zero: u64 = success(
        invoke(
            &kernel,
            count,
            Scope {
                session: "s1".into(),
            }
            .to_value(),
        )
        .unwrap(),
    );
    assert_eq!(zero, 0, "denied execution reached the plugin");
    fs::write(
        output.join("basic-tools.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.b3.tools.v1",
            "cases": [
                "catalog-pagination", "query-bound-cursor", "typed-uppercase",
                "unicode-length", "tool-domain-error", "invalid-json-zero-effects",
                "unknown-tool-zero-effects", "cross-session-zero-effects",
                "denied-execute-zero-effects"
            ],
            "independent_native_package": true,
            "catalog_does_not_authorize_execute": true
        }))
        .unwrap(),
    )
    .unwrap();
    println!("B3 native tools and catalog gate passed");
}
