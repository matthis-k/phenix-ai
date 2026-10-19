# Thin Phenix AI adapter for the generic phenix-flake-ci check planner.
# The actual check inventory always comes from flake.nix at the checked-out SHA.
let
  ci = import ../.ci-tools/flake-ci/lib/default.nix;
in
ci.mkFlakeCheckConfig {
  system = "x86_64-linux";
  shardCount = 5;
  strategy = ci.rustCheckStrategy {
    workspace = "rust";
    cargoLock = "Cargo.lock";
    prebuildTargets = [
      ".#phenix-kernel"
      ".#phenix-describe"
      ".#phenix-m1-conformance-runner"
      ".#phenix-core-example-plugins"
      # Keep only actual shared dependencies in the prebuild hotset.
      # Per-plugin packages are intentionally isolated, not batch-prebuilt.
      # These store derivations appeared in multiple independent shards.
      # Share only the observed hotset, not every check dependency: broad
      # prebuilding would serialize the critical path and bloat the cache.
      ".#phenix-basic-contracts"
      ".#phenix-plugin-event-journal"
      ".#phenix-plugin-storage-sqlite"
      # A reused contract-check derivation was compiled again for ~3 minutes
      # on an unchanged PR head despite the seven-package shared cache.
      # Promote this observed shared Nix *check* to the rooted hotset, without
      # moving test ownership out of flake.nix or skipping any shard checks.
      ".#checks.x86_64-linux.m1-contract"
    ];
    checkHints = {
      "m1-shipping" = { isolate = true; weight = 240; };
      "b4-http" = { weight = 8; affinity = "model-http"; };
      "basic-pending-http-product" = { weight = 8; affinity = "model-http"; };
      "b5-agent" = { weight = 6; affinity = "agent"; };
      "b3-tools" = { affinity = "agent"; };
      "basic-host-smoke" = { affinity = "agent"; };
      "b2-*" = { affinity = "state"; };
      "b3-memory" = { affinity = "state"; };
      "f?-*" = { affinity = "frontend"; };
      "m1-live-state*" = { affinity = "live-state"; };
      "m1-value-sdk*" = { affinity = "value-sdk"; };
    };
  };
  cache = {
    keyPrefix = "phenix-m1";
    keyFiles = [ "flake.lock" "Cargo.lock" "examples/Cargo.lock" ];
  };
}
