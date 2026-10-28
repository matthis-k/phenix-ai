# Render both verified workflows from the one check-policy source.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflows {
  ci = (import ./flake-check-config.nix).ci;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "ada2d1a7b9e641b14cae83a8cacdd8cc63e7256d";
  names = {
    trusted = "Phenix generated Nix CI (4 remote builders)";
    pullRequest = "Minimal kernel flake-aware matrix candidate";
  };
}
