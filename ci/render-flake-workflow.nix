# Source for the tracked generated GitHub Actions workflow.
# The CI check regenerates this text with the pinned library and rejects drift.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflow {
  ci = import ./flake-backend.nix;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "1a733ae1daae2cb99989b735b171bc5dedd80d63";
  name = "Phenix generated Nix CI (4 remote builders)";
  events = {
    pullRequest = false;
    pushBranches = [ "ci/m1-generated-backend-consumer" "ci/m1-p1-ci-cutover" "ci/m1-p1-cache-benchmark" "main" ];
    workflowDispatch = true;
  };
}
