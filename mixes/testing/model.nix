# A test-only Model override; keep slots, authority and grants otherwise unchanged.
{ base, artifact, caller, slot, provider, extraArtifacts ? [] }:
let
  matches = builtins.filter
    (binding: binding.caller == caller && binding.slot == slot)
    base.slots;
in
assert builtins.length matches == 1;
base // {
  artifacts = base.artifacts ++ [ artifact ] ++ extraArtifacts;
  slots = map (binding:
    if binding.caller == caller && binding.slot == slot
    then binding // { inherit provider; }
    else binding
  ) base.slots;
}
