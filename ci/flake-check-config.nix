# Thin Phenix AI adapter for the generic phenix-flake-ci check planner.
# The actual check inventory always comes from flake.nix at the checked-out SHA.
let
  ci = import ../.ci-tools/flake-ci/lib/default.nix;
in
ci.mkFlakeCheckConfig {
  system = "x86_64-linux";
  shardCount = 5;
  # One typed policy owns both execution paths; no separate backend YAML.
  ci = {
    trusted = {
      repository = "matthis-k/phenix-ai";
      branches = [ "main" ];
      pullRequestAuthors = [ "matthis-k" ];
      backend = "nix-remote";
      remote = {
        transport = "tailscale";
        credentials = "oidc";
        workers = 4;
        slotsPerWorker = 2;
      };
    };
    pullRequest.backend = "nix-dag";
  };
  strategy = ci.rustCheckStrategy {
    workspace = "rust";
    cargoLock = "Cargo.lock";
    # The complete clean-workspace inventory is declared by flake.nix.
    # Do not prebuild deleted P1 products or retain their scheduling hints.
    prebuildTargets = [];
  };
  cache = {
    keyPrefix = "phenix-clean";
    keyFiles = [ "flake.lock" "Cargo.lock" "examples/Cargo.lock" ];
  };
}
