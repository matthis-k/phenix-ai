# Source for the tracked generated GitHub Actions workflow.
# The CI check regenerates this text with the pinned library and rejects drift.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflow {
  ci = import ./flake-backend.nix;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "e91cc8de2e10d0c4592b950e442f73ff6ea9efa3";
  name = "Phenix generated Nix CI (4 remote builders)";
  events = {
    pullRequest = false;
    pushBranches = [ "main" ];
    workflowDispatch = true;
  };
}
