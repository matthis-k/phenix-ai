{
  description = "Phenix P0 and M0.0 native plugin kernel";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      rust = pkgs.rustPlatform;
      source = pkgs.lib.cleanSource ./.;
      kernel = rust.buildRustPackage {
        pname = "phenix-kernel";
        version = "0.1.0";
        src = source;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-kernel" ];
        doCheck = false;
      };
      plugin = name: rust.buildRustPackage {
        pname = name;
        version = "0.1.0";
        src = source;
        cargoRoot = "examples";
        cargoLock.lockFile = ./examples/Cargo.lock;
        cargoBuildFlags = [ "-p" name ];
        doCheck = false;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/lib${builtins.replaceStrings [ "-" ] [ "_" ] name}.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${kernel}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python - $out <<'PY'
          import json, sys
          from pathlib import Path
          out = Path(sys.argv[1])
          record = dict(format="phenix.package.v1", artifact="lib/plugin.so", descriptor="descriptor.bin", target="x86_64-unknown-linux-gnu", **json.load(open("digests.json")))
          (out / "package.json").write_text(json.dumps(record, sort_keys=True))
          PY
        '';
      };
      provider = plugin "example-provider";
      consumer = plugin "example-consumer";
      p0Fat = provider.overrideAttrs (_: { pname = "example-provider-fat"; CARGO_PROFILE_RELEASE_LTO = "fat"; });
      p0Off = provider.overrideAttrs (_: { pname = "example-provider-no-lto"; CARGO_PROFILE_RELEASE_LTO = "off"; });
      p0Lld = provider.overrideAttrs (_: { pname = "example-provider-lld"; nativeBuildInputs = [ pkgs.lld ]; RUSTFLAGS = "-C link-arg=-fuse-ld=lld -C link-arg=-Wl,--gc-sections"; });
      p0Repeat = provider.overrideAttrs (_: { pname = "example-provider-repeat"; });
      descriptorCheck = pkgs.runCommand "phenix-m0-descriptor" { nativeBuildInputs = [ pkgs.python3 pkgs.binutils ]; } ''
        mkdir -p $out
        python ${source}/tests/descriptor.py ${kernel}/bin/phenix-describe ${provider} ${p0Fat} ${p0Off} ${p0Lld} ${p0Repeat} > $out/evidence.json
      '';
    in {
      packages.${system} = {
        default = kernel;
        phenix-kernel = kernel;
        phenix-describe = kernel;
        example-provider = provider;
        example-consumer = consumer;
        phenix-sdk = pkgs.runCommand "phenix-sdk-0.1.0" {} ''
          mkdir -p $out/crates
          cp -r ${source}/crates/phenix-{contract,sdk,macros} $out/crates/
          cp ${source}/Cargo.{toml,lock} $out/
          cp -r ${source}/include $out/
        '';
        phenix-m0-plugins = pkgs.symlinkJoin { name = "phenix-m0-plugins"; paths = []; postBuild = ''
          ln -s ${provider} $out/provider
          ln -s ${consumer} $out/consumer
        ''; };
      };
      checks.${system}.m0-descriptor = descriptorCheck;
      devShells.${system}.default = pkgs.mkShell { packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.binutils pkgs.lld pkgs.python3 ]; };
    };
}
