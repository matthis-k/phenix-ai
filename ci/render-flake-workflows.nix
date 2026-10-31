# Render both verified workflows from the one check-policy source.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflows {
  ci = (import ./flake-check-config.nix).ci;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "5ac9c10ab144a20f32d542f663779b7d642cab49";
  names = {
    trusted = "Phenix generated Nix CI (4 remote builders)";
    pullRequest = "Minimal kernel flake-aware matrix candidate";
  };
}
