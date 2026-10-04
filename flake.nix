{
  description = "Phenix AI core, runtime, plugins, clients, and supported harness";

  inputs = {
    phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/9b860fe900672f4269843200460988063b25ade5";
    phenix-pins = {
      url = "github:matthis-k/phenix-pins";
      inputs.phenix-flake-ci.follows = "phenix-flake-ci";
    };
    nixpkgs.follows = "phenix-pins/nixpkgs";

    phenix-stitch = {
      url = "github:matthis-k/phenix-stitch";
      inputs = {
        flake-parts.follows = "phenix-pins/flake-parts";
        phenix-flake-ci.follows = "phenix-flake-ci";
        phenix-pins.follows = "phenix-pins";
      };
    };
  };

  outputs =
    inputs@{
      self,
      phenix-pins,
      ...
    }:
    phenix-pins.inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      imports = [
        ./modules/rust-artifacts.nix
        ./modules/harness-product.nix
        ./modules/plugin-packaging.nix
        ./modules/package-sets.nix
        ./modules/lua-binding-integration.nix
        ./modules/development.nix
        ./modules/stitch.nix
      ];

      flake.flakeModules.default = import ./modules/flake-module.nix;
    };
}
