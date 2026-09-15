use std::path::Path;
use std::process::Command;

fn check_fixture(name: &str) {
    let manifest =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/external-consumer/Cargo.toml");
    let status = Command::new(env!("CARGO"))
        .args(["check", "--locked", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", name])
        .status()
        .expect("external fixture cargo check starts");
    assert!(status.success(), "external fixture {name} compiles");
}

#[test]
fn external_consumer_with_canonical_sdk_dependency_compiles() {
    check_fixture("phenix-sdk-external-canonical-fixture");
}

#[test]
fn external_consumer_with_renamed_sdk_dependency_compiles() {
    check_fixture("phenix-sdk-external-renamed-fixture");
}

#[test]
fn derive_reaches_compiler_type_checking() {
    trybuild::TestCases::new().compile_fail("tests/ui/missing_value_codec.rs");
}
