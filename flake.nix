{
  description = "Phenix core, conductor, plugins, clients, and supported harness";

  inputs = {
    phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/fa8008e193d3b6d6d52703979bdf63c9eaa1a4da";
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
        ./modules/harness-product.nix
        ./modules/plugin-packaging.nix
        ./modules/package-sets.nix
        ./modules/lua-binding-integration.nix
        ./modules/nvim-client.nix
        ./modules/development.nix
        ./modules/stitch.nix
      ];

      flake.flakeModules.default = import ./modules/flake-module.nix;
    };
}
