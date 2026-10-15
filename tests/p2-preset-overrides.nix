# Consumer-side Nix package overrides. The fixture only uses the public
# derivation's ordinary .override method, not Phenix composition helpers.
{ advanced, historyTestModel }:
let
  replaceModel = provider: builtins.map (binding:
    if binding.caller == "phenix.basic.agent" && binding.slot == "Model"
    then binding // { inherit provider; }
    else binding
  );
in
{
  substituted = advanced.override (old: {
    artifacts = old.artifacts ++ [ "${historyTestModel}" ];
    slots = replaceModel "phenix.model.history-test" old.slots;
  });

  missingProvider = advanced.override (old: {
    slots = replaceModel "phenix.model.nonexistent" old.slots;
  });

  excessiveGrant = advanced.override (old: {
    grants = old.grants ++ [
      {
        caller = "root";
        interface = "phenix.advanced.evidence@1";
        operation = "get";
        resources = [ "phenix.session.foreign" ];
      }
    ];
  });
}
