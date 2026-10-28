# Render both verified workflows from the one check-policy source.
let
  flakeCi = import ../.ci-tools/flake-ci/lib/default.nix;
in
flakeCi.mkFlakeCiWorkflows {
  ci = (import ./flake-check-config.nix).ci;
  configPath = "ci/flake-check-config.nix";
  libraryRef = "29292dde595a11d6636c87bc5121c861cb1e1b0f";
  names = {
    trusted = "Phenix generated Nix CI (4 remote builders)";
    pullRequest = "Minimal kernel flake-aware matrix candidate";
  };
}
