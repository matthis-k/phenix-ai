{
  description = "Phenix graph, storage, plugin, and kernel foundation";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  inputs.phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/5ca9d5ba929c334c7c745804d3575bb42972f5d2";

  outputs =
    {
      self,
      nixpkgs,
      phenix-flake-ci,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      inherit (pkgs) lib;

      rust = phenix-flake-ci.lib.mkCargoWorkspace {
        inherit pkgs;
        src = lib.cleanSource ./.;
        lockFile = ./Cargo.lock;
        noDefaultFeatures = true;
        nixfmtFiles = [
          ./flake.nix
          ./ci/flake-check-config.nix
        ];
        smoke.kernel-smoke = {
          package = "phenix-kernel";
          example = "basic";
        };
      };
      checkConfig = import ./ci/flake-check-config.nix;
    in
    {
      lib.flakeCiWorkflow = phenix-flake-ci.lib.mkFlakeCiWorkflow {
        ci = checkConfig.ci;
        configPath = "ci/flake-check-config.nix";
        libraryRef = phenix-flake-ci.rev;
      };
      packages.${system} = rust.packages;
      checks.${system} = rust.checks // {
        workflow = pkgs.runCommand "phenix-workflow-sync" { } ''
          diff -u ${pkgs.writeText "flake-ci.yml" self.lib.flakeCiWorkflow} ${./.github/workflows/flake-ci.yml}
          touch $out
        '';
      };
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rustfmt
          pkgs.clippy
          pkgs.binutils
          pkgs.lld
        ];
      };
      formatter.${system} = pkgs.nixfmt;
    };
}
