use phenix_graph::{
    AllSameCohort, ContractValue, Deployment, GraphBuilder, InputSlot, InvocationCtx, NodeTemplate,
    OutputSlot, Port, ReportRetention, Schema, Store, Value, WaveError, emit,
};

fn descriptor(optional: bool) -> Schema {
    Schema::record("portable/number", &[("number", optional, i64::schema())])
}

struct Sender(i64);
impl Clone for Sender {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}
impl ContractValue for Sender {
    fn schema() -> Schema {
        descriptor(false)
    }
    fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        i64::decode(bytes).map(Self)
    }
}

// Each variant changes exactly one descriptor component, not the Rust codec.
struct Receiver<const VARIANT: u8>(i64);
impl<const V: u8> Clone for Receiver<V> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}
impl<const V: u8> ContractValue for Receiver<V> {
    fn schema() -> Schema {
        let mut schema = descriptor(V == 5);
        match V {
            1 => schema.name = "portable/other".into(),
            2 => schema.revision += 1,
            3 => schema.structure.push_str("extra-field"),
            4 => schema.codec = "other-codec".into(),
            _ => {}
        }
        schema
    }
    fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        i64::decode(bytes)
            .map(Self)
            .map_err(|_| "receiver requires exactly eight bytes".into())
    }
}

struct Incomplete;
impl ContractValue for Incomplete {
    fn schema() -> Schema {
        Schema {
            structure: String::new(),
            ..Sender::schema()
        }
    }
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
    fn decode(_: &[u8]) -> Result<Self, String> {
        Ok(Self)
    }
}

struct OtherIncomplete;
impl ContractValue for OtherIncomplete {
    fn schema() -> Schema {
        Incomplete::schema()
    }
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
    fn decode(_: &[u8]) -> Result<Self, String> {
        Ok(Self)
    }
}

struct Inconsistent;
impl ContractValue for Inconsistent {
    fn schema() -> Schema {
        descriptor(false)
    }
    fn wire_schema() -> std::sync::Arc<str> {
        "nominal-override".into()
    }
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
    fn decode(_: &[u8]) -> Result<Self, String> {
        Ok(Self)
    }
}

struct EncodedLength(usize);
impl ContractValue for EncodedLength {
    fn schema() -> Schema {
        Sender::schema()
    }
    fn encode(&self) -> Vec<u8> {
        vec![0; self.0]
    }
    fn decode(_: &[u8]) -> Result<Self, String> {
        Err("encode-only fixture".into())
    }
}

fn relay<T: ContractValue>(id: u16) -> NodeTemplate {
    NodeTemplate::with_action(
        id,
        vec![
            Port::new(
                0,
                vec![InputSlot::deferred::<T>(0).named("value")],
                AllSameCohort,
            )
            .named("run")
            .args::<T, _>(|values| values.get("value")),
        ],
        OutputSlot::typed::<T>(0).named("value"),
        |mut ctx: InvocationCtx| Ok(vec![emit(0, ctx.take_prepared::<T>()?)]),
    )
}

fn pair<T: ContractValue, U: ContractValue>() -> GraphBuilder {
    let mut graph = GraphBuilder::new("portable");
    graph.add("source", relay::<T>(1)).unwrap();
    graph.add("sink", relay::<U>(2)).unwrap();
    graph
}

fn connect<T: ContractValue, U: ContractValue>(graph: &mut GraphBuilder) -> Result<(), String> {
    let source = graph
        .discover("source")
        .unwrap()
        .output::<T, phenix_graph::EncodedValue>()?;
    let sink = graph
        .discover("sink")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), U>::new("run"))
        .field(phenix_graph::SlotField::<U, U>::new("value"))
        .target()?;
    graph.connect(source, sink).map(|_| ())
}

fn engine(mut graph: GraphBuilder) -> phenix_graph::Engine {
    let request = graph.expose_input("request", "source").unwrap();
    graph.trigger("run", [request]).unwrap();
    graph.expose_output("result", "sink").unwrap();
    Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
}

#[test]
fn unrelated_manual_types_execute_and_roundtrip_vectors() {
    assert_eq!(Sender::schema(), Receiver::<0>::schema());
    let mut graph = pair::<Sender, Receiver<0>>();
    connect::<Sender, Receiver<0>>(&mut graph).unwrap();
    // Traverse the boundary in both directions, using each receiver's decoder.
    graph.add("return", relay::<Sender>(3)).unwrap();
    let from = graph
        .discover("sink")
        .unwrap()
        .output::<Receiver<0>, phenix_graph::EncodedValue>()
        .unwrap();
    let to = graph
        .discover("return")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), Sender>::new("run"))
        .field(phenix_graph::SlotField::<Sender, Sender>::new("value"))
        .target()
        .unwrap();
    graph.connect(from, to).unwrap();
    let engine = engine(graph);
    for number in [i64::MIN, -1, 0, 1, 42, i64::MAX] {
        let bytes = number.to_le_bytes().to_vec();
        assert_eq!(Sender::decode(&bytes).unwrap().encode(), bytes);
        assert_eq!(Receiver::<0>::decode(&bytes).unwrap().encode(), bytes);
        let report = engine.fire("run", vec![Value::of(Sender(number))]).unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        let values: Vec<_> = report
            .publications()
            .map(|p| p.value::<Sender>().unwrap().0)
            .collect();
        assert_eq!(values, vec![number, number, number]);
    }
}

#[test]
fn malformed_receiver_decode_is_a_typed_atomic_wave_failure() {
    let mut graph = GraphBuilder::new("malformed");
    // A deliberately broken encoder puts malformed bytes on the actual edge.
    graph
        .add(
            "source",
            NodeTemplate::with_action(
                1,
                vec![
                    Port::new(
                        0,
                        vec![InputSlot::deferred::<i64>(0).named("value")],
                        AllSameCohort,
                    )
                    .named("run"),
                ],
                OutputSlot::typed::<Sender>(0).named("value"),
                |ctx: InvocationCtx| Ok(vec![emit(0, EncodedLength(ctx.arg::<i64>(0)? as usize))]),
            ),
        )
        .unwrap();
    graph.add("sink", relay::<Receiver<0>>(2)).unwrap();
    connect::<Sender, Receiver<0>>(&mut graph).unwrap();
    let engine = engine(graph);
    for length in [0, 1, 7, 9, 16] {
        let report = engine.fire("run", vec![Value::of(length as i64)]).unwrap();
        assert!(!report.success());
        assert_eq!(report.invocation_count(1), 1);
        assert_eq!(report.invocation_count(2), 0);
        assert!(
            matches!(report.errors(), [WaveError::InvalidArgs(_, _, _, reason)]
            if reason == "receiver requires exactly eight bytes"),
            "{:?}",
            report.errors()
        );
        assert_eq!(report.publications().count(), 1, "only source may publish");
    }
    let report = engine.fire("run", vec![Value::of(8_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().count(), 2);
}

fn mismatch<const V: u8>() {
    let mut graph = pair::<Sender, Receiver<V>>();
    let error = connect::<Sender, Receiver<V>>(&mut graph).unwrap_err();
    assert!(error.contains("schema mismatch"), "{error}");
    graph.compile().unwrap();
}

#[test]
fn schema_name_mismatch() {
    mismatch::<1>();
}
#[test]
fn schema_revision_mismatch() {
    mismatch::<2>();
}
#[test]
fn schema_structure_mismatch() {
    mismatch::<3>();
}
#[test]
fn schema_codec_mismatch() {
    mismatch::<4>();
}
#[test]
fn schema_optionality_mismatch() {
    mismatch::<5>();
}

#[test]
fn incomplete_equal_descriptors_do_not_certify_portability() {
    let mut graph = pair::<Incomplete, Incomplete>();
    assert!(
        connect::<Incomplete, Incomplete>(&mut graph)
            .unwrap_err()
            .contains("schema mismatch or non-portable descriptor")
    );
    let mut graph = pair::<Incomplete, OtherIncomplete>();
    assert!(connect::<Incomplete, OtherIncomplete>(&mut graph).is_err());
    let mut graph = pair::<Sender, Incomplete>();
    assert!(
        connect::<Sender, Incomplete>(&mut graph)
            .unwrap_err()
            .contains("schema mismatch or non-portable descriptor")
    );
    let mut graph = pair::<Incomplete, Sender>();
    assert!(
        connect::<Incomplete, Sender>(&mut graph)
            .unwrap_err()
            .contains("schema mismatch or non-portable descriptor")
    );
}

#[test]
fn overridden_wire_schema_must_agree_with_descriptor_at_both_ends() {
    let mut graph = pair::<Inconsistent, Receiver<0>>();
    assert!(
        connect::<Inconsistent, Receiver<0>>(&mut graph)
            .unwrap_err()
            .contains("canonical wire endpoints")
    );
    let mut graph = pair::<Sender, Inconsistent>();
    assert!(
        connect::<Sender, Inconsistent>(&mut graph)
            .unwrap_err()
            .contains("canonical wire endpoints")
    );
    let mut graph = pair::<Inconsistent, Inconsistent>();
    assert!(connect::<Inconsistent, Inconsistent>(&mut graph).is_err());
}

#[test]
fn ordinary_same_type_connect_still_executes() {
    let mut graph = pair::<Sender, Sender>();
    let source = graph
        .discover("source")
        .unwrap()
        .output::<Sender, phenix_graph::EncodedValue>()
        .unwrap();
    let sink = graph
        .discover("sink")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), Sender>::new("run"))
        .field(phenix_graph::SlotField::<Sender, Sender>::new("value"))
        .target()
        .unwrap();
    graph.connect(source, sink).unwrap();
    let report = engine(graph)
        .fire("run", vec![Value::of(Sender(123))])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .map(|p| p.value::<Sender>().unwrap().0)
            .collect::<Vec<_>>(),
        vec![123, 123]
    );
}

#[test]
fn encoded_connections_preserve_duplicate_scope_and_disconnect_checks() {
    let mut graph = pair::<Sender, Receiver<0>>();
    let source = graph
        .discover("source")
        .unwrap()
        .output::<Sender, phenix_graph::EncodedValue>()
        .unwrap();
    let sink = graph
        .discover("sink")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), Receiver<0>>::new("run"))
        .field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new(
            "value",
        ))
        .target()
        .unwrap();
    graph.connect(source.clone(), sink.clone()).unwrap();
    assert_eq!(
        graph.connect(source.clone(), sink.clone()).err().unwrap(),
        "duplicate connection"
    );
    let mut foreign = pair::<Sender, Receiver<0>>();
    assert!(foreign.connect(source.clone(), sink.clone()).is_err());
    assert!(foreign.disconnect(source.clone(), sink.clone()).is_err());
    graph.disconnect(source.clone(), sink.clone()).unwrap();
    assert_eq!(
        graph
            .disconnect(source.clone(), sink.clone())
            .err()
            .unwrap(),
        "connection not found"
    );
    graph.connect(source, sink).unwrap();
    let report = engine(graph)
        .fire("run", vec![Value::of(Sender(7))])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
}

#[test]
fn stale_and_divergent_clone_endpoints_are_rejected() {
    for replace_source in [false, true] {
        let original = pair::<Sender, Receiver<0>>();
        let source = original
            .discover("source")
            .unwrap()
            .output::<Sender, phenix_graph::EncodedValue>()
            .unwrap();
        let sink = original
            .discover("sink")
            .unwrap()
            .inlets::<(), phenix_graph::EncodedValue>()
            .port(phenix_graph::InletPort::<(), Receiver<0>>::new("run"))
            .field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new(
                "value",
            ))
            .target()
            .unwrap();
        let mut branch = original.clone();
        branch.connect(source.clone(), sink.clone()).unwrap();
        if replace_source {
            branch.replace("source", relay::<Sender>(1)).unwrap();
        } else {
            branch.replace("sink", relay::<Receiver<0>>(2)).unwrap();
        }
        assert!(branch.connect(source.clone(), sink.clone()).is_err());
        assert!(branch.disconnect(source, sink).is_err());
    }
    let base = GraphBuilder::new("siblings");
    let mut left = base.clone();
    let mut right = base.clone();
    for graph in [&mut left, &mut right] {
        graph.add("source", relay::<Sender>(1)).unwrap();
        graph.add("sink", relay::<Receiver<0>>(2)).unwrap();
    }
    let left_source = left
        .discover("source")
        .unwrap()
        .output::<Sender, phenix_graph::EncodedValue>()
        .unwrap();
    let right_sink = right
        .discover("sink")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), Receiver<0>>::new("run"))
        .field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new(
            "value",
        ))
        .target()
        .unwrap();
    assert!(
        left.connect(left_source.clone(), right_sink.clone())
            .is_err()
    );
    assert!(right.connect(left_source, right_sink).is_err());
}

fn rustc_paths(output_dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    // Let Cargo supply the exact feature-specific artifact, rather than guessing
    // among hashed rlibs left by other builds in the integration test's deps dir.
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
        "fixture library build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let profile = target.join("debug");
    (profile.join("deps"), profile.join("libphenix_graph.rlib"))
}

#[test]
fn ordinary_connect_compile_checks_unrelated_endpoint_types() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let output_dir =
        std::env::temp_dir().join(format!("phenix-portable-wire-{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).unwrap();
    let (deps, library) = rustc_paths(&output_dir);
    let test_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/portable_wire.rs");
    for (case, endpoints, valid) in [
        (
            "same-wire",
            r#"let mut g = pair::<Sender, Sender>(); let a = g.discover("source").unwrap().output::<Sender, phenix_graph::EncodedValue>().unwrap(); let b = g.discover("sink").unwrap().inlets::<(), phenix_graph::EncodedValue>().port(phenix_graph::InletPort::<(), Sender>::new("run")).field(phenix_graph::SlotField::<Sender, Sender>::new("value")).target().unwrap();"#,
            true,
        ),
        (
            "unrelated-wire",
            r#"let mut g = pair::<Sender, Receiver<0>>(); let a = g.discover("source").unwrap().output::<Sender, phenix_graph::EncodedValue>().unwrap(); let b = g.discover("sink").unwrap().inlets::<(), phenix_graph::EncodedValue>().port(phenix_graph::InletPort::<(), Receiver<0>>::new("run")).field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new("value")).target().unwrap();"#,
            true,
        ),
        (
            "unrelated-local",
            r#"let mut g = GraphBuilder::new("local"); g.node("a", |x: Sender| x).unwrap(); g.node("b", |x: Receiver<0>| x).unwrap(); let a = g.discover("a").unwrap().output::<Sender, phenix_graph::LocalValue>().unwrap(); let b = g.discover("b").unwrap().inlets::<(), phenix_graph::LocalValue>().port(phenix_graph::InletPort::<(), Receiver<0>>::new("run")).field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new("value")).target().unwrap();"#,
            false,
        ),
        (
            "unrelated-local-lookup",
            r#"let mut g = GraphBuilder::new("local"); g.node("a", |x: Sender| x).unwrap(); g.node("b", |x: Receiver<0>| x).unwrap(); let a = g.discover("a").unwrap().output::<Sender, phenix_graph::LocalValue>().unwrap(); let b = g.discover("b").unwrap().inlets::<(), phenix_graph::LocalValue>().port(phenix_graph::InletPort::<(), Receiver<0>>::new("run")).field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new("value")).target().unwrap();"#,
            false,
        ),
        (
            "same-local",
            r#"let mut g = GraphBuilder::new("local"); g.node("a", |x: Sender| x).unwrap(); g.node("b", |x: Sender| x).unwrap(); let a = g.discover("a").unwrap().output::<Sender, phenix_graph::LocalValue>().unwrap(); let b = g.discover("b").unwrap().inlets::<(), phenix_graph::LocalValue>().port(phenix_graph::InletPort::<(), Sender>::new("run")).field(phenix_graph::SlotField::<Sender, Sender>::new("value")).target().unwrap();"#,
            true,
        ),
        (
            "same-type-local-to-wire",
            r#"let mut g = pair::<Sender, Sender>(); g.node("local", |x: Sender| x).unwrap(); let a = g.discover("local").unwrap().output::<Sender, phenix_graph::LocalValue>().unwrap(); let b = g.discover("sink").unwrap().inlets::<(), phenix_graph::EncodedValue>().port(phenix_graph::InletPort::<(), Sender>::new("run")).field(phenix_graph::SlotField::<Sender, Sender>::new("value")).target().unwrap();"#,
            false,
        ),
        (
            "wire-to-local",
            r#"let mut g = pair::<Sender, Sender>(); g.node("local", |x: Sender| x).unwrap(); let a = g.discover("source").unwrap().output::<Sender, phenix_graph::EncodedValue>().unwrap(); let b = g.discover("local").unwrap().inlets::<(), phenix_graph::LocalValue>().port(phenix_graph::InletPort::<(), Sender>::new("run")).field(phenix_graph::SlotField::<Sender, Sender>::new("value")).target().unwrap();"#,
            false,
        ),
        (
            "wire-node-to-local-node",
            r#"let mut g = GraphBuilder::new("mixed"); g.node("wire", phenix_graph::Node::wire(|x: i64| x)).unwrap(); g.node("local", |x: i64| x).unwrap(); let a = g.discover("wire").unwrap().output::<i64, phenix_graph::EncodedValue>().unwrap(); let b = g.discover("local").unwrap().inlets::<(), phenix_graph::LocalValue>().port(phenix_graph::InletPort::<(), i64>::new("run")).field(phenix_graph::SlotField::<i64, i64>::new("value")).target().unwrap();"#,
            false,
        ),
        (
            "local-to-wire",
            r#"let mut g = pair::<Sender, Receiver<0>>(); g.node("local", |x: Sender| x).unwrap(); let a = g.discover("local").unwrap().output::<Sender, phenix_graph::LocalValue>().unwrap(); let b = g.discover("sink").unwrap().inlets::<(), phenix_graph::EncodedValue>().port(phenix_graph::InletPort::<(), Receiver<0>>::new("run")).field(phenix_graph::SlotField::<Receiver<0>, Receiver<0>>::new("value")).target().unwrap();"#,
            false,
        ),
    ] {
        let mut child = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-name",
                "portable_probe",
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
        write!(
            child.stdin.take().unwrap(),
            "include!({:?}); fn main() {{ {endpoints} g.connect(a, b).unwrap(); }}",
            test_file
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.success(), valid, "{case}: {stderr}");
        if !valid {
            assert!(
                stderr.contains("ConnectEndpoints") || stderr.contains("mismatched types"),
                "{case}: {stderr}"
            );
        }
    }
    std::fs::remove_dir_all(output_dir).unwrap();
}

#[test]
fn independently_compiled_portable_crates_execute() {
    use std::process::Command;
    let output_dir =
        std::env::temp_dir().join(format!("phenix-portable-fixtures-{}", std::process::id()));
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
        "fixture execution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(output_dir).unwrap();
}

#[test]
fn manual_codec_cannot_omit_complete_schema() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let output_dir =
        std::env::temp_dir().join(format!("phenix-required-schema-{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).unwrap();
    let (deps, library) = rustc_paths(&output_dir);
    let mut child = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-name",
            "required_schema",
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            br#"
        use phenix_graph::ContractValue;
        struct Nominal;
        impl ContractValue for Nominal {
            fn wire_schema() -> std::sync::Arc<str> { "nominal-only".into() }
            fn encode(&self) -> Vec<u8> { vec![] }
            fn decode(_: &[u8]) -> Result<Self, String> { Ok(Self) }
        }
        fn main() {}
    "#,
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "nominal-only implementation compiled"
    );
    assert!(
        stderr.contains("missing") && stderr.contains("schema"),
        "{stderr}"
    );
    std::fs::remove_dir_all(output_dir).unwrap();
}
