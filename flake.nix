{
  description = "Phenix minimal typed-service kernel through M0.4";
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
        ./tests/generation-identity-v1.json
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
        ./tests/generation-identity-v1.json
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
        ./tests/generation-identity-v1.json
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
        let
          manifest = builtins.fromTOML (
            builtins.readFile (./examples + "/${pkgs.lib.removePrefix "example-" name}/Cargo.toml")
          );
          libraryName = manifest.lib.name or (builtins.replaceStrings [ "-" ] [ "_" ] name);
        in
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
            cp target/*/release/lib${libraryName}.so $out/lib/plugin.so
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
      choice = plugin "example-choice";
      other = plugin "example-other";
      restricted = plugin "example-scoped-client";
      relay = plugin "example-relay";
      resources = plugin "example-resource-provider";
      pendingPlugin = plugin "example-pending";
      pendingConsumer = plugin "example-pending-consumer";
      pendingImmediate = plugin "example-pending-immediate";
      revisionA = plugin "example-revision-a";
      revisionB = plugin "example-revision-b";
      pendingRepeat = pendingPlugin.overrideAttrs (_: {
        pname = "example-pending-repeat";
      });
      pendingFat = pendingPlugin.overrideAttrs (_: {
        pname = "example-pending-fat";
        CARGO_PROFILE_RELEASE_LTO = "fat";
      });
      pendingOff = pendingPlugin.overrideAttrs (_: {
        pname = "example-pending-off";
        CARGO_PROFILE_RELEASE_LTO = "off";
      });
      pendingLld = pendingPlugin.overrideAttrs (old: {
        pname = "example-pending-lld";
        nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.lld ];
        RUSTFLAGS = "-C link-arg=-fuse-ld=lld -C link-arg=-Wl,--gc-sections";
      });
      lifecycleProvider = plugin "example-lifecycle-provider";
      lifecycleConsumer = plugin "example-lifecycle-consumer";
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
            python -m json.tool $out/evidence.json > /dev/null
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
      referenceWireCheck = (build "phenix-contract" publicSource).overrideAttrs (old: {
        cargoBuildFlags = [ "-p" "phenix-contract" "--bin" "phenix-value-probe" ];
        nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.python3 ];
        installPhase = ''
          mkdir -p $out
          probe=$(find target -type f -name phenix-value-probe -perm -u+x | head -1)
          test -n "$probe"
          python ${./tests/m1-wire-negative.py} "$probe" ${./tests/m1-contract-vectors.json} > $out/evidence.json
          python -m json.tool $out/evidence.json > /dev/null
        '';
      });
      reactiveContracts = rust.buildRustPackage {
        pname = "example-reactive-contracts";
        version = "0.1.0";
        src = pluginSource;
        buildAndTestSubdir = "examples";
        cargoRoot = "examples";
        cargoLock.lockFile = ./examples/Cargo.lock;
        cargoBuildFlags = [ "-p" "example-reactive-contracts" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = false;
        installPhase = ''
          test ! -e crates/phenix-kernel
          test ! -e crates/phenix-artifact
          test ! -e ../crates/phenix-kernel
          test ! -e ../crates/phenix-artifact
          mkdir -p $out/bin
          cp target/*/release/example-reactive-contracts $out/bin/
        '';
      };
      metadataStateCheck = (build "phenix-artifact" artifactSource).overrideAttrs (old: {
        nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.python3 ];
        installPhase = ''
          mkdir -p $out
          probe=$(find target -type f -name phenix-metadata-probe -perm -u+x | head -1)
          describe=$(find target -type f -name phenix-describe -perm -u+x | head -1)
          test -n "$probe"
          test -n "$describe"
          python ${./tests/m1-metadata-negative.py} "$probe" "$describe" ${reactiveContracts}/bin/example-reactive-contracts ${./tests/m1-metadata-vectors.json} > $out/metadata.json
          contract=$(find target -path '*/release/deps/libphenix_contract-*.rlib' | head -1)
          test -n "$contract"
          $CC -std=c11 -Wall -Wextra -Werror -I${./include} -c ${./tests/m1-state-abi.c} -o "$TMPDIR/state.o"
          rustc --edition=2021 -C lto=thin -C opt-level=3 ${./tests/m1-state-abi.rs} --extern phenix_contract="$contract" -L dependency="$(dirname "$contract")" -C link-arg="$TMPDIR/state.o" -o "$TMPDIR/state-abi"
          "$TMPDIR/state-abi" > $out/native.json
          python -m json.tool $out/metadata.json > /dev/null
          python -m json.tool $out/native.json > /dev/null
        '';
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
        ${conformanceBinary}/bin/phenix-conformance ${provider} ${consumer} ${alternate} ${cycle} ${badAbi 1} ${badAbi 2} ${badAbi 3} ${badAbi 4} ${badAbi 5} $out
      '';
      productCheck = pkgs.runCommand "phenix-m0-product" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/product.py} ${kernel}/bin/phenix-kernel ${deployment} ${request} ${./tests/inspection-golden.json} $out
      '';
      compositionCheck = pkgs.runCommand "phenix-m01-composition" { } ''
        ${conformanceBinary}/bin/phenix-m01-conformance ${provider} ${alternate} ${choice} ${other} ${restricted} ${relay} ${resources} ${cycle} $out
      '';
      compositionProduct =
        pkgs.runCommand "phenix-m01-product" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            python ${./tests/composition-product.py} ${kernel}/bin/phenix-kernel ${compositionCheck} ${./tests/composition-golden.json} $out
          '';
      lifecycleCheck = pkgs.runCommand "phenix-m02-lifecycle" { } ''
        ${conformanceBinary}/bin/phenix-m02-lifecycle ${lifecycleProvider} ${lifecycleConsumer} $out
      '';
      lifecycleProduct =
        pkgs.runCommand "phenix-m02-lifecycle-product" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            python ${./tests/lifecycle-product.py} ${kernel}/bin/phenix-kernel ${lifecycleCheck} $out
          '';
      pendingCheck =
        pkgs.runCommand "phenix-m02-pending"
          {
            nativeBuildInputs = [
              pkgs.python3
              pkgs.binutils
            ];
          }
          ''
            mkdir -p $out
            export PHENIX_PENDING_TRACE=$TMPDIR/offline-trace
            python ${./tests/descriptor.py} ${describe}/bin/phenix-describe ${pendingPlugin} ${pendingFat} ${pendingOff} ${pendingLld} ${pendingRepeat} > $out/descriptor-evidence.json
            test ! -e "$PHENIX_PENDING_TRACE"
            ${conformanceBinary}/bin/phenix-m02-pending ${pendingPlugin} ${provider} $out
          '';
      pendingProduct =
        pkgs.runCommand "phenix-m02-pending-product" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            python ${./tests/pending-product.py} ${kernel}/bin/phenix-kernel ${pendingCheck} $out
          '';
      importsCheck = pkgs.runCommand "phenix-m02-imports" { } ''
        ${conformanceBinary}/bin/phenix-m02-imports ${pendingConsumer} ${pendingPlugin} ${provider} ${pendingImmediate} $out
      '';
      generationsCheck =
        pkgs.runCommand "phenix-m03-generations" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            mkdir -p $out
            python ${./tests/generation-identity.py} ${./tests/generation-identity-v1.json} > $out/identity-vector.json
            ${conformanceBinary}/bin/phenix-m03-generations ${revisionA} ${revisionB} ${importsCheck} $out
          '';
      publicSdkCheck = (build "phenix-public-sdk-check" publicSource).overrideAttrs (_: {
        cargoBuildFlags = [ ];
        buildPhase = ''
          export CARGO_HOME=$TMPDIR/cargo-home
          test ! -e crates/phenix-kernel
          test ! -e crates/phenix-artifact
          test ! -e crates/phenix-conformance
          cargo test --workspace --offline
        '';
        installPhase = "mkdir -p $out; echo passed > $out/public-sdk";
      });
      productionClosure = pkgs.closureInfo { rootPaths = [ kernel ]; };
      shippingCheck = pkgs.runCommand "phenix-m04-shipping" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/shipping-product.py} ${kernel} ${publicSdkCheck} ${conformance} ${productionClosure} ${exampleSource}/examples $out
      '';
      release = pkgs.runCommand "phenix-m0-release" { } ''
        mkdir -p $out
        cp -rs ${conformance}/. $out/
        ln -s ${publicSdkCheck} $out/public-sdk
        ln -s ${shippingCheck} $out/shipping
        ln -s ${productionClosure} $out/production-closure
      '';
      conformance = pkgs.runCommand "phenix-m0-conformance" { } ''
        mkdir -p $out
        ln -s ${descriptorCheck} $out/descriptor
        ln -s ${contractCheck} $out/contract
        ln -s ${nativeCheck} $out/native
        ln -s ${productCheck} $out/product
        ln -s ${compositionCheck} $out/composition
        ln -s ${compositionProduct} $out/composition-product
        ln -s ${lifecycleCheck} $out/lifecycle
        ln -s ${lifecycleProduct} $out/lifecycle-product
        ln -s ${pendingCheck} $out/pending
        ln -s ${pendingProduct} $out/pending-product
        ln -s ${importsCheck} $out/imports
        ln -s ${generationsCheck} $out/generations
      '';
    in
    {
      packages.${system} = {
        default = kernel;
        phenix-kernel = kernel;
        phenix-describe = describe;
        example-provider = provider;
        example-consumer = consumer;
        phenix-m0-conformance = release;
        phenix-m0-release = release;
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
        m1-reference-wire = referenceWireCheck;
        m1-metadata-state = metadataStateCheck;
        m0-descriptor = descriptorCheck;
        m0-contract = contractCheck;
        m0-native = nativeCheck;
        m0-product = productCheck;
        m01-composition = compositionCheck;
        m01-product = compositionProduct;
        m04-public-sdk = publicSdkCheck;
        m04-shipping = shippingCheck;
        m03-generations = generationsCheck;
        m02-imports = importsCheck;
        m02-pending = pendingCheck;
        m02-pending-product = pendingProduct;
        m02-lifecycle = lifecycleCheck;
        m02-lifecycle-product = lifecycleProduct;
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
