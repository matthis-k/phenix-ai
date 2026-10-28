# Render both verified workflows from the one check-policy source.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflows {
  ci = (import ./flake-check-config.nix).ci;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "557deb346279aa92ce5c75550e2050da0761a61c";
  names = {
    trusted = "Phenix generated Nix CI (4 remote builders)";
    pullRequest = "Minimal kernel flake-aware matrix candidate";
  };
}
