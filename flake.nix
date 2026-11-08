{
  description = "Phenix graph, storage, plugin, and kernel foundation";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";

  outputs =
    { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      source = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./packages
        ];
      };
      foundation = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-foundation";
        version = "0.1.0";
        src = source;
        cargoLock.lockFile = ./Cargo.lock;
        cargoTestFlags = [
          "--workspace"
          "--all-targets"
        ];
        nativeBuildInputs = [
          pkgs.rustfmt
          pkgs.clippy
        ];
        preCheck = ''
          cargo fmt --all --check
          cargo clippy --locked --offline --workspace --all-targets -- -D warnings
        '';
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_graph.rlib "$out/lib/"
          cp target/*/release/libphenix_plugin.rlib "$out/lib/"
          cp target/*/release/libphenix_storage.rlib "$out/lib/"
          cp target/*/release/libphenix_kernel.rlib "$out/lib/"
        '';
      };
    in
    {
      packages.${system}.default = foundation;
      checks.${system}.foundation = foundation;
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
