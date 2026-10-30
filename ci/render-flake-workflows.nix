# Render both verified workflows from the one check-policy source.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflows {
  ci = (import ./flake-check-config.nix).ci;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "e83c6029afc0140a242ae8240dbff6efe5979725";
  names = {
    trusted = "Phenix generated Nix CI (4 remote builders)";
    pullRequest = "Minimal kernel flake-aware matrix candidate";
  };
}
