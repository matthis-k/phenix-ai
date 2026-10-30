use phenix_artifact::VerifiedPackage;
use phenix_contract::{graph::v2::*, Authority, Value};
use phenix_kernel::{
    graph::v2::{OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy},
    resolve::{Composition, Prepared, ScopedGrant, Selection},
    Kernel, Outcome,
};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Barrier},
};

fn load(package: String, owner: &str, contract: &str) -> Kernel {
    let verified = VerifiedPackage::verify_trusted_read_only(Path::new(&package)).unwrap();
    let security = PortSecurity {
        confidentiality: Classification::Public,
        integrity: IntegrityClass::Untrusted,
    };
    let operation = format!("{contract}/greet");
    let mut trusted = TrustedCompositionPolicy::default();
    trusted.source_owners.insert(
        owner.into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Public,
            composition_ceiling: Classification::Public,
        },
    );
    trusted.sources.insert(format!("{owner}/source"), security);
    trusted.operations.insert(
        (owner.into(), operation.clone()),
        OperationSecurity {
            input: security,
            ok: security,
            err: security,
        },
    );
    assert_eq!(verified.graph_v2().unwrap().contributor, owner);
    Prepared::prepare_graph_v2(
        Composition {
            format: "phenix.composition.v3".into(),
            definitions: vec![],
            artifacts: vec![package],
            entry: Selection {
                interface: contract.into(),
                operation: "greet".into(),
                provider: owner.into(),
            },
            slots: vec![],
            grants: ["root", owner]
                .into_iter()
                .map(|caller| ScopedGrant {
                    caller: caller.into(),
                    interface: contract.into(),
                    operation: "greet".into(),
                    resources: BTreeSet::new(),
                })
                .collect(),
            authority: Authority {
                operations: BTreeSet::from([operation]),
                resources: BTreeSet::new(),
            },
            excluded: BTreeSet::new(),
            substitutions: vec![],
            resource: None,
        },
        &trusted,
    )
    .unwrap()
    .load()
    .unwrap()
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        3,
        "regressions A-PACKAGE B-PACKAGE SCALAR-PACKAGE"
    );
    // Initialize the process interpreter before testing concurrent *module* init.
    // Independent PyO3 images do not serialize a cold CPython startup.
    let scalar = load(args[2].clone(), "example.scalar", "example.scalar@1");
    for count in 1..=2 {
        assert_eq!(
            scalar
                .admit_default_root()
                .unwrap()
                .invoke(&Value::String("hello".into()))
                .unwrap(),
            Outcome::Success(Value::String(format!("scalar:hello:{count}")))
        );
    }
    println!("PASS scalar str -> str runtime: scalar:hello:1, scalar:hello:2 (process interpreter initialized)");
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = args[..2]
        .iter()
        .cloned()
        .zip(["a", "b"])
        .map(|(package, token)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                // Kernels and roots stay on their owning thread, not sent between threads.
                let kernel = load(
                    package,
                    &format!("example.concurrent-{token}"),
                    "example.concurrent@1",
                );
                barrier.wait();
                for count in 1..=2 {
                    let request = Value::map([("text", Value::String("input".into()))]);
                    let expected = Value::map([(
                        "text",
                        Value::String(format!("{}:input:{count}", token.to_uppercase())),
                    )]);
                    assert_eq!(
                        kernel
                            .admit_default_root()
                            .unwrap()
                            .invoke(&request)
                            .unwrap(),
                        Outcome::Success(expected)
                    );
                }
                kernel.shutdown().unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    println!("PASS concurrent independent images: A:input:1, A:input:2; B:input:1, B:input:2 (GIL-releasing module init)");
    scalar.shutdown().unwrap();
    println!("PASS compiled Operation/ValueSchema dataclasses; replacement sys.modules entries preserved");
}
