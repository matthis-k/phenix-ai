# Source for the tracked generated GitHub Actions workflow.
# The CI check regenerates this text with the pinned library and rejects drift.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflow {
  ci = import ./flake-backend.nix;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "25da62028d5d72d2e5ae3aca9d1571c425eb0d09";
  name = "Phenix generated Nix CI (4 remote builders)";
  events = {
    pullRequest = false;
    pushBranches = [ "ci/m1-generated-backend-consumer" ];
    workflowDispatch = true;
  };
}
