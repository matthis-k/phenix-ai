{
  description = "Phenix P0 and M0.0 native plugin kernel";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      rust = pkgs.rustPlatform;
      source = pkgs.lib.cleanSource ./.;
      subset =
        paths:
        pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions paths;
        };
      publicPaths = [
        ./Cargo.toml
        ./Cargo.lock
        ./crates/phenix-contract
        ./crates/phenix-sdk
        ./crates/phenix-macros
        ./include
      ];
      publicSource = subset publicPaths;
      sdk = pkgs.runCommand "phenix-sdk-0.1.0" { } "cp -r ${publicSource} $out";
      exampleSource = subset [ ./examples ];
      pluginSource = pkgs.runCommand "phenix-external-plugin-source" { } ''
        cp -r ${sdk} $out
        chmod u+w $out
        cp -r ${exampleSource}/examples $out/examples
      '';
      contractSource = subset [
        ./Cargo.toml
        ./Cargo.lock
        ./crates
        ./examples
        ./include
        ./tests/abi.c
      ];
      artifactSource = subset [
        ./Cargo.toml
        ./Cargo.lock
        ./crates/phenix-contract
        ./crates/phenix-artifact
      ];
      kernelSource = subset [
        ./Cargo.toml
        ./Cargo.lock
        ./crates/phenix-contract
        ./crates/phenix-artifact
        ./crates/phenix-kernel
      ];
      build =
        name: src:
        rust.buildRustPackage {
          pname = name;
          version = "0.1.0";
          inherit src;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            name
          ];
          preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
          doCheck = false;
        };
      conformanceSource = subset [
        ./Cargo.toml
        ./Cargo.lock
        ./crates/phenix-contract
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-conformance
      ];
      describe = build "phenix-artifact" artifactSource;
      kernel = build "phenix-kernel" kernelSource;
      conformanceBinary = build "phenix-conformance" conformanceSource;
      recordWriter = pkgs.writeText "phenix-package-record.py" ''
        import json, sys
        from pathlib import Path
        out = Path(sys.argv[1])
        record = dict(format="phenix.package.v1", artifact="lib/plugin.so", descriptor="descriptor.bin", target="x86_64-unknown-linux-gnu", **json.load(open("digests.json")))
        (out / "package.json").write_text(json.dumps(record, sort_keys=True))
      '';
      plugin =
        name:
        rust.buildRustPackage {
          pname = name;
          version = "0.1.0";
          src = pluginSource;
          buildAndTestSubdir = "examples";
          cargoRoot = "examples";
          cargoLock.lockFile = ./examples/Cargo.lock;
          cargoBuildFlags = [
            "-p"
            name
          ];
          preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
          doCheck = false;
          installPhase = ''
            mkdir -p $out/lib
            cp target/*/release/lib${builtins.replaceStrings [ "-" ] [ "_" ] name}.so $out/lib/plugin.so
          '';
          postFixup = ''
            ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
            ${pkgs.python3}/bin/python ${recordWriter} $out
          '';
        };
      provider = plugin "example-provider";
      consumer = plugin "example-consumer";
      alternate = plugin "example-alternate";
      cycle = plugin "example-cycle";
      badAbi =
        case:
        pkgs.runCommand "phenix-bad-abi-${toString case}"
          {
            nativeBuildInputs = [
              pkgs.stdenv.cc
              pkgs.binutils
              pkgs.python3
            ];
          }
          ''
            mkdir -p $out/lib
            $CC -shared -fPIC -DPHENIX_BADCASE=${toString case} -I${./include} ${./tests/abi-negative.c} -o bad.so
            cp ${provider}/lib/plugin.so provider-copy.so
            chmod u+w provider-copy.so
            objcopy --dump-section .phenix=descriptor.section provider-copy.so
            objcopy --add-section .phenix=descriptor.section bad.so $out/lib/plugin.so
            ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
            python ${recordWriter} $out
          '';
      p0Fat = provider.overrideAttrs (_: {
        pname = "example-provider-fat";
        CARGO_PROFILE_RELEASE_LTO = "fat";
      });
      p0Off = provider.overrideAttrs (_: {
        pname = "example-provider-no-lto";
        CARGO_PROFILE_RELEASE_LTO = "off";
      });
      p0Lld = provider.overrideAttrs (old: {
        pname = "example-provider-lld";
        nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.lld ];
        RUSTFLAGS = "-C link-arg=-fuse-ld=lld -C link-arg=-Wl,--gc-sections";
      });
      p0Repeat = provider.overrideAttrs (_: {
        pname = "example-provider-repeat";
      });
      descriptorCheck =
        pkgs.runCommand "phenix-m0-descriptor"
          {
            nativeBuildInputs = [
              pkgs.python3
              pkgs.binutils
            ];
          }
          ''
            mkdir -p $out
            python ${./tests/descriptor.py} ${describe}/bin/phenix-describe ${provider} ${p0Fat} ${p0Off} ${p0Lld} ${p0Repeat} > $out/evidence.json
          '';
      contractCheck = (build "phenix-contract-check" contractSource).overrideAttrs (old: {
        cargoBuildFlags = [ ];
        nativeBuildInputs = old.nativeBuildInputs ++ [
          pkgs.rustfmt
          pkgs.clippy
        ];
        buildPhase = ''
          export CARGO_HOME=$TMPDIR/cargo-home
          cargo fmt --all --check
          cargo fmt --manifest-path examples/Cargo.toml --all --check
          cargo clippy --workspace --all-targets --offline -- -D warnings
          cargo test --workspace --offline
          $CC -std=c11 -Iinclude tests/abi.c -o abi-layout
          ./abi-layout
        '';
        installPhase = "mkdir -p $out; echo passed > $out/contract";
      });
      deployment = pkgs.writeText "phenix-m0-deployment.json" (
        builtins.toJSON {
          format = "phenix.deployment.v1";
          artifacts = [
            "${provider}"
            "${consumer}"
          ];
          entry = {
            interface = "example.consumer@1";
            operation = "run";
            provider = "example.consumer";
          };
          bindings = [
            {
              interface = "example.transform@1";
              operation = "apply";
              provider = "example.provider";
            }
          ];
          grants = [
            {
              caller = "root";
              interface = "example.consumer@1";
              operation = "run";
            }
            {
              caller = "example.consumer";
              interface = "example.transform@1";
              operation = "apply";
            }
          ];
        }
      );
      request = pkgs.runCommand "phenix-m0-request" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python - "$out" <<'PY'
        import sys
        # Canonical value {"text": "phenix"} from the normative value profile.
        open(sys.argv[1], "wb").write(bytes([8])+ (1).to_bytes(4,"big")+(4).to_bytes(4,"big")+b"text"+bytes([5])+(6).to_bytes(4,"big")+b"phenix")
        PY
      '';
      nativeCheck = pkgs.runCommand "phenix-m0-native" { } ''
        ${conformanceBinary}/bin/phenix-conformance ${provider} ${consumer} ${alternate} ${cycle} ${badAbi 1} ${badAbi 2} ${badAbi 3} $out
      '';
      productCheck = pkgs.runCommand "phenix-m0-product" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/product.py} ${kernel}/bin/phenix-kernel ${deployment} ${request} $out
      '';
      conformance = pkgs.runCommand "phenix-m0-conformance" { } ''
        mkdir -p $out
        ln -s ${descriptorCheck} $out/descriptor
        ln -s ${contractCheck} $out/contract
        ln -s ${nativeCheck} $out/native
        ln -s ${productCheck} $out/product
      '';
    in
    {
      packages.${system} = {
        default = kernel;
        phenix-kernel = kernel;
        phenix-describe = describe;
        example-provider = provider;
        example-consumer = consumer;
        phenix-m0-conformance = conformance;
        phenix-m0-deployment = deployment;
        phenix-sdk = sdk;
        phenix-m0-plugins = pkgs.runCommand "phenix-m0-plugins" { } ''
          mkdir -p $out
          ln -s ${provider} $out/provider
          ln -s ${consumer} $out/consumer
        '';
        phenix-m0-demo = pkgs.writeShellApplication {
          name = "phenix-m0-demo";
          text = "exec ${kernel}/bin/phenix-kernel invoke ${deployment} ${request}";
        };
      };
      checks.${system} = {
        m0-descriptor = descriptorCheck;
        m0-contract = contractCheck;
        m0-native = nativeCheck;
        m0-product = productCheck;
      };
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rustfmt
          pkgs.clippy
          pkgs.binutils
          pkgs.lld
          pkgs.python3
        ];
      };
      formatter.${system} = pkgs.nixfmt;
    };
}
