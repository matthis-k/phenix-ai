//! Loads independently compiled SDK-only plugins through the existing kernel.
use phenix_contract::{Authority, Code, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Call, Outcome, Poll,
};
use std::{collections::BTreeSet, fs, path::Path};
const PROVIDER: &str = "test.authored-provider";
const CLIENT: &str = "test.authored-client";
const COUNTER: &str = "test.authored.counter@1";
const RELAY: &str = "test.authored.relay@1";
const PROPERTY: &str = "test.authored-provider.property.counter@1";
const OBSERVED: &str = "test.authored-provider.property.observed@1";
const FIELD_PROPERTY: &str = "test.authored-field.property.notes@1";
fn selection(interface: &str, operation: &str, provider: &str) -> Selection {
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn composition(artifacts: Vec<String>) -> Composition {
    let resources: BTreeSet<_> = [PROPERTY, OBSERVED, FIELD_PROPERTY]
        .map(|key| key.trim_end_matches("@1").to_owned())
        .into();
    let mut operations = BTreeSet::new();
    let mut grants = Vec::new();
    for (caller, interface, names) in [
        (
            "root",
            COUNTER,
            vec!["run", "reference", "observed_reference", "slow"],
        ),
        ("root", RELAY, vec!["run"]),
        ("root", PROPERTY, vec!["get", "set", "watch"]),
        ("root", OBSERVED, vec!["get", "watch"]),
        (CLIENT, COUNTER, vec!["run", "reference"]),
        (PROVIDER, PROPERTY, vec!["get", "set", "watch"]),
        (CLIENT, PROPERTY, vec!["get", "set", "watch"]),
        (CLIENT, FIELD_PROPERTY, vec!["get"]),
    ] {
        for operation in names {
            operations.insert(format!("{interface}/{operation}"));
            grants.push(ScopedGrant {
                caller: caller.into(),
                interface: interface.into(),
                operation: operation.into(),
                resources: resources.clone(),
            });
        }
    }
    Composition {
        format: "phenix.composition.v3".into(),
        definitions: vec![],
        slots: ["counter", "all"]
            .map(|slot| SlotBinding {
                caller: CLIENT.into(),
                slot: slot.into(),
                index: 0,
                provider: PROVIDER.into(),
                export: "counter".into(),
            })
            .into_iter()
            .chain([
                SlotBinding {
                    caller: CLIENT.into(),
                    slot: "count".into(),
                    index: 0,
                    provider: PROVIDER.into(),
                    export: "property.counter".into(),
                },
                SlotBinding {
                    caller: CLIENT.into(),
                    slot: "notes".into(),
                    index: 0,
                    provider: "test.authored-field".into(),
                    export: "property.notes".into(),
                },
            ])
            .collect(),

        artifacts,
        entry: selection(RELAY, "run", CLIENT),
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations,
            resources,
        },
        resource: None,
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 6, "PROVIDER CLIENT FIELD DEFINITIONS OUTPUT");
    let output = Path::new(&args[5]);
    fs::create_dir_all(output).unwrap();
    let mut c = composition(args[1..4].to_vec());
    c.definitions.push(args[4].clone());
    let prepared = Prepared::prepare(c.clone()).unwrap();
    // Property imports exist only through the two explicitly selected slots.
    assert!(prepared.inspect()["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|binding| binding["caller"] == CLIENT
            && binding["operation"]
                .as_str()
                .unwrap()
                .contains(".property."))
        .all(|binding| binding["operation"]
            .as_str()
            .is_some_and(|key| key.starts_with("count:") || key.starts_with("notes:"))));
    let field_package =
        phenix_artifact::VerifiedPackage::verify_trusted_read_only(Path::new(&args[3])).unwrap();
    let field = field_package.metadata();
    assert!(field.provides.is_empty());
    assert_eq!(field.properties.len(), 1);
    let kernel = prepared.load().unwrap();
    assert_eq!(kernel.inspect()["live_state"]["cells"], 3);
    assert_eq!(
        kernel
            .admit_root(selection(COUNTER, "slow", PROVIDER))
            .unwrap()
            .invoke(&Value::String("pending".into()))
            .unwrap_err()
            .code,
        Code::AbiIncompatible
    );
    assert_eq!(
        kernel
            .admit_default_root()
            .unwrap()
            .invoke(&Value::String("7".into()))
            .unwrap(),
        Outcome::Success(Value::U64(7))
    );
    assert_eq!(kernel.inspect()["live_state"]["epoch"], 1);
    for (input, expected) in [("locate-field", 0), ("locate-count", 7)] {
        assert_eq!(
            kernel
                .admit_default_root()
                .unwrap()
                .invoke(&Value::String(input.into()))
                .unwrap(),
            Outcome::Success(Value::U64(expected))
        );
    }
    for (input, code) in [
        ("lookup-native", Code::Denied),
        ("lookup-missing", Code::Denied),
        ("lookup-mismatch", Code::SchemaRejected),
    ] {
        assert_eq!(
            kernel
                .admit_default_root()
                .unwrap()
                .invoke(&Value::String(input.into()))
                .unwrap_err()
                .code,
            code
        );
    }
    assert_eq!(kernel.inspect()["live_state"]["epoch"], 1);
    assert_eq!(kernel.inspect()["live_state"]["subscriptions"], 0);
    // A published read-only property stays authoritative in Rust state.
    let owned = kernel
        .admit_root(selection(COUNTER, "observed_reference", PROVIDER))
        .unwrap()
        .invoke(&Value::map([] as [(&str, Value); 0]))
        .unwrap();
    let Outcome::Success(owned_reference) = owned else {
        panic!("owned property reference");
    };
    assert_eq!(
        kernel
            .admit_root(selection(COUNTER, "run", PROVIDER))
            .unwrap()
            .invoke(&Value::String("owned-publish".into()))
            .unwrap(),
        Outcome::Success(Value::U64(42))
    );
    assert_eq!(
        kernel
            .admit_root(selection(OBSERVED, "get", PROVIDER))
            .unwrap()
            .invoke(&owned_reference)
            .unwrap(),
        Outcome::Success(Value::U64(42))
    );
    // The owner can publish even though the property declares no external set.
    assert!(kernel
        .admit_root(selection(OBSERVED, "set", PROVIDER))
        .is_err());
    assert_eq!(
        kernel
            .admit_default_root()
            .unwrap()
            .invoke(&Value::String("domain".into()))
            .unwrap(),
        Outcome::Domain(Value::String("recoverable".into()))
    );
    let reference = kernel
        .admit_root(selection(COUNTER, "reference", PROVIDER))
        .unwrap()
        .invoke(&Value::map([] as [(&str, Value); 0]))
        .unwrap();
    let Outcome::Success(reference) = reference else {
        panic!("reference operation failed")
    };
    assert_eq!(
        kernel
            .admit_root(selection(PROPERTY, "get", PROVIDER))
            .unwrap()
            .invoke(&reference)
            .unwrap(),
        Outcome::Success(Value::U64(7))
    );
    {
        let root = kernel
            .admit_root(selection(COUNTER, "slow", PROVIDER))
            .unwrap();
        let Call::Pending(ticket) = root.begin(&Value::String("pending".into())).unwrap() else {
            panic!("expected pending")
        };
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        assert_eq!(
            root.invoke_selected(
                &selection(RELAY, "run", CLIENT),
                &Value::String("locate-count".into())
            )
            .unwrap(),
            Outcome::Success(Value::U64(7))
        );
        assert_eq!(
            root.invoke_selected(&selection(PROPERTY, "get", PROVIDER), &reference)
                .unwrap(),
            Outcome::Success(Value::U64(7))
        );
        let update = Value::map([
            ("expected", Value::U64(1)),
            ("location", reference.clone()),
            ("value", Value::U64(8)),
        ]);
        assert_eq!(
            root.invoke_selected(&selection(PROPERTY, "set", PROVIDER), &update)
                .unwrap(),
            Outcome::Success(Value::U64(2))
        );
        assert_eq!(
            root.invoke_selected(&selection(PROPERTY, "get", PROVIDER), &reference)
                .unwrap(),
            Outcome::Success(Value::U64(8))
        );
        assert!(matches!(
            root.invoke_selected(&selection(PROPERTY, "watch", PROVIDER), &reference)
                .unwrap(),
            Outcome::Success(Value::U64(_))
        ));
        let epoch = kernel.inspect()["live_state"]["epoch"].clone();
        assert_eq!(
            root.invoke_selected(
                &selection(COUNTER, "run", PROVIDER),
                &Value::String("99".into())
            )
            .unwrap_err()
            .code,
            Code::ReentrancyRejected
        );
        assert_eq!(
            root.invoke_selected(&selection(PROPERTY, "set", PROVIDER), &update)
                .unwrap(),
            Outcome::Domain(Value::String("Conflict".into()))
        );
        let Value::Ref(mut forged) = reference.clone() else {
            unreachable!()
        };
        forged.cell = phenix_contract::ValueId::from_bytes(&[91; 16]).unwrap();
        assert_eq!(
            root.invoke_selected(&selection(PROPERTY, "get", PROVIDER), &Value::Ref(forged))
                .unwrap(),
            Outcome::Domain(Value::String("StaleReference".into()))
        );
        assert_eq!(kernel.inspect()["live_state"]["epoch"], epoch);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        assert!(matches!(
            root.poll(&ticket).unwrap(),
            Poll::Ready(Ok(Outcome::Success(Value::U64(23))))
        ));
        root.release(&ticket).unwrap();
    }
    assert_eq!(kernel.inspect()["live_state"]["subscriptions"], 0);
    {
        let root = kernel
            .admit_root(selection(COUNTER, "slow", PROVIDER))
            .unwrap();
        let Call::Pending(ticket) = root.begin(&Value::String("pending".into())).unwrap() else {
            panic!("expected pending")
        };
        assert!(root.cancel(&ticket).unwrap());
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 0);
        root.release(&ticket).unwrap();
    }
    let before = kernel.inspect();
    kernel.shutdown().unwrap();
    for boundary in [
        "root-lookup-ceiling",
        "client-lookup-grant",
        "client-lookup-resource",
        "unselected-lookup",
    ] {
        let mut denied = c.clone();
        match boundary {
            "root-lookup-ceiling" => {
                denied
                    .authority
                    .operations
                    .remove(&format!("{PROPERTY}/get"));
            }
            "client-lookup-grant" => denied.grants.retain(|g| {
                !(g.caller == CLIENT && g.interface == PROPERTY && g.operation == "get")
            }),
            "client-lookup-resource" => {
                for grant in &mut denied.grants {
                    if grant.caller == CLIENT && grant.interface == PROPERTY {
                        grant.resources.clear();
                    }
                }
            }
            _ => (),
        }
        let kernel = Prepared::prepare(denied).unwrap().load().unwrap();
        let input = if boundary == "unselected-lookup" {
            "lookup-missing"
        } else {
            "locate-count"
        };
        assert_eq!(
            kernel
                .admit_default_root()
                .unwrap()
                .invoke(&Value::String(input.into()))
                .unwrap_err()
                .code,
            Code::Denied,
            "{boundary}"
        );
        assert_eq!(kernel.inspect()["live_state"]["epoch"], 0);
        assert_eq!(kernel.inspect()["live_state"]["subscriptions"], 0);
        kernel.shutdown().unwrap();
    }
    assert_eq!(kernel.inspect()["live_state"]["cells"], 0);
    for denied_boundary in [
        "root-ceiling",
        "client-import",
        "provider-property",
        "client-resource",
    ] {
        let mut denied = c.clone();
        match denied_boundary {
            "root-ceiling" => {
                denied
                    .authority
                    .operations
                    .remove(&format!("{PROPERTY}/set"));
            }
            "client-import" => denied.grants.retain(|g| {
                !(g.caller == CLIENT && g.interface == COUNTER && g.operation == "run")
            }),
            "provider-property" => denied
                .grants
                .retain(|g| !(g.caller == PROVIDER && g.operation == "set")),
            _ => {
                denied
                    .grants
                    .iter_mut()
                    .filter(|g| g.caller == CLIENT && g.operation == "run")
                    .for_each(|g| g.resources.clear());
            }
        }
        let kernel = Prepared::prepare(denied).unwrap().load().unwrap();
        assert_eq!(
            kernel
                .admit_default_root()
                .unwrap()
                .invoke(&Value::String("99".into()))
                .unwrap_err()
                .code,
            Code::Denied,
            "{denied_boundary}"
        );
        assert_eq!(kernel.inspect()["live_state"]["epoch"], 0);
        kernel.shutdown().unwrap();
    }
    {
        let mut denied = c.clone();
        let resource = OBSERVED.trim_end_matches("@1");
        denied
            .grants
            .iter_mut()
            .filter(|grant| {
                grant.caller == "root" && grant.interface == COUNTER && grant.operation == "run"
            })
            .for_each(|grant| {
                grant.resources.remove(resource);
            });
        let kernel = Prepared::prepare(denied).unwrap().load().unwrap();
        let reference = kernel
            .admit_root(selection(COUNTER, "observed_reference", PROVIDER))
            .unwrap()
            .invoke(&Value::map([] as [(&str, Value); 0]))
            .unwrap();
        let Outcome::Success(reference) = reference else {
            panic!("owned property reference");
        };
        assert_eq!(
            kernel
                .admit_root(selection(COUNTER, "run", PROVIDER))
                .unwrap()
                .invoke(&Value::String("owned-publish".into()))
                .unwrap_err()
                .code,
            Code::Denied,
            "owner publication must retain the property resource grant"
        );
        assert_eq!(
            kernel
                .admit_root(selection(OBSERVED, "get", PROVIDER))
                .unwrap()
                .invoke(&reference)
                .unwrap(),
            Outcome::Success(Value::U64(0)),
            "failed publication must not change the observed value"
        );
        assert_eq!(kernel.inspect()["live_state"]["epoch"], 0);
        kernel.shutdown().unwrap();
    }
    for denied_boundary in ["root-cell-grant", "root-cell-resource", "narrow-root"] {
        let mut denied = c.clone();
        if denied_boundary == "root-cell-grant" {
            denied
                .grants
                .retain(|g| !(g.caller == "root" && g.interface == PROPERTY));
        } else if denied_boundary == "root-cell-resource" {
            for grant in &mut denied.grants {
                if grant.caller == "root" && grant.interface == PROPERTY {
                    grant.resources.clear();
                }
            }
        }
        let kernel = Prepared::prepare(denied.clone()).unwrap().load().unwrap();
        let Outcome::Success(reference) = kernel
            .admit_root(selection(COUNTER, "reference", PROVIDER))
            .unwrap()
            .invoke(&Value::map([] as [(&str, Value); 0]))
            .unwrap()
        else {
            panic!("reference");
        };
        let mut authority = denied.authority.clone();
        if denied_boundary == "narrow-root" {
            authority
                .operations
                .retain(|key| !key.starts_with(PROPERTY));
        }
        let root = kernel
            .admit_scoped_root(
                selection(COUNTER, "slow", PROVIDER),
                phenix_contract::ScopedRequest {
                    authority,
                    resource: None,
                },
            )
            .unwrap();
        let Call::Pending(ticket) = root.begin(&Value::String("pending".into())).unwrap() else {
            panic!("pending worker");
        };
        for operation in ["get", "set", "watch"] {
            let request = if operation == "set" {
                Value::map([
                    ("expected", Value::U64(0)),
                    ("location", reference.clone()),
                    ("value", Value::U64(99)),
                ])
            } else {
                reference.clone()
            };
            assert_eq!(
                root.invoke_selected(&selection(PROPERTY, operation, PROVIDER), &request)
                    .unwrap_err()
                    .code,
                Code::Denied,
                "{denied_boundary}/{operation}"
            );
        }
        assert_eq!(
            root.invoke_selected(
                &selection(COUNTER, "run", "absent.provider"),
                &Value::String("99".into())
            )
            .unwrap_err()
            .code,
            Code::MissingBinding
        );
        assert_eq!(kernel.inspect()["live_state"]["epoch"], 0);
        assert_eq!(kernel.inspect()["live_state"]["subscriptions"], 0);
        assert_eq!(kernel.inspect()["pending"]["physical_leases"], 1);
        assert!(root.cancel(&ticket).unwrap());
        root.release(&ticket).unwrap();
        drop(root);
        kernel.shutdown().unwrap();
    }
    let kernel = Prepared::prepare(c.clone()).unwrap().load().unwrap();
    {
        let root = kernel
            .admit_root(selection(COUNTER, "slow", PROVIDER))
            .unwrap();
        for input in ["panic", "11"] {
            let Call::Pending(ticket) = root.begin(&Value::String(input.into())).unwrap() else {
                panic!("expected native ticket")
            };
            let Poll::Ready(Err(error)) = root.poll(&ticket).unwrap() else {
                panic!("expected typed plugin failure")
            };
            assert_eq!(error.code, Code::PluginFailed);
            root.release(&ticket).unwrap();
        }
    }
    kernel.shutdown().unwrap();
    fs::write(
        output.join("composition.json"),
        serde_json::to_vec_pretty(&c).unwrap(),
    )
    .unwrap();
    fs::write(output.join("authoring.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "format":"phenix.m1-native-authoring-evidence.v1", "all_passed":true,
        "cases":["sdk-only-provider-client-field", "definition-only-package", "generated-v3-native-glue", "field-binding-cleanup", "typed-import-slots", "optional-and-many", "delegation-ceiling-without-import", "synthetic-property-read", "domain-propagation", "pending-poll", "cancellation-quiescence", "root-client-provider-denied-write-zero-effects", "panic-poisoning-v2", "stateless-client-short-signature", "cell-get-set-watch-during-native-worker", "native-reentry-denied-during-worker", "conflict-forged-cell-zero-effects", "root-cell-grant-resource-scope-denials-zero-effects", "unknown-selection-denied-before-encoding", "root-watch-cleanup"],
        "property_discovery_cases":["independent-field-only-reader", "typed-get-set-watch-locations", "native-target-rejected", "unselected-slot-rejected", "typed-schema-mismatch", "discovery-during-physical-pending", "root-ceiling-denied", "import-grant-denied", "resource-denied", "zero-writes-and-subscriptions"],
        "before_stop":before,
    })).unwrap()).unwrap();
    println!("S3b native authoring slice passed");
}
