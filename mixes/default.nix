# Select exact product or test domain compositions at the mix layer.
# This module does not register plugins, grant authority or alter the kernel.
{ lib }:
let
  # Default presets must not accidentally gain experimental Python plugins or
  # standalone testing providers. Explicit consumer overrides remain possible.
  testOnlyNames = [
    "phenix-python-poc"
    "phenix-python-authored"
    "phenix-python-host-fixture"
    "python-trivial"
    "phenix-plugin-model-history-test"
    "phenix-plugin-provider-loopback-fixture"
  ];
  isTestOnly = artifact:
    builtins.any (name: lib.hasInfix name (toString artifact)) testOnlyNames;
  production = name: spec:
    assert lib.assertMsg
      (builtins.all (artifact: !(isTestOnly artifact)) spec.artifacts)
      "${name} production mix includes a test-only plugin; choose a testing mix instead";
    spec;
  modelHistory = { base, artifact, caller, slot, provider }:
    let
      matches = builtins.filter
        (binding: binding.caller == caller && binding.slot == slot)
        base.slots;
    in
    assert lib.assertMsg (builtins.length matches == 1)
      "history test mix requires exactly one existing selected model slot";
    base // {
      artifacts = base.artifacts ++ [ artifact ];
      slots = map (binding:
        if binding.caller == caller && binding.slot == slot
        then binding // { inherit provider; }
        else binding
      ) base.slots;
    };
  providerLoopback = { base, artifact, provider }:
    base // {
      entry = base.entry // { inherit provider; };
      artifacts = [ artifact ];
    };
in
{
  production = {
    basic = production "Basic";
    advanced = production "Advanced";
  };
  testing = {
    inherit modelHistory providerLoopback;
  };
}
