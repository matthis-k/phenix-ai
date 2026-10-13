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
      # These exact store derivations appeared in multiple independent shards.
      # Share only the observed hotset, not every check dependency: broad
      # prebuilding would serialize the critical path and bloat the cache.
      ".#phenix-basic-contracts"
      ".#phenix-plugin-event-journal"
      ".#phenix-plugin-storage-sqlite"
    ];
    checkHints = {
      "m1-shipping" = { isolate = true; weight = 240; };
      "b4-http" = { weight = 8; };
      "basic-pending-http-product" = { weight = 8; };
      "b5-agent" = { weight = 6; };
    };
  };
  cache = {
    keyPrefix = "phenix-m1";
    keyFiles = [ "flake.lock" "Cargo.lock" "examples/Cargo.lock" ];
  };
}
