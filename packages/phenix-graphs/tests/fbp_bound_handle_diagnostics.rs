//! Check intended native diagnostics, not just arbitrary compile failure. These
//! fixtures have no derives and run unchanged under isolated no-default features.
#[test]
fn bound_capabilities_enforce_payload_direction_authority_and_privacy() {
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let library = std::fs::read_dir(&deps)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("libphenix_graph-")
                && path.extension().is_some_and(|ext| ext == "rlib")
        })
        .max_by_key(|path| std::fs::metadata(path).unwrap().modified().unwrap())
        .expect("built Graph rlib");
    let directory = std::env::temp_dir().join(format!("opencode/fbp-a5-ui-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let imports = "use phenix_graph::{BoundInput, BoundOutput, CaptureHandle, CompiledNetwork, IngressHandle, InputKey, NetworkSession, OutputKey, SourceHandle};\n";
    let fixtures = [
        (
            "positive",
            "struct MoveOnly; fn positive(c: &CompiledNetwork, s: &mut NetworkSession, i: &IngressHandle<MoveOnly>, capture: &CaptureHandle) { let _ = i.clone(); let _ = s.inject_bound(i, MoveOnly); let _ = s.take_bound_output(capture); let _ = s.begin_bound_request(capture); let _ = c.bind_ingress(\"in\", InputKey::<MoveOnly>::new(\"in\")); }",
            None,
        ),
        (
            "payload",
            "fn wrong(s: &mut NetworkSession, i: &IngressHandle<i64>) { s.inject_bound(i, String::new()); }",
            Some("E0308"),
        ),
        (
            "key_direction",
            "fn wrong(c: &CompiledNetwork) { c.bind_ingress(\"in\", OutputKey::<i64>::new(\"in\")); }",
            Some("E0308"),
        ),
        (
            "capture_key_direction",
            "fn wrong(c: &CompiledNetwork) { c.bind_capture(\"out\", InputKey::<i64>::new(\"out\")); }",
            Some("E0308"),
        ),
        (
            "internal_ingress",
            "fn wrong(s: &mut NetworkSession, i: &BoundInput<i64>) { s.inject_bound(i, 1); }",
            Some("E0308"),
        ),
        (
            "internal_capture",
            "fn wrong(s: &mut NetworkSession, o: &BoundOutput<i64>) { s.take_bound_output(o); }",
            Some("E0308"),
        ),
        (
            "forge_ingress",
            "fn wrong(endpoint: BoundInput<i64>) { let _ = IngressHandle { key: InputKey::<i64>::new(\"in\"), endpoint }; }",
            Some("E0451"),
        ),
        (
            "forge_source",
            "fn wrong(s: SourceHandle) { let SourceHandle { path, .. } = s; }",
            Some("E0451"),
        ),
        (
            "forge_capture",
            "fn wrong(c: CaptureHandle) { let CaptureHandle { path, .. } = c; }",
            Some("E0451"),
        ),
    ];
    for (name, body, expected) in fixtures {
        let source = directory.join(format!("{name}.rs"));
        std::fs::write(
            &source,
            format!("#![allow(dead_code, unused_imports, unused_must_use)]\n{imports}{body}\n"),
        )
        .unwrap();
        let output =
            std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .arg("--edition=2024")
                .arg("--crate-type=lib")
                .arg("--error-format=json")
                .arg("--extern")
                .arg(format!("phenix_graph={}", library.display()))
                .arg("-L")
                .arg(format!("dependency={}", deps.display()))
                .arg("--out-dir")
                .arg(&directory)
                .arg(&source)
                .output()
                .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        if let Some(code) = expected {
            assert!(!output.status.success(), "{name} unexpectedly compiled");
            assert!(
                stderr.contains(&format!("\"code\":\"{code}\"")),
                "{name}: intended {code} missing: {stderr}"
            );
            assert!(
                !stderr.contains("\"code\":\"E0432\"") && !stderr.contains("\"code\":\"E0463\""),
                "{name}: missing imports/dependencies: {stderr}"
            );
        } else {
            assert!(output.status.success(), "positive control: {stderr}");
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}
