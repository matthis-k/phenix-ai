# Source for the tracked generated GitHub Actions workflow.
# The CI check regenerates this text with the pinned library and rejects drift.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflow {
  ci = import ./flake-backend.nix;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "a1480620950508215bdda4a5404f046becf65cf8";
  name = "Phenix generated Nix CI (4 remote builders)";
  events = {
    pullRequest = false;
    pushBranches = [ "ci/m1-generated-backend-consumer" "ci/m1-p1-ci-cutover" "ci/m1-p1-cache-benchmark" "main" ];
    workflowDispatch = true;
  };
}
