{
  description = "Phenix clean kernel, SDK and native fixtures";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  inputs.phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/cd2403a45333a5211775bdf01b3ce45f28e02e5f";
  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      lib = pkgs.lib;
      source = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./crates ./tests ./include ./examples ];
      };
      publicMembers = [ "phenix-contract" "phenix-macros" "phenix-sdk" ];
      manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      packageSource = name:
        let
          dependencyTables = cargo: map (kind: cargo.${kind} or { })
            [ "dependencies" "dev-dependencies" "build-dependencies" ];
          members = map (entry: entry.key) (builtins.genericClosure {
            startSet = [ { key = "crates/${name}"; } ];
            operator = entry:
              let
                cargo = builtins.fromTOML (builtins.readFile (./. + "/${entry.key}/Cargo.toml"));
                tables = dependencyTables cargo
                  ++ lib.concatMap dependencyTables (builtins.attrValues (cargo.target or { }));
              in lib.concatMap (table: lib.concatMap (depName:
                let
                  declared = table.${depName};
                  inherited = builtins.isAttrs declared && (declared.workspace or false);
                  dep = if inherited then manifest.workspace.dependencies.${depName} else declared;
                  base = if inherited then ./. else ./. + "/${entry.key}";
                in lib.optional (builtins.isAttrs dep && dep ? path) {
                  key = lib.path.removePrefix ./. (base + "/${dep.path}");
                }
              ) (builtins.attrNames table)) tables;
          });
          packageManifest = (pkgs.formats.toml { }).generate "Cargo.toml" (manifest // {
            workspace = manifest.workspace // { inherit members; };
          });
          files = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions (
              [ ./Cargo.lock ] ++ map (member: ./. + "/${member}") members
              ++ lib.optional (builtins.elem "crates/phenix-kernel" members)
                ./tests/generation-identity-v2.json
            );
          };
        in pkgs.runCommand "${name}-source" { } ''
          cp -r ${files} "$out"
          chmod u+w "$out"
          cp ${packageManifest} "$out/Cargo.toml"
        '';
      # Cargo resolves every member even for -p; external authors must not need
      # the private kernel or the other clean-workspace members on disk.
      publicManifest = (pkgs.formats.toml { }).generate "Cargo.toml" (manifest // {
        workspace = manifest.workspace // { members = map (n: "crates/${n}") publicMembers; };
      });
      publicFiles = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.lock ./include ./crates/phenix-contract ./crates/phenix-macros ./crates/phenix-sdk
        ];
      };
      sdk = pkgs.runCommand "phenix-public-sdk-source" { } ''
        cp -r ${publicFiles} "$out"
        chmod u+w "$out"
        cp ${publicManifest} "$out/Cargo.toml"
      '';
      exampleSource = pkgs.runCommand "phenix-external-example-source" { } ''
        cp -r ${sdk} "$out"
        chmod u+w "$out"
        cp -r ${./examples} "$out/examples"
      '';
      build = name: pkgs.rustPlatform.buildRustPackage {
        pname = name;
        version = "0.1.0";
        src = packageSource name;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" name ];
        cargoTestFlags = [ "-p" name ];
        doCheck = true;
      };
      binaries = lib.genAttrs [ "phenix-contract" "phenix-artifact" "phenix-kernel" "phenix-basic-contracts" ] build;
      fixtureNames = [ "phenix-plugin-model-fixture" "phenix-plugin-tool-reverse" "phenix-plugin-counter-fixture" "phenix-plugin-session-fixture" ];
      fixtures = lib.genAttrs fixtureNames (name: (build name).overrideAttrs (_: {
        installPhase = ''
          runHook preInstall
          mkdir -p "$out/lib"
          cp target/*/release/lib${builtins.replaceStrings [ "-" ] [ "_" ] name}.so "$out/lib/"
          test -s "$out/lib/lib${builtins.replaceStrings [ "-" ] [ "_" ] name}.so"
          runHook postInstall
        '';
      }));
      session = "${fixtures.phenix-plugin-session-fixture}/lib/libphenix_plugin_session_fixture.so";
      workspace = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-clean-workspace-tests";
        version = "0.1.0";
        src = source;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "--workspace" ];
        cargoTestFlags = [ "--workspace" ];
        nativeBuildInputs = [ pkgs.python3 pkgs.rustfmt pkgs.clippy ];
        preCheck = ''
          test -s ${session}
          export PHENIX_SESSION_FIXTURE_NATIVE=${session}
        '';
        postCheck = ''
          $CC -std=c11 -Wall -Wextra -Werror -Iinclude tests/abi.c -o abi-layout
          ./abi-layout
          python tests/m1-architecture.py
        '';
        postBuild = ''
          $CC -std=c11 -Wall -Wextra -Werror -Iinclude -c tests/m1-state-abi.c -o "$TMPDIR/state.o"
          contracts=(target/x86_64-unknown-linux-gnu/release/deps/libphenix_contract-*.rlib)
          test "''${#contracts[@]}" -eq 1
          rustc --edition=2021 --target x86_64-unknown-linux-gnu -C lto=thin -C opt-level=3 \
            tests/m1-state-abi.rs --extern phenix_contract="''${contracts[0]}" \
            -L dependency=target/x86_64-unknown-linux-gnu/release/deps \
            -L dependency=target/release/deps \
            -C link-arg="$TMPDIR/state.o" -o "$TMPDIR/state-abi"
          "$TMPDIR/state-abi"
        '';
      };
      publicSdkCheck = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-public-sdk-tests";
        version = "0.1.0";
        src = sdk;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "--workspace" ];
        cargoTestFlags = [ "--workspace" ];
        preBuild = ''
          test ! -e crates/phenix-kernel
          test ! -e crates/phenix-artifact
        '';
      };
      examples = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-external-native-examples";
        version = "0.1.0";
        src = exampleSource;
        cargoRoot = "examples";
        buildAndTestSubdir = "examples";
        cargoLock.lockFile = ./examples/Cargo.lock;
        # These two crates emit the same library filename; build them separately.
        cargoBuildFlags = [ "--workspace" "--exclude" "example-revision-a" "--exclude" "example-revision-b" ];
        cargoTestFlags = [ "--workspace" "--exclude" "example-revision-a" "--exclude" "example-revision-b" ];
        installPhase = ''
          mkdir -p "$out/lib" "$out/bin"
          cp target/*/release/lib*.so "$out/lib/"
          for artifact in "$out"/lib/*.so; do test -s "$artifact"; done
          cp target/*/release/example-reactive-contracts "$out/bin/"
        '';
      };
      revisions = lib.genAttrs [ "a" "b" ] (revision: examples.overrideAttrs (_: {
        pname = "example-revision-${revision}";
        cargoBuildFlags = [ "-p" "example-revision-${revision}" ];
        cargoTestFlags = [ "-p" "example-revision-${revision}" ];
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libexample_revision.so "$out/lib/"
          test -s "$out/lib/libexample_revision.so"
        '';
      }));
      individualExample = name: examples.overrideAttrs (_: {
        pname = name;
        cargoBuildFlags = [ "-p" name ];
        cargoTestFlags = [ "-p" name ];
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/lib${builtins.replaceStrings [ "-" ] [ "_" ] name}.so "$out/lib/plugin.so"
          test -s "$out/lib/plugin.so"
        '';
      });
      nativePackage = artifact: pkgs.runCommand "${artifact.pname}-package" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out/lib"
        cp ${artifact}/lib/plugin.so "$out/lib/plugin.so"
        ${binaries.phenix-artifact}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
        python - "$out" <<'PY'
        import json, sys
        from pathlib import Path
        out = Path(sys.argv[1])
        record = dict(format="phenix.native-package.v2", artifact="lib/plugin.so",
                      descriptor="descriptor.bin", target="x86_64-unknown-linux-gnu",
                      **json.load(open("digests.json")))
        (out / "package.json").write_text(json.dumps(record, sort_keys=True))
        PY
        test -s "$out/lib/plugin.so"
        test -s "$out/descriptor.bin"
        test -s "$out/package.json"
      '';
      retention = name:
        let
          base = individualExample name;
          variants = [ base
            (base.overrideAttrs (_: { pname = "${name}-fat"; CARGO_PROFILE_RELEASE_LTO = "fat"; }))
            (base.overrideAttrs (_: { pname = "${name}-off"; CARGO_PROFILE_RELEASE_LTO = "off"; }))
            (base.overrideAttrs (old: {
              pname = "${name}-lld";
              nativeBuildInputs = (old.nativeBuildInputs or []) ++ [ pkgs.lld ];
              RUSTFLAGS = "-C link-arg=-fuse-ld=lld -C link-arg=-Wl,--gc-sections";
            }))
            (base.overrideAttrs (_: { pname = "${name}-repeat"; }))
          ];
        in pkgs.runCommand "${name}-metadata-retention" {
          nativeBuildInputs = [ pkgs.python3 pkgs.binutils ];
        } ''
          mkdir -p "$out"
          export PHENIX_PENDING_TRACE="$TMPDIR/offline-trace"
          python ${./tests/descriptor.py} ${binaries.phenix-artifact}/bin/phenix-describe \
            ${lib.concatMapStringsSep " " (v: "${nativePackage v}") variants} > "$out/evidence.json"
          test ! -e "$PHENIX_PENDING_TRACE"
          python -m json.tool "$out/evidence.json" > /dev/null
        '';
      boundaryChecks = pkgs.runCommand "phenix-contract-boundary-checks" {
        nativeBuildInputs = [ pkgs.python3 pkgs.stdenv.cc pkgs.binutils ];
      } ''
        mkdir -p "$out"
        python ${./tests/m1-wire-negative.py} ${binaries.phenix-contract}/bin/phenix-value-probe \
          ${./tests/m1-contract-vectors.json} > "$out/wire.json"
        python ${./tests/m1-value-sdk-vectors.py} ${binaries.phenix-contract}/bin/phenix-static-schema-probe \
          ${./tests/m1-contract-vectors.json} "$out"
        python ${./tests/m1-metadata-negative.py} ${binaries.phenix-artifact}/bin/phenix-metadata-probe \
          ${binaries.phenix-artifact}/bin/phenix-describe ${examples}/bin/example-reactive-contracts \
          ${./tests/m1-metadata-vectors.json} > "$out/metadata.json"
        python ${./tests/generation-identity.py} ${./tests/generation-identity-v2.json} > "$out/generation.json"
      '';
      nativeSession = workspace.overrideAttrs (_: {
        pname = "phenix-native-session-check";
        cargoBuildFlags = [ "-p" "phenix-kernel" ];
        cargoTestFlags = [ "-p" "phenix-kernel" "selected_native_session_handoff_requires_grants_and_preserves_owner_state" ];
      });
      quality = workspace.overrideAttrs (_: {
        pname = "phenix-clean-quality";
        buildPhase = ''
          cargo fmt --all --check
          cargo fmt --manifest-path examples/Cargo.toml --all --check
          cargo clippy --workspace --all-targets --offline -- -D warnings
        '';
        doCheck = false;
        installPhase = ''mkdir -p "$out"; cp Cargo.toml "$out/checked-workspace.toml"'';
      });
    in {
      packages.${system} = binaries // fixtures // {
        default = binaries.phenix-kernel;
        phenix-sdk = sdk;
        phenix-native-examples = examples;
        example-revision-a = revisions.a;
        example-revision-b = revisions.b;
      };
      checks.${system} = {
        clean-workspace = workspace;
        quality = quality;
        public-sdk = publicSdkCheck;
        native-examples = examples;
        native-session = nativeSession;
        contract-boundaries = boundaryChecks;
        metadata-retention = retention "example-provider";
        pending-metadata-retention = retention "example-pending";
        example-revision-a = revisions.a;
        example-revision-b = revisions.b;
      } // fixtures;
      devShells.${system}.default = pkgs.mkShell {
        packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.python3 pkgs.pkg-config ];
      };
    };
}
