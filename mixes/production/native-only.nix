# Enforce that default Rust-native product mixes do not select test artifacts.
{ lib }:
name: spec:
let
  testOnlyNames = [
    "phenix-python-poc"
    "phenix-python-authored"
    "phenix-python-host-fixture"
    "python-trivial"
    "phenix-plugin-model-history-test"
    "phenix-plugin-provider-loopback-fixture"
  ];
  isTestOnly = artifact:
    builtins.any (fragment: lib.hasInfix fragment (toString artifact)) testOnlyNames;
in
assert lib.assertMsg
  (builtins.all (artifact: !(isTestOnly artifact)) spec.artifacts)
  "${name} production mix includes a test-only plugin; select a testing mix instead";
spec
