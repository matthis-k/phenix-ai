# Source for the tracked generated GitHub Actions workflow.
# The CI check regenerates this text with the pinned library and rejects drift.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflow {
  ci = import ./flake-backend.nix;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "5366cd3b0f79d83049b95fdb278220394f6866a9";
  name = "Phenix generated Nix CI (4 remote builders)";
  events = {
    pullRequest = false;
    pushBranches = [ "main" ];
    workflowDispatch = true;
  };
}
