//! Cross-cutting regressions retained when the wave engine was removed.
use phenix_graph::{
    Contract, ContractValue, Interface, NetworkBuilder, PortDef, Process, ProcessContext,
    ProcessEmission, ResourceGrant, Schema, Step, Store, TypeIdentity, Value,
};
use std::{num::NonZeroUsize, sync::Arc};

struct Relay;
impl Process for Relay {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        match context.take("in") {
            Some(value) => Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::of(value.get::<i64>()?),
            ))),
            None => Ok(Step::Complete),
        }
    }
}

#[test]
fn store_preserves_typed_replacement_and_grants_do_not_follow_claims() {
    let mut store = Store::new();
    assert!(store.get::<i64>("counter").unwrap_err().contains("absent"));
    store.insert("counter", 1_i64);
    assert!(
        store
            .get::<String>("counter")
            .unwrap_err()
            .contains("wrong Rust type")
    );
    store.insert("counter", 7_i64);
    let store = Arc::new(store);
    let denied = ProcessContext::default();
    assert!(denied.read::<i64, _>("counter", |x| *x).is_err());
    let read = ProcessContext::default()
        .with_resources(vec![ResourceGrant::read(store.clone(), "counter")]);
    assert_eq!(read.read::<i64, _>("counter", |x| *x).unwrap(), 7);
    assert!(read.write::<i64, _>("counter", |x| *x += 1).is_err());
    let write = ProcessContext::default()
        .with_resources(vec![ResourceGrant::write(store.clone(), "counter")]);
    write.write::<i64, _>("counter", |x| *x += 1).unwrap();
    assert_eq!(*store.get::<i64>("counter").unwrap().read().unwrap(), 8);
    assert!(write.read::<i64, _>("other", |x| *x).is_err());
    assert!(write.read::<String, _>("counter", Clone::clone).is_err());
    let claim = Contract::new("resource/counter", Arc::new(Interface::default())).unwrap();
    claim.check_shape(&Interface::default()).unwrap();
    assert!(denied.write::<i64, _>("counter", |x| *x = 0).is_err());
}

#[test]
fn malformed_wire_payload_fails_without_poisoning_an_independent_session() {
    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let relay = builder
        .node("relay", interface, Arc::new(|| Relay))
        .unwrap();
    builder
        .expose_input(
            "in",
            relay.wire_input::<i64>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("out", relay.wire_output::<i64>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut bad = compiled.start();
    bad.inject(
        "in",
        Value::from_wire_parts(i64::schema().identity(), vec![1]),
    )
    .unwrap();
    bad.run_until_idle(100);
    assert_eq!(bad.state(), phenix_graph::runtime::SessionState::Failed);
    assert!(bad.take_output("out").unwrap().is_none());
    let mut good = compiled.start();
    good.inject("in", Value::of(42_i64)).unwrap();
    good.run_until_idle(100);
    assert_eq!(
        good.take_output("out")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        42
    );
}

#[test]
fn option_and_result_remain_data() {
    struct Pass;
    impl Process for Pass {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            Ok(match context.take("in") {
                Some(value) => Step::Emit(ProcessEmission::new("out", value)),
                None => Step::Complete,
            })
        }
    }
    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::local::<Option<i64>>("in")],
            [PortDef::local::<Option<i64>>("out")],
        )
        .unwrap(),
    );
    let node = builder.node("pass", interface, Arc::new(|| Pass)).unwrap();
    builder
        .expose_input(
            "in",
            node.input::<Option<i64>>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("out", node.output::<Option<i64>>("out").unwrap())
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    for expected in [None, Some(7_i64)] {
        session.inject("in", Value::local(expected)).unwrap();
        session.run_until_idle(100);
        let packet = session.take_output("out").unwrap().unwrap();
        assert_eq!(packet.value().borrow::<Option<i64>>(), Some(&expected));
    }
    let error: Result<i64, String> = Err("ordinary data".into());
    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::local::<Result<i64, String>>("in")],
            [PortDef::local::<Result<i64, String>>("out")],
        )
        .unwrap(),
    );
    let node = builder
        .node("result", interface, Arc::new(|| Pass))
        .unwrap();
    builder
        .expose_input(
            "in",
            node.input::<Result<i64, String>>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("out", node.output::<Result<i64, String>>("out").unwrap())
        .unwrap();
    let mut session = builder.build().unwrap().compile().unwrap().start();
    session.inject("in", Value::local(error.clone())).unwrap();
    session.run_until_idle(100);
    let packet = session.take_output("out").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<Result<i64, String>>(), Some(&error));
}

#[test]
fn network_schema_metadata_and_captures_release_after_unload() {
    struct Echo;
    impl Process for Echo {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            Ok(match context.take("in") {
                Some(value) => Step::Emit(ProcessEmission::new("out", value)),
                None => Step::Complete,
            })
        }
    }
    for revision in 0..128 {
        let schema = Schema {
            name: format!("reload/{revision}"),
            revision,
            structure: "i64".into(),
            codec: "i64-le-v1".into(),
        }
        .identity();
        let weak = Arc::downgrade(&schema);
        let packet = {
            let interface = Arc::new(
                Interface::new(
                    [PortDef {
                        name: "in".into(),
                        value: TypeIdentity::Wire(schema.clone()),
                    }],
                    [PortDef {
                        name: "out".into(),
                        value: TypeIdentity::Wire(schema.clone()),
                    }],
                )
                .unwrap(),
            );
            let mut builder = NetworkBuilder::new();
            let node = builder.node("echo", interface, Arc::new(|| Echo)).unwrap();
            builder
                .expose_input_dynamic(
                    "in",
                    &node.input_endpoint("in").unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            builder
                .expose_output_dynamic("out", &node.output_endpoint("out").unwrap())
                .unwrap();
            let definition = builder.build().unwrap();
            let compiled = definition.compile().unwrap();
            let independently_compiled = definition.compile().unwrap();
            let mut session = compiled.start();
            session
                .inject("in", Value::from_wire_parts(schema, 42_i64.encode()))
                .unwrap();
            session.run_until_idle(100);
            let packet = session.take_output("out").unwrap().unwrap();
            drop(independently_compiled);
            packet
        };
        assert!(weak.upgrade().is_some());
        let clone = packet.value().clone();
        drop(packet);
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(
            weak.upgrade().is_none(),
            "reload {revision} retained schema metadata"
        );
    }
}

#[test]
fn generated_dags_and_contributor_orders_match_an_independent_reference() {
    use phenix_graph::network::ConnectionDef;
    use phenix_graph::runtime::{Readiness, SessionOptions};
    use phenix_graph::{
        ComponentDef, Composer, Edit, GraphOrigin, NetworkPatch, PortPath, Registration,
    };

    struct Sum {
        bias: i64,
    }
    impl Process for Sum {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            let names: Vec<_> = context.inputs().map(|(name, _)| name.to_owned()).collect();
            if names.is_empty() {
                return Ok(Step::Complete);
            }
            let mut sum = self.bias;
            for name in names {
                sum += context.take(&name).unwrap().get::<i64>()?;
            }
            Ok(Step::Emit(ProcessEmission::new("out", Value::of(sum))))
        }
    }

    let capacity = NonZeroUsize::new(1).unwrap();
    let mut random = 0x91a7_2463_u64;
    for case in 0..32 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let count = 2 + (random as usize % 7);
        let mut parents = Vec::<Vec<usize>>::new();
        let mut biases = Vec::new();
        let mut registrations = Vec::new();
        let mut options = SessionOptions::default();
        for index in 0..count {
            let mut incoming = Vec::new();
            for predecessor in 0..index {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                if predecessor + 1 == index || random & 3 == 0 {
                    incoming.push(predecessor);
                }
            }
            let bias = (random % 13) as i64 - 6;
            let path = format!("p{index}");
            let input_names: Vec<_> = if index == 0 {
                vec!["external".to_owned()]
            } else {
                incoming
                    .iter()
                    .map(|parent| format!("from{parent}"))
                    .collect()
            };
            let interface = Arc::new(
                Interface::new(
                    input_names
                        .iter()
                        .map(|name| PortDef::wire::<i64>(name).unwrap()),
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            );
            let definition = Arc::new(ComponentDef::primitive(
                interface,
                Arc::new(move || Sum { bias }),
            ));
            let mut edits = vec![
                Edit::Add {
                    path: path.clone(),
                    definition,
                },
                Edit::ExposeOutput {
                    name: path.clone(),
                    from: PortPath {
                        process: path.clone(),
                        port: "out".into(),
                    },
                },
            ];
            if index == 0 {
                edits.push(Edit::ExposeInput {
                    name: "input".into(),
                    targets: vec![PortPath {
                        process: path.clone(),
                        port: "external".into(),
                    }],
                    capacity,
                });
            } else {
                for parent in &incoming {
                    edits.push(Edit::Connect(ConnectionDef {
                        from: PortPath {
                            process: format!("p{parent}"),
                            port: "out".into(),
                        },
                        to: PortPath {
                            process: path.clone(),
                            port: format!("from{parent}"),
                        },
                        capacity,
                    }));
                }
            }
            let patch = NetworkPatch::new(edits);
            registrations.push(Registration::new(
                path.clone(),
                incoming.iter().map(|parent| format!("p{parent}")),
                0,
                GraphOrigin::default(),
                patch.footprints(),
                patch,
            ));
            options.readiness.insert(path, Readiness::All);
            parents.push(incoming);
            biases.push(bias);
        }

        // The reference evaluates integer equations only. It does not use the
        // compiler's flattened topology, scheduling decisions, or captured values.
        for reverse in [false, true] {
            let mut composer = Composer::new(NetworkBuilder::new().build().unwrap()).unwrap();
            let mut ordered = registrations.clone();
            if reverse {
                ordered.reverse();
            }
            composer.register(ordered).unwrap();
            let mut session = composer
                .current()
                .compiled()
                .start_with_options(options.clone());
            for input in [-7_i64, 0, 19] {
                let mut expected = Vec::<i64>::new();
                for index in 0..count {
                    let sum = if index == 0 {
                        input
                    } else {
                        parents[index].iter().map(|parent| expected[*parent]).sum()
                    };
                    expected.push(sum + biases[index]);
                }
                session.inject("input", Value::of(input)).unwrap();
                session.run_until_idle(1000);
                for (index, expected) in expected.iter().enumerate() {
                    let output = session.take_output(&format!("p{index}")).unwrap().unwrap();
                    assert_eq!(
                        output.value().get::<i64>().unwrap(),
                        *expected,
                        "case {case}, reverse {reverse}, node {index}, input {input}"
                    );
                    assert!(session.take_output(&format!("p{index}")).unwrap().is_none());
                }
            }
        }
    }
}

#[test]
fn schema_descriptors_and_interface_errors_are_reclaimable() {
    for revision in 0..128 {
        let schema = Schema {
            name: format!("reload/{revision}"),
            revision,
            structure: "i64".into(),
            codec: "i64-le-v1".into(),
        };
        let expected = schema.identity();
        let actual = i64::schema().identity();
        let weak_expected = Arc::downgrade(&expected);
        let weak_actual = Arc::downgrade(&actual);
        let error = {
            let required = Arc::new(
                Interface::new(
                    [],
                    [PortDef {
                        name: "out".into(),
                        value: TypeIdentity::Wire(expected),
                    }],
                )
                .unwrap(),
            );
            let contract = Contract::new("reload/value", required).unwrap();
            let provided = Interface::new(
                [],
                [PortDef {
                    name: "out".into(),
                    value: TypeIdentity::Wire(actual),
                }],
            )
            .unwrap();
            contract.check_shape(&provided).unwrap_err()
        };
        let clone = error.clone();
        drop(error);
        assert!(weak_expected.upgrade().is_some());
        assert!(weak_actual.upgrade().is_some());
        drop(clone);
        assert!(weak_expected.upgrade().is_none());
        assert!(weak_actual.upgrade().is_none());
    }
}

#[cfg(feature = "derive")]
#[test]
fn derived_identities_are_not_cached_and_local_types_remain_distinct() {
    #[derive(ContractValue)]
    #[contract(schema = "ownership/record")]
    struct Record {
        value: i64,
    }
    #[derive(ContractValue)]
    #[contract(schema = "ownership/record")]
    struct Independent {
        value: i64,
    }
    for _ in 0..128 {
        let value = Value::of(Record { value: 42 });
        let weak = Arc::downgrade(value.schema());
        let independent = Independent::schema().identity();
        assert_eq!(*value.schema(), independent);
        assert!(!Arc::ptr_eq(value.schema(), &independent));
        assert_eq!(value.get::<Independent>().unwrap().value, 42);
        assert_ne!(
            Value::local(Record { value: 42 }).identity(),
            value.identity()
        );
        assert_ne!(
            Value::local(Record { value: 42 }).identity(),
            Value::local(Independent { value: 42 }).identity()
        );
        drop(value);
        assert!(weak.upgrade().is_none());
    }
}

// Build the exact feature-specific artifact, not an arbitrary hashed rlib from
// another concurrent Cargo invocation. The independent crates share no Rust type.
fn rustc_paths(output_dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let target = output_dir.join("target");
    let mut command = std::process::Command::new(env!("CARGO"));
    command
        .args([
            "build",
            "--offline",
            "--locked",
            "--lib",
            "--no-default-features",
            "--manifest-path",
        ])
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target);
    if cfg!(feature = "derive") {
        command.args(["--features", "derive"]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "fixture build: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        target.join("debug/deps"),
        target.join("debug/libphenix_graph.rlib"),
    )
}

#[test]
fn independently_compiled_portable_crates_execute() {
    use std::process::Command;
    let output_dir =
        std::env::temp_dir().join(format!("phenix-native-fixtures-{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).unwrap();
    let (deps, library) = rustc_paths(&output_dir);
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "portable_provider",
        "portable_consumer",
        "portable_execution",
    ] {
        let mut command = Command::new("rustc");
        if cfg!(feature = "derive") {
            command.args(["--cfg", "feature=\"derive\""]);
        }
        command
            .args(["--edition=2024", "--crate-name", name])
            .arg("--extern")
            .arg(format!("phenix_graph={}", library.display()))
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
            .arg("--out-dir")
            .arg(&output_dir)
            .arg(fixtures.join(format!("{name}.rs")));
        if name == "portable_execution" {
            command
                .arg("-L")
                .arg(format!("dependency={}", output_dir.display()));
            for dependency in ["portable_provider", "portable_consumer"] {
                command.arg("--extern").arg(format!(
                    "{dependency}={}",
                    output_dir.join(format!("lib{dependency}.rlib")).display()
                ));
            }
        } else {
            command.args(["--crate-type", "rlib"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new(output_dir.join("portable_execution"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture execution: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(output_dir).unwrap();
}

#[test]
fn typed_connections_and_required_schema_are_compile_checked() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let output_dir =
        std::env::temp_dir().join(format!("phenix-native-compile-{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).unwrap();
    let (deps, library) = rustc_paths(&output_dir);
    for (name, source, expected) in [
        (
            "same",
            "fn connect(b: &mut NetworkBuilder, a: OutputRef<i64>, z: InputRef<i64>) { b.connect(a,z,NonZeroUsize::new(1).unwrap()).unwrap(); }",
            true,
        ),
        (
            "different",
            "fn connect(b: &mut NetworkBuilder, a: OutputRef<i64>, z: InputRef<String>) { b.connect(a,z,NonZeroUsize::new(1).unwrap()).unwrap(); }",
            false,
        ),
        (
            "schema",
            "struct Nominal; impl ContractValue for Nominal { fn encode(&self)->Vec<u8>{vec![]} fn decode(_: &[u8])->Result<Self,String>{Ok(Self)} }",
            false,
        ),
    ] {
        let mut child = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-name",
                name,
                "--emit=metadata",
                "--out-dir",
            ])
            .arg(&output_dir)
            .arg("--extern")
            .arg(format!("phenix_graph={}", library.display()))
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(child.stdin.take().unwrap(), "use phenix_graph::{{NetworkBuilder,OutputRef,InputRef,ContractValue}}; use std::num::NonZeroUsize; {source} fn main() {{}}").unwrap();
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.success(), expected, "{name}: {stderr}");
        if name == "different" {
            assert!(stderr.contains("mismatched types"), "{stderr}");
        }
        if name == "schema" {
            assert!(
                stderr.contains("missing") && stderr.contains("schema"),
                "{stderr}"
            );
        }
    }
    std::fs::remove_dir_all(output_dir).unwrap();
}
