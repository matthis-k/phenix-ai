{
  description = "Phenix M1 typed contract kernel";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  inputs.phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/cd2403a45333a5211775bdf01b3ce45f28e02e5f";
  outputs =
    { self, nixpkgs, phenix-flake-ci }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      testLib = phenix-flake-ci.lib;
      a = testLib.testAssert;
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
        ./tests/m1-architecture.py
        ./tests/generation-identity-v2.json
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
        ./tests/generation-identity-v2.json
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
        ./crates/phenix-sdk
        ./crates/phenix-macros
        ./crates/phenix-basic-contracts
        ./tests/generation-identity-v2.json
      ];
      describe = build "phenix-artifact" artifactSource;
      pluginSourceTool = pkgs.writeShellApplication {
        name = "phenix-plugin-sources";
        runtimeInputs = [ pkgs.python3 pkgs.cargo pkgs.rustc pkgs.stdenv.cc pkgs.git pkgs.nix ];
        text = ''
          exec ${pkgs.python3}/bin/python ${./tools/phenix-plugin-sources.py} \
            --describe ${describe}/bin/phenix-describe "$@"
        '';
      };
      kernel = build "phenix-kernel" kernelSource;
      conformanceBinary = build "phenix-conformance" conformanceSource;
      recordWriter = pkgs.writeText "phenix-package-record.py" ''
        import json, sys
        from pathlib import Path
        out = Path(sys.argv[1])
        profile = "phenix.native-package.v2"
        record = dict(format=profile, artifact="lib/plugin.so", descriptor="descriptor.bin", target="x86_64-unknown-linux-gnu", **json.load(open("digests.json")))
        (out / "package.json").write_text(json.dumps(record, sort_keys=True))
      '';
      # Compatible, default-feature example cdylibs share one Cargo
      # compilation instead of recompiling the workspace in each Nix check
      # shard. Keep feature-variant sources (pending, reactive-provider) on
      # individual Rust derivations so ABI/LTO/security-negative fixtures
      # continue to test the actual distinct binaries. Revision A/B must
      # also stay separate: both Cargo crates emit libexample_revision.so,
      # but their different executable digests are part of generation tests.
      # Packaging stays per-plugin with a descriptor and digest of the final
      # copied shared library, preserving native path-based loading.
      coreExamples = [
        "example-provider"
        "example-consumer"
        "example-alternate"
        "example-cycle"
        "example-choice"
        "example-other"
        "example-scoped-client"
        "example-relay"
        "example-resource-provider"
        "example-lifecycle-provider"
        "example-lifecycle-consumer"
        "example-agent"
        "example-agent-alternate"
        "example-agent-model"
        "example-agent-tool"
        "example-authoring-client"
        "example-authoring-field"
        "example-authoring-provider"
        "example-graph-a"
        "example-graph-b"
        "example-graph-c"
        "example-pending-consumer"
        "example-pending-immediate"
        "example-pending-security"
        "example-reactive-client"
        "example-value-client"
        "example-value-provider"
      ];
      exampleLibraryName =
        name:
        let
          manifest = builtins.fromTOML (
            builtins.readFile (./examples + "/${pkgs.lib.removePrefix "example-" name}/Cargo.toml")
          );
        in
        manifest.lib.name or (builtins.replaceStrings [ "-" ] [ "_" ] name);
      coreExamplesBuild = rust.buildRustPackage {
        pname = "phenix-core-example-plugins";
        version = "0.1.0";
        src = pluginSource;
        buildAndTestSubdir = "examples";
        cargoRoot = "examples";
        cargoLock.lockFile = ./examples/Cargo.lock;
        cargoBuildFlags = pkgs.lib.concatMap (name: [ "-p" name ]) coreExamples;
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = false;
        installPhase = ''
          mkdir -p "$out/lib"
          ${pkgs.lib.concatMapStringsSep "\n" (name: ''
            cp target/*/release/lib${exampleLibraryName name}.so "$out/lib/"
          '') coreExamples}
        '';
      };
      packagedCoreExample = name:
        pkgs.runCommand "${name}-native-package" { } ''
          mkdir -p "$out/lib"
          # runCommand executes buildCommand, not stdenv's postFixup phase.
          # The shared Cargo output was already fixed and stripped, so hash
          # its final copied bytes and produce both metadata files here.
          cp ${coreExamplesBuild}/lib/lib${exampleLibraryName name}.so "$out/lib/plugin.so"
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
          test -s "$out/lib/plugin.so"
          test -s "$out/descriptor.bin"
          test -s "$out/package.json"
        '';
      individualPlugin =
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
      plugin =
        name:
        if builtins.elem name coreExamples then
          packagedCoreExample name
        else
          individualPlugin name;
      provider = plugin "example-provider";
      consumer = plugin "example-consumer";
      alternate = plugin "example-alternate";
      cycle = plugin "example-cycle";
      choice = plugin "example-choice";
      other = plugin "example-other";
      restricted = plugin "example-scoped-client";
      relay = plugin "example-relay";
      resources = plugin "example-resource-provider";
      valueProvider = plugin "example-value-provider";
      valueClient = plugin "example-value-client";
      graphA = plugin "example-graph-a";
      graphB = plugin "example-graph-b";
      graphC = plugin "example-graph-c";
      graphDefinitionsBinary = rust.buildRustPackage {
        pname = "example-graph-contracts";
        version = "0.1.0";
        src = pluginSource;
        buildAndTestSubdir = "examples";
        cargoRoot = "examples";
        cargoLock.lockFile = ./examples/Cargo.lock;
        cargoBuildFlags = [
          "-p"
          "example-graph-contracts"
          "--bin"
          "example-graph-contracts"
        ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = false;
        installPhase = "mkdir -p $out/bin; cp target/*/release/example-graph-contracts $out/bin/";
      };
      definitionPackage = name: emitter: hasGraph:
        pkgs.runCommand name { nativeBuildInputs = [ pkgs.python3 ]; } ''
          mkdir -p $out
          ${emitter} $out/descriptor.bin ${pkgs.lib.optionalString hasGraph "$out/graph.bin"}
          python - "$out" <<'PY'
          import hashlib, json, sys
          from pathlib import Path
          out = Path(sys.argv[1])
          digest = lambda name: hashlib.sha256((out / name).read_bytes()).hexdigest()
          record = dict(format="phenix.contract-package.v2", descriptor="descriptor.bin", descriptor_sha256=digest("descriptor.bin"))
          if (out / "graph.bin").exists():
              record["graph"] = dict(file="graph.bin", sha256=digest("graph.bin"))
          (out / "package.json").write_text(json.dumps(record))
          PY
          ${describe}/bin/phenix-describe --metadata $out > $out/inspection.json
        '';
      graphDefinitions = definitionPackage "phenix-graph-definitions" "${graphDefinitionsBinary}/bin/example-graph-contracts" true;
      basicContractsSource = subset (publicPaths ++ [ ./crates/phenix-basic-contracts ]);
      basicContractsBinary = build "phenix-basic-contracts" basicContractsSource;
      basicDefinitions = definitionPackage "phenix-basic-contracts" "${basicContractsBinary}/bin/phenix-basic-contracts" true;
      basicContractsCheck = pkgs.runCommand "phenix-basic-contracts-check" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p $out
        python - "${basicDefinitions}/inspection.json" "$out" <<'PY'
        import json, sys
        from pathlib import Path
        report = json.load(open(sys.argv[1]))
        assert len(report["definitions"]) == 19
        assert "phenix.basic.tool-registrations@1" in report["definitions"], report
        assert "phenix.basic.turn-advance@1" in report["definitions"], report
        assert "phenix.basic.turn-prepare@1" in report["definitions"], report
        assert report["plugin"] == "phenix.basic-contracts"
        (Path(sys.argv[2]) / "evidence.json").write_text(json.dumps(report, indent=2, sort_keys=True))
        PY
      '';
      environmentContractsSource = subset (publicPaths ++ [ ./crates/phenix-environment-contracts ]);
      environmentContractsBinary = rust.buildRustPackage {
        pname = "phenix-environment-contracts";
        version = "0.1.0";
        src = environmentContractsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-environment-contracts" ];
        cargoTestFlags = [ "-p" "phenix-environment-contracts" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
      };
      environmentDefinitions = definitionPackage "phenix-environment-contracts"
        "${environmentContractsBinary}/bin/phenix-environment-contracts" false;
      environmentPluginSource = subset (publicPaths ++ [
        ./crates/phenix-environment-contracts
        ./crates/phenix-plugin-environment
      ]);
      environmentPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-environment";
        version = "0.1.0";
        src = environmentPluginSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-environment" ];
        cargoTestFlags = [ "-p" "phenix-plugin-environment" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_environment.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      environmentProductCheck = pkgs.runCommand "phenix-environment-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/environment-product.py} ${kernel}/bin/phenix-kernel ${environmentPlugin} ${environmentDefinitions} "$out"
        test -s "$out/evidence.json"
      '';
      workspaceContractsSource = subset (publicPaths ++ [ ./crates/phenix-workspace-contracts ]);
      workspaceContractsBinary = rust.buildRustPackage {
        pname = "phenix-workspace-contracts";
        version = "0.1.0";
        src = workspaceContractsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-workspace-contracts" ];
        cargoTestFlags = [ "-p" "phenix-workspace-contracts" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
      };
      workspaceDefinitions = definitionPackage "phenix-workspace-contracts"
        "${workspaceContractsBinary}/bin/phenix-workspace-contracts" false;
      workspacePluginSource = subset (publicPaths ++ [
        ./crates/phenix-workspace-contracts
        ./crates/phenix-plugin-workspace-selection
        ./crates/phenix-basic-contracts
      ]);
      workspacePlugin = rust.buildRustPackage {
        pname = "phenix-plugin-workspace-selection";
        version = "0.1.0";
        src = workspacePluginSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-workspace-selection" ];
        cargoTestFlags = [ "-p" "phenix-plugin-workspace-selection" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_workspace_selection.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      workspaceProductCheck = pkgs.runCommand "phenix-workspace-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/workspace-product.py} ${kernel}/bin/phenix-kernel ${workspacePlugin} ${storageSqlite} ${workspaceDefinitions} ${basicDefinitions} "$out"
        test -s "$out/evidence.json"
      '';
      # Generic provider host; data-only contributors never implement catalog
      # or authentication contracts.
      providerHostPluginSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-providers
        ./crates/phenix-provider-auth-strategies
        ./crates/phenix-provider-discovery
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerHostPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-providers";
        version = "0.1.0";
        src = providerHostPluginSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-providers" ];
        cargoTestFlags = [ "-p" "phenix-plugin-providers" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_providers.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      providerBuiltinsPluginSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-builtins
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerBuiltinsPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-provider-builtins";
        version = "0.1.0";
        src = providerBuiltinsPluginSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-builtins" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-builtins" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_builtins.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      # Minimal OpenAI declaration-only plugin: one graph on-populate hook.
      providerOpenAiDeclarationSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-openai
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerOpenAiDeclarationPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-provider-openai";
        version = "0.1.0";
        src = providerOpenAiDeclarationSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-openai" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-openai" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_openai.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      # Extra standard providers register through the same generic populate
      # trigger; the OpenAI declaration plugin remains its separate owner.
      providerStandardDeclarationsSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-standards
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerStandardDeclarationsPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-provider-standards";
        version = "0.1.0";
        src = providerStandardDeclarationsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-standards" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-standards" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_standards.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      providerHostApiKeyProductCheck = pkgs.runCommand "phenix-provider-host-api-key-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/provider-host-api-key-product.py} \
          ${kernel}/bin/phenix-kernel ${providerHostPlugin} \
          ${providerOpenAiApiFixture} ${providerCredentialStore} ${providerDefinitions} "$out"
        test -s "$out/verified.json"
      '';
      providerBuiltinsPluginCheck = pkgs.runCommand "phenix-provider-builtins-plugin-check" {
        nativeBuildInputs = [ pkgs.jq pkgs.python3 ];
      } ''
        mkdir -p "$out"
        test -s "${providerBuiltinsPlugin}/descriptor.bin"
        test -s "${providerBuiltinsPlugin}/lib/plugin.so"
        grep -aq 'phenix.provider.builtins' "${providerBuiltinsPlugin}/descriptor.bin"
        test -s "${providerHostPlugin}/descriptor.bin"
        test -s "${providerHostPlugin}/lib/plugin.so"
        grep -aq 'phenix.provider.host' "${providerHostPlugin}/descriptor.bin"
        jq -e '.format == "phenix.native-package.v2" and .artifact == "lib/plugin.so"' \
          "${providerBuiltinsPlugin}/package.json"
        python ${./tests/provider-builtins-product.py} \
          ${kernel}/bin/phenix-kernel ${providerHostPlugin} ${providerBuiltinsPlugin} ${providerDefinitions} "$out" ${providerOpenAiApi} ${providerCredentialStore} ${providerChatGpt}
        touch "$out/verified"
      '';
      providerBuiltinsSource = subset (publicPaths ++ [
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerBuiltinsCheck = rust.buildRustPackage {
        pname = "phenix-provider-builtins";
        version = "0.1.0";
        src = providerBuiltinsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-provider-builtins" ];
        cargoTestFlags = [ "-p" "phenix-provider-builtins" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out
          touch $out/verified
        '';
      };
      # Opt-in provider plugin: real per-profile API-key verification. The
      # default Basic composition and metadata-only provider remain unchanged.
      providerOpenAiApiSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-openai-api
        ./crates/phenix-provider-auth-strategies
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-discovery
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerOpenAiApi = rust.buildRustPackage {
        pname = "phenix-plugin-provider-openai-api";
        version = "0.1.0";
        src = providerOpenAiApiSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-openai-api" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-openai-api" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_openai_api.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      # Provider-owned ChatGPT authorization-code/OIDC plugin. This is
      # dormant unless a separate launcher explicitly composes its exact
      # named-account credential grants and provider auth routes.
      providerChatGptSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-chatgpt
        ./crates/phenix-provider-auth-strategies
        ./crates/phenix-provider-discovery
        ./crates/phenix-provider-builtins
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerChatGpt = rust.buildRustPackage {
        pname = "phenix-plugin-provider-chatgpt";
        version = "0.1.0";
        src = providerChatGptSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-chatgpt" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-chatgpt" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_chatgpt.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      # Dedicated test-only artifact permits local HTTP transport fixtures
      # without adding a loopback URL override to the production provider.
      providerOpenAiApiFixture = rust.buildRustPackage {
        pname = "phenix-plugin-provider-openai-api-test";
        version = "0.1.0";
        src = providerOpenAiApiSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-openai-api" "--features" "c11-loopback-test" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-openai-api" "--features" "c11-loopback-test" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_openai_api.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      providerOpenAiApiProductCheck = pkgs.runCommand "phenix-provider-openai-api-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        export PYTHONPATH=${./tests}
        python ${./tests/acp-openai-api-key-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime ${basicHttpComposition} \
          ${providerOpenAiApiFixture} ${providerCredentialStore} ${providerDefinitions} "$out"
        test -s "$out/openai-api-auth-product.json"
      '';
      # C11b: separately installed API-key ACP control plane. This initial
      # opt-in preset authorizes exactly one non-secret profile identity.
      # Changing profiles requires an explicit new composition, not a
      # frontend-supplied selector or a broadened credential root.
      openAiApiCredentialResource =
        "phenix.credentials.6f70656e6169.617069.706572736f6e616c";
      openAiApiProviderOperations = [
        { interface = "phenix.provider.catalog@1"; operation = "list"; }
        { interface = "phenix.provider.catalog@1"; operation = "describe"; }
        { interface = "phenix.provider.catalog@1"; operation = "models"; }
        { interface = "phenix.provider.auth@1"; operation = "status"; }
        { interface = "phenix.provider.auth@1"; operation = "begin"; }
        { interface = "phenix.provider.auth@1"; operation = "poll"; }
        { interface = "phenix.provider.auth@1"; operation = "cancel"; }
      ];
      openAiApiManagementComposition = pkgs.writeText "phenix-openai-api-management.json"
        (builtins.toJSON {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.catalog@1";
            operation = "list";
            provider = "phenix.provider.openai-api";
          };
          definitions = [ "${providerDefinitions}" ];
          artifacts = [ "${providerOpenAiApi}" "${providerCredentialStore}" ];
          slots = [
            {
              caller = "phenix.provider.openai-api";
              slot = "Credentials";
              provider = "phenix.provider.credential-files";
              export = "Credentials";
              index = 0;
            }
          ];
          grants = (map (entry: {
            caller = "root";
            inherit (entry) interface operation;
            resources = [ openAiApiCredentialResource ];
          }) openAiApiProviderOperations) ++ [
            {
              caller = "phenix.provider.openai-api";
              interface = "phenix.provider.credentials@1";
              operation = "resolve";
              resources = [ openAiApiCredentialResource ];
            }
          ];
          authority = {
            operations = (map (entry: "${entry.interface}/${entry.operation}") openAiApiProviderOperations)
              ++ [ "phenix.provider.credentials@1/resolve" ];
            resources = [ openAiApiCredentialResource ];
          };
          resource = openAiApiCredentialResource;
          excluded = [];
          substitutions = [];
        });
      openAiApiCredentialComposition = pkgs.writeText "phenix-openai-api-credentials.json"
        (builtins.toJSON {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.credentials@1";
            operation = "import";
            provider = "phenix.provider.credential-files";
          };
          definitions = [ "${providerDefinitions}" ];
          artifacts = [ "${providerCredentialStore}" ];
          slots = [];
          grants = map (operation: {
            caller = "root";
            interface = "phenix.provider.credentials@1";
            inherit operation;
            resources = [ openAiApiCredentialResource ];
          }) [ "import" "bind_environment" "revoke" ];
          authority = {
            operations = [
              "phenix.provider.credentials@1/import"
              "phenix.provider.credentials@1/bind_environment"
              "phenix.provider.credentials@1/revoke"
            ];
            resources = [ openAiApiCredentialResource ];
          };
          resource = openAiApiCredentialResource;
          excluded = [];
          substitutions = [];
        });
      openAiApiResolverComposition = pkgs.writeText "phenix-openai-api-resolver.json"
        (builtins.toJSON {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.credentials@1";
            operation = "resolve";
            provider = "phenix.provider.credential-files";
          };
          definitions = [ "${providerDefinitions}" ];
          artifacts = [ "${providerCredentialStore}" ];
          slots = [];
          grants = [
            {
              caller = "root";
              interface = "phenix.provider.credentials@1";
              operation = "resolve";
              resources = [ openAiApiCredentialResource ];
            }
          ];
          authority = {
            operations = [ "phenix.provider.credentials@1/resolve" ];
            resources = [ openAiApiCredentialResource ];
          };
          resource = openAiApiCredentialResource;
          excluded = [];
          substitutions = [];
        });
      openAiApiProviderBundle = pkgs.writeText "phenix-openai-api-bundle.json"
        (builtins.toJSON {
          provider = {
            id = "phenix.provider.openai-api";
            artifact = "${providerOpenAiApi}";
          };
          credentials = {
            id = "phenix.provider.credential-files";
            artifact = "${providerCredentialStore}";
          };
          definitions = [ "${providerDefinitions}" ];
          account = {
            provider_id = "openai";
            connection_id = "api";
            profile_id = "personal";
          };
          provider_credential_operations = [ "resolve" ];
          credential_management_operations = [ "import" "bind_environment" "revoke" ];
          model_resolution = true;
        });
      # C13f: browser OAuth provider runs in its own exact-account root.
      # A missing credential directory or import/resolve grant is a hard error,
      # not an implicit no-auth or API-key provider fallback.
      chatGptCredentialResource =
        "phenix.credentials.6f70656e6169.63686174677074.706572736f6e616c";
      # The public root can only request sign-out. The provider plugin
      # privately resolves and CAS-clears the exact OAuth account before
      # attempting remote revocation. The editor has no access to tokens.
      chatGptLogoutComposition = pkgs.writeText "phenix-chatgpt-provider-logout.json"
        (builtins.toJSON {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.credentials@1";
            operation = "revoke";
            provider = "phenix.provider.chatgpt";
          };
          definitions = [ "${providerDefinitions}" ];
          artifacts = [ "${providerChatGpt}" "${providerCredentialStore}" ];
          slots = [{
            caller = "phenix.provider.chatgpt";
            slot = "Credentials";
            provider = "phenix.provider.credential-files";
            export = "Credentials";
            index = 0;
          }];
          grants = [
            {
              caller = "root";
              interface = "phenix.provider.credentials@1";
              operation = "revoke";
              resources = [ chatGptCredentialResource ];
            }
          ] ++ (map (operation: {
            caller = "phenix.provider.chatgpt";
            interface = "phenix.provider.credentials@1";
            inherit operation;
            resources = [ chatGptCredentialResource ];
          }) [ "resolve" "compare_exchange" ]);
          authority = {
            operations = [
              "phenix.provider.credentials@1/revoke"
              "phenix.provider.credentials@1/resolve"
              "phenix.provider.credentials@1/compare_exchange"
            ];
            resources = [ chatGptCredentialResource ];
          };
          resource = chatGptCredentialResource;
          excluded = [];
          substitutions = [];
        });
      chatGptManagementComposition = pkgs.writeText "phenix-chatgpt-management.json"
        (builtins.toJSON {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.catalog@1";
            operation = "list";
            provider = "phenix.provider.chatgpt";
          };
          definitions = [ "${providerDefinitions}" ];
          artifacts = [ "${providerChatGpt}" "${providerCredentialStore}" ];
          slots = [
            {
              caller = "phenix.provider.chatgpt";
              slot = "Credentials";
              provider = "phenix.provider.credential-files";
              export = "Credentials";
              index = 0;
            }
          ];
          grants = (map (entry: {
            caller = "root";
            inherit (entry) interface operation;
            resources = [ chatGptCredentialResource ];
          }) openAiApiProviderOperations) ++ (map (operation: {
            caller = "phenix.provider.chatgpt";
            interface = "phenix.provider.credentials@1";
            inherit operation;
            resources = [ chatGptCredentialResource ];
          }) [ "resolve" "import" "compare_exchange" ]);
          authority = {
            operations = (map (entry: "${entry.interface}/${entry.operation}") openAiApiProviderOperations)
              ++ [ "phenix.provider.credentials@1/resolve" "phenix.provider.credentials@1/import"
                   "phenix.provider.credentials@1/compare_exchange" ];
            resources = [ chatGptCredentialResource ];
          };
          resource = chatGptCredentialResource;
          excluded = [];
          substitutions = [];
        });
      # Private account-bound OAuth bearer handoff: the same plugin verifies
      # its protected OAuth record before exposing a short-lived bearer to
      # the existing generic selected Model installation.
      chatGptResolverComposition = pkgs.writeText "phenix-chatgpt-resolver.json"
        (builtins.toJSON {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.credentials@1";
            operation = "resolve";
            provider = "phenix.provider.chatgpt";
          };
          definitions = [ "${providerDefinitions}" ];
          artifacts = [ "${providerChatGpt}" "${providerCredentialStore}" ];
          slots = [{
            caller = "phenix.provider.chatgpt";
            slot = "Credentials";
            provider = "phenix.provider.credential-files";
            export = "Credentials";
            index = 0;
          }];
          grants = [
            {
              caller = "root";
              interface = "phenix.provider.credentials@1";
              operation = "resolve";
              resources = [ chatGptCredentialResource ];
            }
            {
              caller = "phenix.provider.chatgpt";
              interface = "phenix.provider.credentials@1";
              operation = "resolve";
              resources = [ chatGptCredentialResource ];
            }
            {
              caller = "phenix.provider.chatgpt";
              interface = "phenix.provider.credentials@1";
              operation = "compare_exchange";
              resources = [ chatGptCredentialResource ];
            }
          ];
          authority = {
            operations = [ "phenix.provider.credentials@1/resolve"
              "phenix.provider.credentials@1/compare_exchange" ];
            resources = [ chatGptCredentialResource ];
          };
          resource = chatGptCredentialResource;
          excluded = [];
          substitutions = [];
        });
      # Installed wrapper contract: no preconfigured credential variables are
      # required, while explicit opt-out and unsafe paths are rejected.
      p1AuthLauncherProductCheck = pkgs.runCommand "phenix-p1-auth-launcher-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        ${pkgs.python3}/bin/python ${./tests/p1-auth-launcher-product.py} \
          ${self.packages.${system}.phenix-basic-p1-auth-acp}/bin/phenix-basic-p1-auth-acp "$out"
      '';
      p1CombinedAuthProductCheck = pkgs.runCommand "phenix-p1-combined-auth-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        set -eu
        mkdir -p "$out"
        ${pkgs.python3}/bin/python ${./tests/acp-p1-combined-auth-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime \
          ${basicHttpComposition} \
          ${openAiApiManagementComposition} \
          ${chatGptManagementComposition} \
          ${openAiApiCredentialComposition} \
          ${openAiApiResolverComposition} \
          ${chatGptResolverComposition} \
          ${chatGptLogoutComposition} \
          "$out"
      '';
      providerOpenAiApiLauncherCheck = pkgs.runCommand "phenix-openai-api-launcher-check" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/check-openai-auth-presets.py} \
          ${openAiApiManagementComposition} \
          ${openAiApiCredentialComposition} \
          ${openAiApiResolverComposition} \
          ${openAiApiCredentialResource} "$out"
        # Without explicit operator consent this cannot initialize a backend
        # or prompt for/resolve a secret through a hidden default.
        if PHENIX_CREDENTIAL_FILE_CONSENT=0 PHENIX_CREDENTIAL_FILE_DIRECTORY="" \
          ${self.packages.${system}.phenix-basic-openai-api-acp}/bin/phenix-basic-openai-api-acp \
          </dev/null >"$out/unexpected-stdout" 2>"$out/refusal"; then
          echo "OpenAI API launcher started without credential consent" >&2
          exit 1
        fi
        grep -F "Set PHENIX_CREDENTIAL_FILE_DIRECTORY" "$out/refusal"
        test -s "$out/launcher-authority.json"
        # Exercise the shipping *grants* and composition paths with the
        # dedicated deterministic loopback provider artifact; the production
        # provider cannot be redirected to a test host at runtime.
        export PYTHONPATH=${./tests}
        python ${./tests/acp-openai-api-key-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime ${basicHttpComposition} \
          ${providerOpenAiApiFixture} ${providerCredentialStore} ${providerDefinitions} "$out/streamed" \
          ${openAiApiManagementComposition} ${openAiApiCredentialComposition} ${openAiApiResolverComposition} ${openAiApiProviderBundle}
        grep -F '"shipping_launcher_authority_exercised": true' "$out/streamed/openai-api-auth-product.json"
      '';
      providerCredentialStoreSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-credential-files
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
        ./crates/phenix-provider-credential-files
      ]);
      providerCredentialStore = rust.buildRustPackage {
        pname = "phenix-plugin-provider-credential-files";
        version = "0.1.0";
        src = providerCredentialStoreSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-credential-files" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-credential-files" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_credential_files.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      providerCredentialStoreCheck = pkgs.runCommand "phenix-provider-credential-store-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        python ${./tests/provider-credential-store-product.py} \
          ${kernel}/bin/phenix-kernel ${providerCredentialStore} ${providerDefinitions} "$out"
      '';
      providerSecretServiceSource = subset (publicPaths ++ [
        ./crates/phenix-plugin-provider-secret-service
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerSecretService = rust.buildRustPackage {
        pname = "phenix-plugin-provider-secret-service";
        version = "0.1.0";
        src = providerSecretServiceSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-provider-secret-service" ];
        cargoTestFlags = [ "-p" "phenix-plugin-provider-secret-service" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out/lib
          cp target/*/release/libphenix_plugin_provider_secret_service.so $out/lib/plugin.so
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      providerSecretServiceCheck = pkgs.runCommand "phenix-provider-secret-service-product" {
        nativeBuildInputs = [ pkgs.python3 pkgs.dbus pkgs.gnome-keyring ];
      } ''
        export HOME="$TMPDIR/home"
        export XDG_DATA_HOME="$HOME/.local/share"
        export XDG_CONFIG_HOME="$HOME/.config"
        export XDG_RUNTIME_DIR="$TMPDIR/runtime"
        mkdir -p "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
        chmod 700 "$HOME" "$XDG_RUNTIME_DIR"
        export PHENIX_ISOLATED_SECRET_SERVICE_TEST=1
        dbus-run-session --config-file=${pkgs.dbus}/share/dbus-1/session.conf -- python ${./tests/provider-secret-service-product.py} \
          ${kernel}/bin/phenix-kernel ${providerSecretService} ${providerDefinitions} "$out"
      '';
      providerCredentialFilesSource = subset (publicPaths ++ [ ./crates/phenix-provider-credential-files ]);
      providerCredentialFilesCheck = rust.buildRustPackage {
        pname = "phenix-provider-credential-files";
        version = "0.1.0";
        src = providerCredentialFilesSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-provider-credential-files" ];
        cargoTestFlags = [ "-p" "phenix-provider-credential-files" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out
          touch $out/verified
        '';
      };
      providerProtocolsSource = subset (publicPaths ++ [ ./crates/phenix-provider-protocols ./crates/phenix-basic-contracts ]);
      providerProtocolsCheck = rust.buildRustPackage {
        pname = "phenix-provider-protocols";
        version = "0.1.0";
        src = providerProtocolsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-provider-protocols" "--features" "model-codecs,siwc-verified" ];
        cargoTestFlags = [ "-p" "phenix-provider-protocols" "--features" "model-codecs,siwc-verified" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out
          touch $out/verified
        '';
      };
      providerDiscoverySource = subset (publicPaths ++ [ ./crates/phenix-provider-discovery ./crates/phenix-provider-protocols ./crates/phenix-basic-contracts ]);
      providerDiscoveryCheck = rust.buildRustPackage {
        pname = "phenix-provider-discovery";
        version = "0.1.0";
        src = providerDiscoverySource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-provider-discovery" ];
        cargoTestFlags = [ "-p" "phenix-provider-discovery" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p $out
          touch $out/verified
        '';
      };
      providerContractsSource = subset (publicPaths ++ [
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-basic-contracts
      ]);
      providerContractsBinary = rust.buildRustPackage {
        pname = "phenix-provider-contracts";
        version = "0.1.0";
        src = providerContractsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-provider-contracts" ];
        cargoTestFlags = [ "-p" "phenix-provider-contracts" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
      };
      providerDefinitions = definitionPackage "phenix-provider-contracts" "${providerContractsBinary}/bin/phenix-provider-contracts" false;
      providerContractsCheck = pkgs.runCommand "phenix-provider-contracts-check" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p $out
        python - "${providerDefinitions}/inspection.json" "$out" <<'PY'
        import json, sys
        from pathlib import Path
        report = json.load(open(sys.argv[1]))
        expected = {
            "phenix.provider.catalog@1",
            "phenix.provider.registrations@1",
            "phenix.provider.registry@1",
            "phenix.provider.startup@1",
            "phenix.provider.settlement@1",
            "phenix.provider.populate@1",
            "phenix.provider.auth@1",
            "phenix.provider.runtime@1",
            "phenix.provider.credentials@1",
            "phenix.provider.http-model-session@1",
        }
        assert set(report["definitions"]) == expected, report["definitions"]
        assert "phenix.provider.runtime@1" in report["definitions"], report
        assert "phenix.provider.registrations@1" in report["definitions"], report
        assert "phenix.provider.credentials@1" in report["definitions"]
        assert "phenix.provider.http-model-session@1" in report["definitions"]
        assert report["plugin"] == "phenix.provider-contracts"
        (Path(sys.argv[2]) / "evidence.json").write_text(json.dumps(report, indent=2, sort_keys=True))
        PY
      '';
      advancedContractsSource = subset (publicPaths ++ [
        ./crates/phenix-basic-contracts
        ./crates/phenix-advanced-contracts
      ]);
      advancedContractsBinary = rust.buildRustPackage {
        pname = "phenix-advanced-contracts";
        version = "0.1.0";
        src = advancedContractsSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-advanced-contracts" ];
        cargoTestFlags = [ "-p" "phenix-advanced-contracts" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
      };
      # The byte-token tokenizer is a deterministic fixture, never a
      # production estimate. Build it independently and run its real tests.
      p2ModelMeterFixtureCheck = rust.buildRustPackage {
        pname = "phenix-plugin-model-meter-fixture";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-plugin-model-meter-fixture
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-model-meter-fixture" ];
        cargoTestFlags = [ "-p" "phenix-plugin-model-meter-fixture" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_model_meter_fixture.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      p2ModelMeterProductRunner = build "phenix-p2-model-meter-product" (subset (publicPaths ++ [
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-basic-contracts
        ./crates/phenix-advanced-contracts
        ./crates/phenix-p2-model-meter-product
      ]));
      p2ModelMeterProductCheck = pkgs.runCommand "phenix-p2-model-meter-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-model-meter-product.py} \
          ${p2ModelMeterProductRunner}/bin/phenix-p2-model-meter-product \
          ${p2ModelMeterFixtureCheck} ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/accepted.json"
      '';
      # P2-3 source bytes stay in the authorized Basic journal; this package
      # checks canonical spans and exact expansion without duplicating storage.
      p2CompactionReplayCheck = rust.buildRustPackage {
        pname = "phenix-p2-compaction-replay";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-p2-compaction-replay
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-p2-compaction-replay" ];
        cargoTestFlags = [ "-p" "phenix-p2-compaction-replay" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
      };
      # Opt-in selected Model summarization; no hidden tools, journal writes
      # or checkpoint authority. Build/test it outside the public Advanced
      # preset until its provider and authority binding are selected.
      p2SelectedModelSummarizer = rust.buildRustPackage {
        pname = "phenix-plugin-advanced-summarizer";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-p2-compaction-replay
          ./crates/phenix-plugin-advanced-summarizer
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-advanced-summarizer" ];
        cargoTestFlags = [ "-p" "phenix-plugin-advanced-summarizer" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_advanced_summarizer.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      compactionCheckpointStore = rust.buildRustPackage {
        pname = "phenix-plugin-compaction-checkpoints";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-p2-compaction-replay
          ./crates/phenix-plugin-compaction-checkpoints
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-compaction-checkpoints" ];
        cargoTestFlags = [ "-p" "phenix-plugin-compaction-checkpoints" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_compaction_checkpoints.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      advancedDefinitions = definitionPackage "phenix-advanced-contracts"
        "${advancedContractsBinary}/bin/phenix-advanced-contracts" true;
      advancedContractsCheck = pkgs.runCommand "phenix-p2-evidence-contracts" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python - "${advancedDefinitions}/inspection.json" "$out" <<'PY'
        import json, sys
        from pathlib import Path
        report = json.load(open(sys.argv[1]))
        assert report["plugin"] == "phenix.advanced-contracts"
        assert len(report["definitions"]) == 9, report
        assert "phenix.advanced.model-preparation-change@1" in report["definitions"], report
        assert "phenix.advanced.model-preparation@1" in report["definitions"], report
        assert "phenix.advanced.compaction-summary@1" in report["definitions"], report
        (Path(sys.argv[2]) / "evidence.json").write_text(
            json.dumps({"format": "phenix.p2.evidence-contracts.v1",
                        "definitions": report["definitions"],
                        "metadata_only": True}, sort_keys=True, indent=2)
        )
        PY
      '';
      evidenceJournal = rust.buildRustPackage {
        pname = "phenix-plugin-evidence-journal";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-plugin-evidence-journal
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-evidence-journal" ];
        cargoTestFlags = [ "-p" "phenix-plugin-evidence-journal" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_evidence_journal.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      evidenceStore = rust.buildRustPackage {
        pname = "phenix-plugin-evidence-store";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-plugin-evidence-store
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-evidence-store" ];
        cargoTestFlags = [ "-p" "phenix-plugin-evidence-store" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_evidence_store.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      projectStorageSqlite = rust.buildRustPackage {
        pname = "phenix-plugin-project-storage-sqlite";
        version = "0.1.0";
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.sqlite ];
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-plugin-project-storage-sqlite
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-project-storage-sqlite" ];
        cargoTestFlags = [ "-p" "phenix-plugin-project-storage-sqlite" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_project_storage_sqlite.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      # Keep the native artifact/package ABI unchanged, but isolate each Basic
      # plugin's Rust sources in its own Nix derivation. A change to one plugin
      # must not rebuild the other default-feature plugins merely because they
      # previously shared a Cargo batch. Common SDK/contracts remain shared
      # source dependencies, so changes to them legitimately rebuild consumers.
      # Example fixture plugins may still be batch-built for conformance speed.
      basicNativePlugin = name: library: pluginPath:
        rust.buildRustPackage {
          pname = name;
          version = "0.1.0";
          src = subset (publicPaths ++ [
            ./crates/phenix-basic-contracts
            pluginPath
          ]);
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" name ];
          preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
          doCheck = false;
          installPhase = ''
            mkdir -p $out/lib
            cp target/*/release/lib${library}.so $out/lib/plugin.so
          '';
          postFixup = ''
            ${describe}/bin/phenix-describe $out/lib/plugin.so $out/descriptor.bin > digests.json
            ${pkgs.python3}/bin/python ${recordWriter} $out
          '';
        };
      # The Rust product host resolves the state directory once. Wrappers
      # select only the allowed Basic preset file and a private creation mask.
      # PHENIX_BASIC_SQLITE_PATH remains an explicit user override.
      basicSqliteEnvironment = fileName: ''
        umask 077
        export PHENIX_BASIC_SQLITE_DEFAULT_NAME="${fileName}"
      '';
      scriptedModel = basicNativePlugin
        "phenix-plugin-model-scripted"
        "phenix_plugin_model_scripted"
        ./crates/phenix-plugin-model-scripted;
      # Verification remains outside the default Basic product and batches.
      advancedModelAdapter = (basicNativePlugin
        "phenix-plugin-advanced-model-adapter"
        "phenix_plugin_advanced_model_adapter"
        ./crates/phenix-plugin-advanced-model-adapter).overrideAttrs (_: {
          src = subset (publicPaths ++ [
            ./crates/phenix-basic-contracts
            ./crates/phenix-advanced-contracts
            ./crates/phenix-p2-compaction-replay
            ./crates/phenix-plugin-advanced-model-adapter
          ]);
        });
      historyTestModel = basicNativePlugin
        "phenix-plugin-model-history-test"
        "phenix_plugin_model_history_test"
        ./crates/phenix-plugin-model-history-test;
      httpModel = (basicNativePlugin
        "phenix-plugin-model-http"
        "phenix_plugin_model_http"
        ./crates/phenix-plugin-model-http).overrideAttrs (old: {
          src = subset (publicPaths ++ [
            ./crates/phenix-basic-contracts
            ./crates/phenix-provider-contracts
            ./crates/phenix-provider-protocols
            ./crates/phenix-plugin-model-http
          ]);
          nativeBuildInputs = (old.nativeBuildInputs or []) ++ [ pkgs.pkg-config ];
          buildInputs = (old.buildInputs or []) ++ [ pkgs.curl pkgs.zlib ];
        });
      # Isolated executable provider fixture for ACP C9 product acceptance.
      # Unlike installed registration-only builtins it deliberately publishes
      # TWO credential-free loopback models. Never included in default Basic,
      # user provider management, shipped client or distribution compositions.
      c9LoopbackProvider = (basicNativePlugin
        "phenix-plugin-provider-loopback-fixture"
        "phenix_plugin_provider_loopback_fixture"
        ./crates/phenix-plugin-provider-loopback-fixture).overrideAttrs (old: {
          src = subset (publicPaths ++ [
            ./crates/phenix-basic-contracts
            ./crates/phenix-provider-contracts
            ./crates/phenix-provider-protocols
            ./crates/phenix-plugin-provider-loopback-fixture
          ]);
        });
      providerRuntimeProductCheck = pkgs.runCommand "phenix-provider-runtime-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/provider-runtime-product.py} \
          ${kernel}/bin/phenix-kernel ${c9LoopbackProvider} ${providerDefinitions} "$out"
        test -s "$out/verified.json"
      '';
      # Provider-specific real native install product: separate from generic
      # phenix-conformance so provider edits do not rebuild kernel fixtures.
      selectedHttpModelRunner = build "phenix-http-selected-product" (subset (publicPaths ++ [
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-basic-contracts
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-http-selected-product
      ]));
      httpModelCheck = pkgs.runCommand "phenix-b4-http" {
        nativeBuildInputs = [ pkgs.python3 pkgs.openssl ];
      } ''
        mkdir -p $out
        export PYTHONPATH=${./tests}
        python ${./tests/http-model.py} ${conformanceBinary}/bin/phenix-b4-http ${httpModel} ${eventJournal} ${storageSqlite} ${basicDefinitions} ${providerDefinitions} $out
        test -s "$out/http-model.json"
        PHENIX_MODEL_HTTP_PROTOCOL=openai-responses python ${./tests/http-model.py} ${conformanceBinary}/bin/phenix-b4-http ${httpModel} ${eventJournal} ${storageSqlite} ${basicDefinitions} ${providerDefinitions} $out/responses
        PHENIX_MODEL_HTTP_PROTOCOL=anthropic-messages python ${./tests/http-model.py} ${conformanceBinary}/bin/phenix-b4-http ${httpModel} ${eventJournal} ${storageSqlite} ${basicDefinitions} ${providerDefinitions} $out/anthropic
        python ${./tests/native-http-selected-product.py} ${selectedHttpModelRunner}/bin/phenix-http-selected-product ${httpModel} ${eventJournal} ${storageSqlite} ${basicDefinitions} ${providerDefinitions} $out/selected-native
        test -s "$out/selected-native/evidence.json"
      '';
      basicTools = basicNativePlugin
        "phenix-plugin-basic-tools"
        "phenix_plugin_basic_tools"
        ./crates/phenix-plugin-basic-tools;
      toolFixture = basicNativePlugin
        "phenix-plugin-tool-fixture"
        "phenix_plugin_tool_fixture"
        ./crates/phenix-plugin-tool-fixture;
      eventJournal = basicNativePlugin
        "phenix-plugin-event-journal"
        "phenix_plugin_event_journal"
        ./crates/phenix-plugin-event-journal;
      storageFixture = basicNativePlugin
        "phenix-plugin-storage-fixture"
        "phenix_plugin_storage_fixture"
        ./crates/phenix-plugin-storage-fixture;
      basicSession = basicNativePlugin
        "phenix-plugin-basic-session"
        "phenix_plugin_basic_session"
        ./crates/phenix-plugin-basic-session;
      storageSqlite = (basicNativePlugin
        "phenix-plugin-storage-sqlite"
        "phenix_plugin_storage_sqlite"
        ./crates/phenix-plugin-storage-sqlite).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or []) ++ [ pkgs.pkg-config ];
          buildInputs = (old.buildInputs or []) ++ [ pkgs.sqlite ];
        });
      sqliteRestartCheck = pkgs.runCommand "phenix-b2-sqlite" { } ''
        mkdir -p $out
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-sessions.db"
        ${conformanceBinary}/bin/phenix-b2-sqlite seed ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        test "$(${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$PHENIX_BASIC_SQLITE_PATH" 'PRAGMA user_version;')" = 1

        # Migrate a pre-versioned database while preserving its durable
        # session and event journal. The initial v0 layout is unchanged.
        cp "$PHENIX_BASIC_SQLITE_PATH" "$TMPDIR/legacy-basic.db"
        ${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$TMPDIR/legacy-basic.db" "PRAGMA user_version = 0;"
        PHENIX_BASIC_SQLITE_PATH="$TMPDIR/legacy-basic.db" ${conformanceBinary}/bin/phenix-b2-sqlite resume ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        test "$(${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$TMPDIR/legacy-basic.db" 'PRAGMA user_version;')" = 1

        # Never overwrite a future file or accept a v1 file with a missing
        # table. An incompatible unversioned layout also fails closed.
        cp "$PHENIX_BASIC_SQLITE_PATH" "$TMPDIR/future-basic.db"
        ${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$TMPDIR/future-basic.db" "PRAGMA user_version = 2;"
        PHENIX_BASIC_SQLITE_PATH="$TMPDIR/future-basic.db" ${conformanceBinary}/bin/phenix-b2-sqlite schema-rejected ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        ${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$TMPDIR/malformed-basic.db" "CREATE TABLE basic_records (session TEXT NOT NULL, name TEXT NOT NULL);"
        PHENIX_BASIC_SQLITE_PATH="$TMPDIR/malformed-basic.db" ${conformanceBinary}/bin/phenix-b2-sqlite schema-rejected ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        ${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$TMPDIR/missing-basic.db" "PRAGMA user_version = 1;"
        PHENIX_BASIC_SQLITE_PATH="$TMPDIR/missing-basic.db" ${conformanceBinary}/bin/phenix-b2-sqlite schema-rejected ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        cat > "$out/sqlite-schema-migration.json" <<'JSON'
        {"format":"phenix.b2.sqlite-schema.v1","cases":["new-schema-v1","legacy-v0-upgrade-preserves-data","future-v2-rejected","malformed-v0-rejected","missing-v1-table-rejected"]}
        JSON

        # Inject a real SQLite statement error on the second update in a
        # two-record CAS transaction; verify the first update also rolls back.
        ${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$PHENIX_BASIC_SQLITE_PATH" \
          "CREATE TRIGGER reject_second_edit BEFORE UPDATE ON basic_records WHEN NEW.name = 'atomic-b' BEGIN SELECT RAISE(ABORT, 'injected SQLite write failure'); END;"
        ${conformanceBinary}/bin/phenix-b2-sqlite faults ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        test -s "$out/sqlite-faults.json"
        ${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$PHENIX_BASIC_SQLITE_PATH" "DROP TRIGGER reject_second_edit;"
        ${conformanceBinary}/bin/phenix-b2-sqlite resume ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        test -s "$PHENIX_BASIC_SQLITE_PATH"
        test -s "$out/sqlite-restart.json"
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-observers.db"
        observer_fixture() {
          ${conformanceBinary}/bin/phenix-b2-observers "$1" ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        }
        observer_fixture seed
        set +e
        observer_fixture crash-before
        before_status=$?
        set -e
        test "$before_status" -eq 71
        observer_fixture verify-before
        set +e
        observer_fixture crash-after
        after_status=$?
        set -e
        test "$after_status" -eq 72
        observer_fixture resume
        test -s "$out/observer-restart.json"

        # Independent processes with independently loaded native plugins
        # contend for the same two-record selected-model/witness CAS.
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/native-cas.db"
        PHENIX_CAS_WORKER=alpha ${conformanceBinary}/bin/phenix-b2-sqlite native-cas-once ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out &
        first=$!
        PHENIX_CAS_WORKER=beta ${conformanceBinary}/bin/phenix-b2-sqlite native-cas-once ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out &
        second=$!
        for i in $(seq 1 300); do
          if test -f "$out/alpha.ready" && test -f "$out/beta.ready"; then break; fi
          sleep 0.05
        done
        test -f "$out/alpha.ready"
        test -f "$out/beta.ready"
        touch "$out/start-cas"
        wait "$first"
        wait "$second"
        test "$(grep -lx committed "$out/alpha.result" "$out/beta.result" | wc -l)" -eq 1
        test "$(grep -lx conflict "$out/alpha.result" "$out/beta.result" | wc -l)" -eq 1
        test "$(${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$PHENIX_BASIC_SQLITE_PATH"           "SELECT COUNT(*) FROM basic_records WHERE session='s1' AND name IN ('phenix.provider.selected-model.v1','phenix.provider.native-routing-witness.v1') AND revision=1;")" -eq 2
        test "$(${pkgs.lib.getBin pkgs.sqlite}/bin/sqlite3 "$PHENIX_BASIC_SQLITE_PATH"           "SELECT COUNT(DISTINCT hex(data)) FROM basic_records WHERE session='s1' AND name IN ('phenix.provider.selected-model.v1','phenix.provider.native-routing-witness.v1');")" -eq 1
        cat > "$out/native-cas-concurrent.json" <<'JSON'
        {"format":"phenix.storage.native-cas.v1","processes":2,"committed":1,"conflicts":1,"atomic_pair":true}
        JSON
      '';
      basicMemory = basicNativePlugin
        "phenix-plugin-basic-memory"
        "phenix_plugin_basic_memory"
        ./crates/phenix-plugin-basic-memory;
      basicMemoryCheck = pkgs.runCommand "phenix-b3-memory" { } ''
        mkdir -p $out
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-memory.db"
        ${conformanceBinary}/bin/phenix-b3-memory seed ${basicMemory} ${storageSqlite} ${basicDefinitions} $out
        ${conformanceBinary}/bin/phenix-b3-memory reload ${basicMemory} ${storageSqlite} ${basicDefinitions} $out
        test -s "$out/memory-restart.json"
      '';
      basicAgent = basicNativePlugin
        "phenix-plugin-basic-agent"
        "phenix_plugin_basic_agent"
        ./crates/phenix-plugin-basic-agent;
      # Verification-only feature-variant of the very same Basic agent.
      # The P1 preset and batched default-feature binary are unchanged.
      basicStagedAgent = rust.buildRustPackage {
        pname = "phenix-plugin-basic-agent-staged";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-plugin-basic-agent
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-basic-agent" "--features" "staged-graph" ];
        cargoTestFlags = [ "-p" "phenix-plugin-basic-agent" "--features" "staged-graph" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_basic_agent.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
          test -s "$out/descriptor.bin"
        '';
      };
      p2BasicStagedPrepareCheck = pkgs.runCommand "phenix-p2-basic-staged-prepare"
        { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p "$out"
        ${pkgs.python3}/bin/python ${./tests/p2-basic-staged-prepare-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime \
          ${basicComposition} ${basicAgent} ${basicStagedAgent} "$out"
        test -s "$out/accepted.json"
      '';
      # Test-only selected graph preparation, with a replaceable no-op
      # ModelPreparation provider. These are independently packaged M1 plugins.
      basicGraphPreparerPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-advanced-preparer-graph";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-plugin-advanced-preparer-graph
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-advanced-preparer-graph" ];
        cargoTestFlags = [ "-p" "phenix-plugin-advanced-preparer-graph" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_advanced_preparer_graph.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      basicNoopPreparerPlugin = rust.buildRustPackage {
        pname = "phenix-plugin-advanced-preparer-noop";
        version = "0.1.0";
        src = subset (publicPaths ++ [
          ./crates/phenix-basic-contracts
          ./crates/phenix-advanced-contracts
          ./crates/phenix-plugin-advanced-preparer-noop
        ]);
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-plugin-advanced-preparer-noop" ];
        cargoTestFlags = [ "-p" "phenix-plugin-advanced-preparer-noop" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = true;
        installPhase = ''
          mkdir -p "$out/lib"
          cp target/*/release/libphenix_plugin_advanced_preparer_noop.so "$out/lib/plugin.so"
        '';
        postFixup = ''
          ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
          ${pkgs.python3}/bin/python ${recordWriter} "$out"
        '';
      };
      basicSelectedGraphPrepareCheck = pkgs.runCommand
        "phenix-p2-selected-graph-preparer"
        { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p "$out"
        ${pkgs.python3}/bin/python ${./tests/p2-basic-staged-prepare-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime \
          ${basicComposition} ${basicAgent} ${basicStagedAgent} "$out" \
          ${basicGraphPreparerPlugin} ${basicNoopPreparerPlugin} ${advancedDefinitions} \
          ${p2ModelMeterFixtureCheck}
        test -s "$out/accepted.json"
      '';
      basicAgentCheck = pkgs.runCommand "phenix-b5-agent" { } ''
        mkdir -p $out
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-agent.db"
        ${conformanceBinary}/bin/phenix-b5-agent ${basicAgent} ${scriptedModel} ${basicTools} ${basicMemory} ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        test -s "$out/agent.json"
      '';
      frontendTerminal = basicNativePlugin
        "phenix-plugin-basic-frontends"
        "phenix_plugin_basic_frontends"
        ./crates/phenix-plugin-basic-frontends;
      frontendJsonl = frontendTerminal.overrideAttrs (old: {
        pname = "phenix-basic-frontend-jsonl";
        cargoBuildFlags = old.cargoBuildFlags ++ [
          "--no-default-features" "--features" "jsonl"
        ];
      });
      frontendAcp = frontendTerminal.overrideAttrs (old: {
        pname = "phenix-basic-frontend-acp";
        cargoBuildFlags = old.cargoBuildFlags ++ [
          "--no-default-features" "--features" "acp"
        ];
      });
      acpCheck = pkgs.runCommand "phenix-f2-acp" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-f2-acp ${frontendAcp} ${basicDefinitions} $out
        test -s "$out/acp-codec.json"
      '';
      frontendCheck = pkgs.runCommand "phenix-f1-frontends" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-f1-frontends ${frontendTerminal} ${frontendJsonl} ${basicDefinitions} $out
        test -s "$out/frontends.json"
      '';
      basicRuntimeSource = subset [
        ./Cargo.toml
        ./Cargo.lock
        ./crates/phenix-contract
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-sdk
        ./crates/phenix-macros
        ./crates/phenix-basic-contracts
        ./crates/phenix-provider-contracts
        ./crates/phenix-provider-protocols
        ./crates/phenix-environment-contracts
        ./crates/phenix-basic-runtime
        ./tests/generation-identity-v2.json
      ];
      basicRuntime = build "phenix-basic-runtime" basicRuntimeSource;
      # Native end-to-end event population, driven by a small product test
      # binary outside the kernel. Uses the existing GraphExecution API.
      providerPopulationTestBinary = rust.buildRustPackage {
        pname = "phenix-provider-population-test";
        version = "0.1.0";
        src = basicRuntimeSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-basic-runtime" "--bin" "phenix-provider-population-test" ];
        doCheck = false;
        installPhase = ''
          mkdir -p $out/bin
          cp target/*/release/phenix-provider-population-test $out/bin/
        '';
      };
      providerPopulationProductCheck = pkgs.runCommand "phenix-provider-population-product" { } ''
        mkdir -p $out
        ${providerPopulationTestBinary}/bin/phenix-provider-population-test \
          ${providerHostPlugin} \
          ${providerOpenAiDeclarationPlugin} \
          ${providerStandardDeclarationsPlugin} \
          ${providerDefinitions} > $out/verified.json
        test -s $out/verified.json
      '';

      # Native tool event population drives the *existing* graph executor,
      # then proves selected executor routing after manager-owned settlement.
      toolPopulationTestBinary = rust.buildRustPackage {
        pname = "phenix-tool-population-test";
        version = "0.1.0";
        src = basicRuntimeSource;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "phenix-basic-runtime" "--bin" "phenix-tool-population-test" ];
        doCheck = false;
        installPhase = ''
          mkdir -p $out/bin
          cp target/*/release/phenix-tool-population-test $out/bin/
        '';
      };
      toolPopulationProductCheck = pkgs.runCommand "phenix-tool-population-product" { } ''
        mkdir -p $out
        ${toolPopulationTestBinary}/bin/phenix-tool-population-test \
          ${basicTools} \
          ${toolFixture} \
          ${basicDefinitions} > $out/verified.json
        test -s $out/verified.json
      '';

      basicGrantSpecs = [
        { caller = "root"; key = "phenix.basic.input@1/submit"; }
        { caller = "root"; key = "phenix.basic.output@1/read"; }
        { caller = "root"; key = "phenix.basic.output@1/observer"; }
        { caller = "root"; key = "phenix.basic.output@1/checkpoint_observer"; }
        { caller = "root"; key = "phenix.basic.frontend@1/decode"; }
        { caller = "root"; key = "phenix.basic.frontend@1/render"; }
        # Live observation and cancellation are explicit host control operations.
        { caller = "root"; key = "phenix.basic.journal@1/read"; }
        { caller = "root"; key = "phenix.basic.journal@1/append"; }
        { caller = "root"; key = "phenix.basic.journal@1/observer"; }
        { caller = "root"; key = "phenix.basic.journal@1/checkpoint_observer"; }
        { caller = "root"; key = "phenix.basic.session@1/open"; }
        { caller = "root"; key = "phenix.basic.session@1/checkpoint"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.input@1/submit"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/append"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/read"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/observer"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/checkpoint_observer"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.model@1/complete"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.catalog@1/list"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.tool@1/execute"; }
        # Tool owner has explicitly scoped, not inferred, lifecycle grants.
        { caller = "phenix.basic.tools"; key = "phenix.tools.startup@1/begin"; }
        { caller = "phenix.basic.tools"; key = "phenix.tools.settlement@1/complete"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.skills@1/list"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.context@1/list"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.session@1/open"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.session@1/checkpoint"; }
        { caller = "phenix.basic.memory"; key = "phenix.basic.storage@1/read"; }
        { caller = "phenix.basic.memory"; key = "phenix.basic.storage@1/transact"; }
        { caller = "phenix.basic.session"; key = "phenix.basic.storage@1/read"; }
        { caller = "phenix.basic.session"; key = "phenix.basic.storage@1/transact"; }
        { caller = "phenix.basic.event-journal"; key = "phenix.basic.storage@1/read"; }
        { caller = "phenix.basic.event-journal"; key = "phenix.basic.storage@1/transact"; }
      ];
      basicGrants = map (item:
        let key = pkgs.lib.splitString "/" item.key;
        in {
          inherit (item) caller;
          interface = builtins.elemAt key 0;
          operation = builtins.elemAt key 1;
          resources = [ "phenix.session.s1" ];
        }) basicGrantSpecs;
      basicBinding = caller: slot: provider: export: {
        inherit caller slot provider export;
        index = 0;
      };
      basicCompositionSpec = {
        format = "phenix.composition.v3";
        entry = {
          interface = "phenix.basic.input@1";
          operation = "submit";
          provider = "phenix.basic.agent";
        };
        artifacts = [
          "${basicAgent}"
          "${scriptedModel}"
          "${basicTools}"
          "${basicMemory}"
          "${basicSession}"
          "${eventJournal}"
          "${storageSqlite}"
          "${frontendTerminal}"
          "${frontendJsonl}"
          "${frontendAcp}"
        ];
        definitions = [ "${basicDefinitions}" ];
        slots = [
          (basicBinding "phenix.basic.agent" "Model" "phenix.model.scripted" "Model")
          (basicBinding "phenix.basic.agent" "Catalog" "phenix.basic.tools" "Catalog")
          (basicBinding "phenix.basic.agent" "Tools" "phenix.basic.tools" "Tools")
          (basicBinding "phenix.basic.agent" "Skills" "phenix.basic.memory" "Skills")
          (basicBinding "phenix.basic.agent" "Context" "phenix.basic.memory" "Context")
          (basicBinding "phenix.basic.agent" "Session" "phenix.basic.session" "Session")
          (basicBinding "phenix.basic.agent" "Journal" "phenix.basic.event-journal" "Journal")
          (basicBinding "phenix.basic.memory" "Storage" "phenix.basic.storage-sqlite" "Storage")
          (basicBinding "phenix.basic.session" "Storage" "phenix.basic.storage-sqlite" "Storage")
          (basicBinding "phenix.basic.event-journal" "Storage" "phenix.basic.storage-sqlite" "Storage")
        ];
        grants = basicGrants;
        excluded = [];
        substitutions = [];
        authority = {
          operations = pkgs.lib.unique (map (entry: entry.key) basicGrantSpecs);
          resources = [ "phenix.session.s1" ];
        };
        resource = "phenix.session.s1";
      };
      basicComposition = pkgs.writeText "phenix-basic-composition.json"
        (builtins.toJSON basicCompositionSpec);
      # Opt-in, read-only ACP provider management uses the same compiled
      # native plugin and typed definitions as the kernel product test, with
      # an independent resource and exact root operations. The default Basic
      # composition remains byte-for-byte unchanged.
      providerManagementCompositionSpec = {
          format = "phenix.composition.v3";
          entry = {
            interface = "phenix.provider.catalog@1";
            operation = "list";
            provider = "phenix.provider.host";
          };
          artifacts = [ "${providerHostPlugin}" "${providerBuiltinsPlugin}" ];
          definitions = [ "${providerDefinitions}" ];
          slots = [{
            caller = "phenix.provider.host";
            slot = "Contributors";
            index = 0;
            provider = "phenix.provider.builtins";
            export = "Registrations";
          }];
          grants = map (binding: {
            caller = "root";
            interface = binding.interface;
            operation = binding.operation;
            resources = [ "phenix.providers.user" ];
          }) [
            { interface = "phenix.provider.catalog@1"; operation = "list"; }
            { interface = "phenix.provider.catalog@1"; operation = "describe"; }
            { interface = "phenix.provider.auth@1"; operation = "status"; }
            { interface = "phenix.provider.auth@1"; operation = "begin"; }
            { interface = "phenix.provider.auth@1"; operation = "poll"; }
            { interface = "phenix.provider.auth@1"; operation = "cancel"; }
            { interface = "phenix.provider.catalog@1"; operation = "models"; }
          ] ++ map (binding: {
            caller = "phenix.provider.host";
            interface = binding.interface;
            operation = binding.operation;
            resources = [ "phenix.providers.user" ];
          }) [
            { interface = "phenix.provider.registrations@1"; operation = "list"; }
            { interface = "phenix.provider.startup@1"; operation = "begin"; }
            { interface = "phenix.provider.settlement@1"; operation = "complete"; }
          ];
          excluded = [];
          substitutions = [];
          authority = {
            operations = [
              "phenix.provider.catalog@1/list"
              "phenix.provider.catalog@1/describe"
              "phenix.provider.auth@1/status"
              "phenix.provider.auth@1/begin"
              "phenix.provider.auth@1/poll"
              "phenix.provider.auth@1/cancel"
              "phenix.provider.catalog@1/models"
              "phenix.provider.registrations@1/list"
              "phenix.provider.startup@1/begin"
              "phenix.provider.settlement@1/complete"
            ];
            resources = [ "phenix.providers.user" ];
          };
          resource = "phenix.providers.user";
        };
      providerManagementComposition = pkgs.writeText "phenix-provider-management.json"
        (builtins.toJSON providerManagementCompositionSpec);
      providerLoopbackManagementComposition = pkgs.writeText "phenix-provider-loopback-management.json"
        (builtins.toJSON (providerManagementCompositionSpec // {
          entry = providerManagementCompositionSpec.entry // {
            provider = "phenix.provider.loopback-fixture";
          };
          artifacts = [ "${c9LoopbackProvider}" ];
          slots = [];
          grants = builtins.filter (binding:
            binding.caller != "phenix.provider.host"
          ) providerManagementCompositionSpec.grants;
          authority = providerManagementCompositionSpec.authority // {
            operations = builtins.filter (op:
              !(builtins.elem op [
                "phenix.provider.registrations@1/list"
                "phenix.provider.startup@1/begin"
                "phenix.provider.settlement@1/complete"
              ])
            ) providerManagementCompositionSpec.authority.operations;
          };
        }));
      # Opt-in real-provider Basic preset. Change only the selected Model and
      # its journal grant; retain the P1 agent graph, tools and storage.
      basicHttpCompositionSpec = basicCompositionSpec // {
        definitions = basicCompositionSpec.definitions ++ [ "${providerDefinitions}" ];
        artifacts =
          builtins.filter (item: item != "${scriptedModel}") basicCompositionSpec.artifacts
          ++ [ "${httpModel}" ];
        slots = map (binding:
          if binding.caller == "phenix.basic.agent" && binding.slot == "Model"
          then binding // { provider = "phenix.model.http"; }
          else binding
        ) basicCompositionSpec.slots ++ [
          (basicBinding "phenix.model.http" "Journal" "phenix.basic.event-journal" "Journal")
        ];
        grants = basicCompositionSpec.grants ++ [
          { caller = "phenix.model.http";
            interface = "phenix.basic.journal@1";
            operation = "append";
            resources = [ "phenix.session.s1" ];
          }
          # Controller-selected HTTP intent uses the same session's Storage
          # CAS. No grant is added to ordinary Basic, other sessions, or ACP.
          { caller = "root";
            interface = "phenix.basic.storage@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.basic.storage@1";
            operation = "transact";
            resources = [ "phenix.session.s1" ];
          }
          # The controller uses this root only on its existing session kernel,
          # after provider-owned catalog/auth and exact credential checks.
          { caller = "root";
            interface = "phenix.provider.http-model-session@1";
            operation = "install";
            resources = [ "phenix.session.s1" ];
          }
        ];
        authority = basicCompositionSpec.authority // {
          operations = basicCompositionSpec.authority.operations ++ [
            "phenix.provider.http-model-session@1/install"
          ];
        };
      };
      basicHttpComposition = pkgs.writeText "phenix-basic-http-composition.json"
        (builtins.toJSON basicHttpCompositionSpec);
      # The original Basic composition stays byte-for-byte equivalent. The
      # verification product adds one provider artifact and changes one slot.
      historyVerificationCompositionSpec = basicCompositionSpec // {
          artifacts = basicCompositionSpec.artifacts ++ [ "${historyTestModel}" ];
          slots = map (binding:
            if binding.caller == "phenix.basic.agent" && binding.slot == "Model"
            then binding // { provider = "phenix.model.history-test"; }
            else binding
          ) basicCompositionSpec.slots;
        };
      historyVerificationComposition = pkgs.writeText "phenix-basic-history-verification.json"
        (builtins.toJSON historyVerificationCompositionSpec);
      # P2-0: extend immutable package specifications without copying Basic's
      # agent implementation or modifying its selected providers. Later P2
      # plugins fill the additional artifacts/definitions/slots/grants.
      extendComposition =
        base:
        {
          addArtifacts ? [],
          addDefinitions ? [],
          replaceSlots ? [],
          addGrants ? [],
          addOperations ? [],
          addResources ? [],
        }:
        base // {
          artifacts = base.artifacts ++ addArtifacts;
          definitions = base.definitions ++ addDefinitions;
          slots =
            builtins.filter
              (existing:
                !(builtins.any
                  (replacement:
                    replacement.caller == existing.caller
                    && replacement.slot == existing.slot
                    && replacement.index == existing.index)
                  replaceSlots))
              base.slots
            ++ replaceSlots;
          grants = base.grants ++ addGrants;
          authority = base.authority // {
            operations = pkgs.lib.unique (base.authority.operations ++ addOperations);
            resources = pkgs.lib.unique (base.authority.resources ++ addResources);
          };
        };
      # These are deliberately P2-0 scaffolds. No Advanced implementation
      # exists yet; Basic parity is the only behavior claimed in this slice.
      advancedCompositionSpec = extendComposition basicCompositionSpec {
        addDefinitions = [ "${advancedDefinitions}" ];
        addArtifacts = [ "${evidenceJournal}" "${evidenceStore}" "${projectStorageSqlite}" "${compactionCheckpointStore}" "${advancedModelAdapter}" ];
        replaceSlots = [
          (basicBinding "phenix.basic.agent" "Model" "phenix.advanced.model-adapter" "Model")
          (basicBinding "phenix.advanced.model-adapter" "Upstream" "phenix.model.scripted" "Model")
          (basicBinding "phenix.advanced.evidence-journal" "Journal"
            "phenix.basic.event-journal" "Journal")
          (basicBinding "phenix.advanced.compaction-checkpoints" "Storage"
            "phenix.basic.storage-sqlite" "Storage")
          (basicBinding "phenix.advanced.compaction-checkpoints" "Journal"
            "phenix.basic.event-journal" "Journal")
          (basicBinding "phenix.advanced.evidence-store" "Storage"
            "phenix.basic.storage-sqlite" "Storage")
          (basicBinding "phenix.advanced.evidence-store" "Source"
            "phenix.advanced.evidence-journal" "Evidence")
          (basicBinding "phenix.advanced.evidence-store" "ProjectStorage"
            "phenix.advanced.project-storage-sqlite" "ProjectStorage")
          (basicBinding "phenix.advanced.evidence-store" "Membership"
            "phenix.advanced.project-storage-sqlite" "Membership")
          (basicBinding "phenix.advanced.evidence-store" "ProjectSource"
            "phenix.advanced.project-storage-sqlite" "ProjectSource")
        ];
        addGrants = [
          { caller = "phenix.advanced.model-adapter";
            interface = "phenix.basic.model@1";
            operation = "complete";
            resources = [ "phenix.session.s1" ];
          }
          # Explicitly preserve the Basic agent's journal capability through
          # the Model decorator, so a selected streaming Model provider may
          # emit authorized TextDelta events without an independent root.
          { caller = "phenix.advanced.model-adapter";
            interface = "phenix.basic.journal@1";
            operation = "append";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.compaction-checkpoints@1";
            operation = "put";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.compaction-checkpoints@1";
            operation = "get";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.compaction-checkpoints@1";
            operation = "latest";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.compaction-checkpoints";
            interface = "phenix.basic.storage@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.compaction-checkpoints";
            interface = "phenix.basic.storage@1";
            operation = "transact";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.compaction-checkpoints";
            interface = "phenix.basic.journal@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.project-membership@1";
            operation = "get";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.project-storage@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-journal";
            interface = "phenix.basic.journal@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.source@1";
            operation = "observe";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-store";
            interface = "phenix.advanced.project-membership@1";
            operation = "get";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-store";
            interface = "phenix.advanced.project-storage@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-store";
            interface = "phenix.advanced.project-storage@1";
            operation = "transact";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-store";
            interface = "phenix.basic.storage@1";
            operation = "read";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-store";
            interface = "phenix.basic.storage@1";
            operation = "transact";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "phenix.advanced.evidence-store";
            interface = "phenix.advanced.source@1";
            operation = "observe";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.evidence@1";
            operation = "put";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.evidence@1";
            operation = "get";
            resources = [ "phenix.session.s1" ];
          }
          { caller = "root";
            interface = "phenix.advanced.evidence@1";
            operation = "search";
            resources = [ "phenix.session.s1" ];
          }
        ];
        addOperations = [
          "phenix.advanced.compaction-checkpoints@1/put"
          "phenix.advanced.compaction-checkpoints@1/get"
          "phenix.advanced.compaction-checkpoints@1/latest"
          "phenix.advanced.project-membership@1/get"
          "phenix.advanced.project-storage@1/read"
          "phenix.advanced.project-storage@1/transact"
          "phenix.basic.journal@1/read"
          "phenix.advanced.source@1/observe"
          "phenix.advanced.evidence@1/put"
          "phenix.advanced.evidence@1/get"
          "phenix.advanced.evidence@1/search"
        ];
      };
      advancedComposition = pkgs.writeText "phenix-advanced-composition.json"
        (builtins.toJSON advancedCompositionSpec);
      advancedVerificationCompositionSpec = extendComposition advancedCompositionSpec {
          addArtifacts = [ "${historyTestModel}" ];
          replaceSlots = [
            (basicBinding "phenix.advanced.model-adapter" "Upstream" "phenix.model.history-test" "Model")
          ];
        };
      advancedVerificationComposition = pkgs.writeText
        "phenix-advanced-verification-composition.json"
        (builtins.toJSON advancedVerificationCompositionSpec);
      p2CompositionFoundationManifest = pkgs.writeText
        "phenix-p2-composition-foundation.json"
        (builtins.toJSON {
          format = "phenix.p2.composition-foundation.v1";
          status = "scaffold";
          p2_accepted = false;
          selected_model = "phenix.model.scripted";
          verification_model = "phenix.model.history-test";
          unimplemented = [
            "advanced-agent-graph"
            "reference-memory"
            "reversible-compaction"
            "code-entity-lineage"
            "runtime-graph-evolution"
            "workspace"
            "durable-jobs"
            "subagents"
            "inspection"
          ];
        });
      # A complete immutable composition, rather than a runtime that asks
      # users to supply the composition JSON on every invocation.
      basicPreset = name: composition: sqliteDefaultName: pkgs.writeShellApplication {
        inherit name;
        text = ''
          umask 077
          export PHENIX_BASIC_SQLITE_DEFAULT_NAME="${sqliteDefaultName}"
          if [ "$#" -eq 0 ]; then
            mode=terminal
          else
            case "$1" in
              terminal|jsonl|acp)
                mode="$1"
                shift
                ;;
              *)
                echo "usage: ${name} [terminal|jsonl|acp] [--session ID for terminal]" >&2
                exit 2
                ;;
            esac
          fi
          exec ${basicRuntime}/bin/phenix-basic-runtime "$mode" ${composition} "$@"
        '';
      };
      # Standard .override changes package arguments while leaving the
      # original preset and its exported composition unchanged.
      overridablePreset = pkgs.lib.makeOverridable (
        { name, compositionName, format, entry, artifacts, definitions, slots,
          grants, excluded, substitutions, authority, resource,
          sqliteDefaultName ? "basic.sqlite" }:
        basicPreset name (pkgs.writeText compositionName (builtins.toJSON {
          inherit format entry artifacts definitions slots grants excluded
            substitutions authority resource;
        })) sqliteDefaultName
      );
      basicBinary = overridablePreset (basicCompositionSpec // {
        name = "phenix-basic";
        compositionName = "phenix-basic-composition.json";
      });
      basicHttpBinary = overridablePreset (basicHttpCompositionSpec // {
        name = "phenix-basic-http";
        compositionName = "phenix-basic-http-composition.json";
        sqliteDefaultName = "basic-http.sqlite";
      });
      historyVerificationBinary = overridablePreset (historyVerificationCompositionSpec // {
        name = "phenix-basic-verification";
        compositionName = "phenix-basic-history-verification.json";
      });
      advancedBinary = overridablePreset (advancedCompositionSpec // {
        name = "phenix-advanced";
        compositionName = "phenix-advanced-composition.json";
      });
      advancedVerificationBinary = overridablePreset (advancedVerificationCompositionSpec // {
        name = "phenix-advanced-verification";
        compositionName = "phenix-advanced-verification-composition.json";
      });
      basicBinaryClosure = pkgs.closureInfo {
        rootPaths = [ basicBinary ];
      };
      basicHttpProductCheck = pkgs.runCommand "phenix-basic-http-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/basic-http-preset-product.py} ${basicHttpBinary}/bin/phenix-basic-http ${basicHttpComposition} "$out"
        test -s "$out/passed.json"
      '';
      basicLauncherDefaultCheck = pkgs.runCommand "phenix-basic-launcher-default-state"
        { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p "$out"
        python ${./tests/basic-launcher-default-product.py} ${self.packages.${system}.phenix-basic-acp}/bin/phenix-basic-acp "$out"
        test -s "$out/accepted.json"
      '';
      historyVerificationCheck = pkgs.runCommand "phenix-basic-history-verification" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/basic-history-verification-product.py} ${historyVerificationBinary}/bin/phenix-basic-verification ${basicComposition} ${historyVerificationComposition} "$out"
        test -s "$out/history-verification.json"
        python ${./tests/basic-history-verification-acp.py} ${historyVerificationBinary}/bin/phenix-basic-verification "$out"
        test -s "$out/acp-history-verification.json"
        # User-facing Basic must not close over the test-only model package.
        ! grep -Fq "${historyTestModel}" ${basicBinaryClosure}/store-paths
        test -x ${basicBinary}/bin/phenix-basic
      '';
      # Deterministic assistant-text streaming. The synchronous history-test
      # Model still journals an ordered TextDelta before each terminal Answer;
      # an empty intermediate tool reply contributes no streaming event.
      basicStreamingCheck = pkgs.runCommand "phenix-basic-streaming" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/basic-streaming-product.py} ${historyVerificationBinary}/bin/phenix-basic-verification "$out"
        test -s "$out/streaming.json"
      '';
      advancedBinaryClosure = pkgs.closureInfo {
        rootPaths = [ advancedBinary ];
      };
      kernelBinaryClosure = pkgs.closureInfo {
        rootPaths = [ kernel ];
      };
      p2SourceEvidenceRunner = build "phenix-p2-source-evidence" (subset (publicPaths ++ [
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-basic-contracts
        ./crates/phenix-advanced-contracts
        ./crates/phenix-p2-source-evidence
      ]));
      p2SourceEvidenceProductCheck = pkgs.runCommand "phenix-p2-source-evidence-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-source-evidence-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2SourceEvidenceRunner}/bin/phenix-p2-source-evidence \
          ${evidenceJournal} ${eventJournal} ${storageSqlite} \
          ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/p2-source-evidence-product.json"
      '';
      p2CompactionProductRunner = build "phenix-p2-compaction-product" (subset (publicPaths ++ [
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-basic-contracts
        ./crates/phenix-advanced-contracts
        ./crates/phenix-p2-compaction-replay
        ./crates/phenix-p2-compaction-product
      ]));
      p2CompactionProductCheck = pkgs.runCommand "phenix-p2-compaction-checkpoints-product" {
        nativeBuildInputs = [ pkgs.python3 pkgs.sqlite ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-compaction-checkpoints-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2CompactionProductRunner}/bin/phenix-p2-compaction-product \
          ${compactionCheckpointStore} ${eventJournal} ${storageSqlite} \
          ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/p2-compaction-checkpoints-product.json"
      '';
      p2EvidenceStoreRunner = build "phenix-p2-evidence-store" (subset (publicPaths ++ [
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-basic-contracts
        ./crates/phenix-advanced-contracts
        ./crates/phenix-p2-evidence-store
      ]));
      p2EvidenceStoreProductCheck = pkgs.runCommand "phenix-p2-evidence-store-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-evidence-store-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2EvidenceStoreRunner}/bin/phenix-p2-evidence-store \
          ${evidenceStore} ${evidenceJournal} ${eventJournal} ${storageSqlite} \
          ${basicDefinitions} ${advancedDefinitions} ${projectStorageSqlite} "$out"
        test -s "$out/p2-evidence-store-product.json"
      '';
      p2ProjectStorageRunner = build "phenix-p2-project-storage" (subset (publicPaths ++ [
        ./crates/phenix-artifact
        ./crates/phenix-kernel
        ./crates/phenix-basic-contracts
        ./crates/phenix-advanced-contracts
        ./crates/phenix-p2-project-storage
      ]));
      p2ProjectStorageProductCheck = pkgs.runCommand "phenix-p2-project-storage-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-project-storage-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2ProjectStorageRunner}/bin/phenix-p2-project-storage \
          ${projectStorageSqlite} ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/p2-project-storage-product.json"
      '';
      p2ProjectMembershipProductCheck = pkgs.runCommand "phenix-p2-project-membership-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-project-membership-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2ProjectStorageRunner}/bin/phenix-p2-project-storage \
          ${projectStorageSqlite} ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/p2-project-membership-product.json"
      '';
      p2ProjectSourceProductCheck = pkgs.runCommand "phenix-p2-project-source-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-project-source-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2ProjectStorageRunner}/bin/phenix-p2-project-storage \
          ${projectStorageSqlite} ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/p2-project-source-product.json"
      '';
      p2ProjectEvidenceProductCheck = pkgs.runCommand "phenix-p2-project-evidence-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-project-evidence-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${p2ProjectStorageRunner}/bin/phenix-p2-project-storage \
          ${p2EvidenceStoreRunner}/bin/phenix-p2-evidence-store \
          ${evidenceStore} ${evidenceJournal} ${eventJournal} ${storageSqlite} \
          ${projectStorageSqlite} ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/p2-project-evidence-product.json"
      '';
      p2CompositionFoundationCheck = pkgs.runCommand "phenix-p2-composition-foundation" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-composition-foundation.py} \
          ${kernel}/bin/phenix-kernel \
          ${basicBinary}/bin/phenix-basic \
          ${historyVerificationBinary}/bin/phenix-basic-verification \
          ${advancedBinary}/bin/phenix-advanced \
          ${advancedVerificationBinary}/bin/phenix-advanced-verification \
          ${basicComposition} ${historyVerificationComposition} \
          ${advancedComposition} ${advancedVerificationComposition} \
          ${p2CompositionFoundationManifest} "$out"
        test -s "$out/p2-composition-foundation.json"
        # Test model and Basic agent never enter public kernel / Advanced closures.
        ! grep -Fq "${historyTestModel}" ${basicBinaryClosure}/store-paths
        ! grep -Fq "${historyTestModel}" ${advancedBinaryClosure}/store-paths
        ! grep -Fq "${basicAgent}" ${kernelBinaryClosure}/store-paths
        grep -Fq "${historyTestModel}" ${(pkgs.closureInfo {
          rootPaths = [ advancedVerificationBinary ];
        })}/store-paths
      '';
      p2ModelAdapterPendingHttpCheck = pkgs.runCommand "phenix-p2-model-adapter-pending-http" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/basic-pending-http-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime ${advancedComposition} ${httpModel} ${providerDefinitions} "$out" --advanced-adapter
        test -s "$out/pending-basic-http.json"
      '';
      # Only a test composition co-binds Model and Meter to the same fixture.
      # Production Advanced presets retain the original pass-through Model.
      p2MeteredAdapterProductCheck = pkgs.runCommand "phenix-p2-metered-model-adapter" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-metered-model-adapter-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime \
          ${advancedComposition} ${p2ModelMeterFixtureCheck} ${p2SelectedModelSummarizer} \
          ${p2CompactionProductRunner}/bin/phenix-p2-compaction-product \
          ${compactionCheckpointStore} ${eventJournal} ${storageSqlite} \
          ${basicDefinitions} ${advancedDefinitions} "$out"
        test -s "$out/accepted.json"
      '';
      p2ModelAdapterProductCheck = pkgs.runCommand "phenix-p2-model-adapter-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/p2-model-adapter-product.py} \
          ${basicBinary}/bin/phenix-basic \
          ${advancedBinary}/bin/phenix-advanced \
          ${advancedVerificationBinary}/bin/phenix-advanced-verification \
          ${basicComposition} ${advancedComposition} ${advancedVerificationComposition} "$out"
        test -s "$out/p2-model-adapter-product.json"
      '';
      p2PresetOverrides = import ./tests/p2-preset-overrides.nix {
        advanced = advancedBinary;
        inherit historyTestModel;
      };
      p2ExternalPreset = (import ./tests/p2-external-override/flake.nix).outputs {
        self = { };
        phenix = {
          packages.${system} = {
            phenix-advanced = advancedBinary;
            phenix-plugin-model-http = httpModel;
          };
        };
      };
      p2PresetOverridesCheck = pkgs.runCommand "phenix-p2-preset-overrides" { } ''
        mkdir -p "$out"
        # Evaluate the same standalone external flake output used by consumers.
        test -x ${p2ExternalPreset.packages.${system}.default}/bin/phenix-advanced
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/preset-overrides.sqlite"
        printf '%s\n' '{"type":"prompt","session":"s1","text":"remember:color=violet"}' \
          | ${p2PresetOverrides.substituted}/bin/phenix-advanced jsonl > "$out/accepted.jsonl"
        grep -Fq 'stored:color=violet' "$out/accepted.jsonl"
        if printf '%s\n' '{"type":"prompt","session":"s1","text":"echo:hello"}' \
          | ${p2PresetOverrides.missingProvider}/bin/phenix-advanced jsonl > "$out/missing.stdout" 2> "$out/missing.stderr"; then
          echo "missing override provider unexpectedly accepted" >&2
          exit 1
        fi
        if printf '%s\n' '{"type":"prompt","session":"s1","text":"echo:hello"}' \
          | ${p2PresetOverrides.excessiveGrant}/bin/phenix-advanced jsonl > "$out/grant.stdout" 2> "$out/grant.stderr"; then
          echo "excessive override grant unexpectedly accepted" >&2
          exit 1
        fi
      '';
      basicProductCheck = pkgs.runCommand "phenix-basic-host-smoke" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p $out
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-product.db"
        printf 'hello\n' | ${basicRuntime}/bin/phenix-basic-runtime terminal ${basicComposition} > $out/terminal.txt
        grep -F 'answer: mock:hello' $out/terminal.txt
        printf '%s\n' '{"type":"prompt","session":"s1","text":"tools:hello","limits":{"model_turns":2,"tool_calls":2}}' | ${basicRuntime}/bin/phenix-basic-runtime jsonl ${basicComposition} > $out/jsonl.txt
        grep -F 'mock:call-1:HELLO|call-2:5' $out/jsonl.txt
        python ${./tests/observer-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} $out
        test -s "$out/observer-product.json"
        python ${./tests/jsonl-multisession-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} $out
        test -s "$out/jsonl-multisession.json"
        python ${./tests/terminal-sessions-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} $out
        test -s "$out/terminal-sessions.json"
        python ${./tests/acp-stdio-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} $out
        test -s "$out/acp-product.json"
        python ${./tests/acp-provider-management-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${providerManagementComposition} ${basicHttpComposition} $out
        test -s "$out/acp-provider-management.json"
        python ${./tests/acp-native-model-select-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicHttpComposition} ${providerLoopbackManagementComposition} $out/c9-selected
        test -s "$out/c9-selected/native-model-acceptance.json"
        python ${./tests/acp-native-routing-restart-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${basicHttpComposition} $out
        test -s "$out/native-routing-restart.json"
        test -s "$PHENIX_BASIC_SQLITE_PATH"
        # A caller that sets neither the explicit variable nor HOME must still
        # resolve the XDG state default instead of failing session ownership.
        unset PHENIX_BASIC_SQLITE_PATH
        export XDG_STATE_HOME="$TMPDIR/xdg-state"
        printf 'hello\n' | ${basicRuntime}/bin/phenix-basic-runtime terminal ${basicComposition} > $out/terminal-default.txt
        grep -F 'answer: mock:hello' $out/terminal-default.txt
        test -s "$XDG_STATE_HOME/phenix/basic.sqlite"
        echo '{"format":"phenix.basic.host-smoke.v1","terminal":true,"jsonl":true,"default_state_path":true,"acp_v1_session_reload":true,"p1_accepted":false}' > $out/evidence.json
      '';
      # Separate acceptance keeps the credential plugin out of ordinary Basic
      # dependency closures and never stores test secrets in Nix outputs.
      basicCredentialAcpCheck = pkgs.runCommand "phenix-basic-acp-credentials-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/acp-credential-management-product.py} \
          ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} \
          ${providerCredentialStore} ${providerDefinitions} ${providerManagementComposition} "$out"
        test -s "$out/acp-credential-management.json"
      '';
      basicPendingHttpProductCheck = pkgs.runCommand "phenix-basic-pending-http-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p $out
        export PYTHONPATH=${./tests}
        python ${./tests/basic-pending-http-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${httpModel} ${providerDefinitions} $out
        test -s "$out/pending-basic-http.json"
        PHENIX_MODEL_HTTP_PROTOCOL=openai-responses python ${./tests/basic-pending-http-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${httpModel} ${providerDefinitions} $out/responses
        PHENIX_MODEL_HTTP_PROTOCOL=anthropic-messages python ${./tests/basic-pending-http-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${httpModel} ${providerDefinitions} $out/anthropic
        python ${./tests/acp-pending-http-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${httpModel} ${providerDefinitions} $out
        test -s "$out/acp-pending-http.json"
      '';
      scriptedModelCheck = pkgs.runCommand "phenix-b4-scripted" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-b4-scripted ${scriptedModel} ${basicDefinitions} $out
      '';
      basicToolsCheck = pkgs.runCommand "phenix-b3-tools" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-b3-tools ${basicTools} ${basicDefinitions} $out
      '';
      toolContributorsCheck = pkgs.runCommand "phenix-tool-contributors-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p "$out"
        python ${./tests/tool-contributors-product.py} \
          ${kernel}/bin/phenix-kernel ${basicTools} ${toolFixture} ${basicDefinitions} "$out"
        test -s "$out/verified.json"
      '';
      eventJournalCheck = pkgs.runCommand "phenix-b2-journal" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-b2-journal ${eventJournal} ${storageFixture} ${basicDefinitions} $out
      '';
      basicSessionCheck = pkgs.runCommand "phenix-b2-session" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-b2-session ${basicSession} ${storageFixture} ${basicDefinitions} $out
      '';
      graphCheck = pkgs.runCommand "phenix-m1-graph" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-m1-graph ${graphA} ${graphB} ${graphC} ${graphDefinitions} $out
      '';
      authoredProvider = plugin "example-authoring-provider";
      authoredClient = plugin "example-authoring-client";
      authoredField = plugin "example-authoring-field";
      authoredApi = rust.buildRustPackage {
        pname = "example-authoring-api";
        version = "0.1.0";
        src = pluginSource;
        buildAndTestSubdir = "examples";
        cargoRoot = "examples";
        cargoLock.lockFile = ./examples/Cargo.lock;
        cargoBuildFlags = [ "-p" "example-authoring-api" ];
        preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
        doCheck = false;
        installPhase = ''
          test ! -e ../crates/phenix-kernel
          test ! -e ../crates/phenix-artifact
          mkdir -p $out/bin
          cp target/*/release/example-authoring-api $out/bin/
        '';
      };
      authoredDefinitions = definitionPackage "phenix-authored-definitions" "${authoredApi}/bin/example-authoring-api" false;
      authoringCheck = pkgs.runCommand "phenix-m1-native-authoring" { } ''
        ${conformanceBinary}/bin/phenix-m1-authoring ${authoredProvider} ${authoredClient} ${authoredField} ${authoredDefinitions} $out
      '';
      staticSchemaProbe = (build "phenix-contract" publicSource).overrideAttrs (old: {
        cargoBuildFlags = [ "-p" "phenix-contract" "--bin" "phenix-static-schema-probe" ];
      });
      valueSdkCheck = pkgs.runCommand "phenix-m1-value-sdk" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-m1-sdk ${valueProvider} ${valueClient} ${stateNegative "alternative"} $out
        python ${./tests/m1-value-sdk-vectors.py} ${staticSchemaProbe}/bin/phenix-static-schema-probe ${./tests/m1-contract-vectors.json} $out
        python ${./tests/m1-live-state-product.py} ${kernel}/bin/phenix-kernel $out "$TMPDIR/metadata" --metadata-only
        cp "$TMPDIR/metadata/metadata.json" $out/metadata.json
      '';
      valueSdkProduct = pkgs.runCommand "phenix-m1-value-sdk-product" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/m1-value-sdk-product.py} ${kernel}/bin/phenix-kernel ${valueSdkCheck} $out
      '';
      stateProvider = plugin "example-reactive-provider";
      stateClient = plugin "example-reactive-client";
      stateNegative = feature: (plugin "example-reactive-provider").overrideAttrs (old: {
        pname = "example-reactive-provider-${feature}";
        cargoBuildFlags = old.cargoBuildFlags ++ [ "--features" feature ];
      });
      stateCheck = pkgs.runCommand "phenix-m1-live-state" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        ${conformanceBinary}/bin/phenix-m1-state ${stateProvider} ${stateClient} ${stateNegative "alternative"} ${stateNegative "bad-init"} ${stateNegative "bad-schema"} ${stateNegative "bad-table"} $out
        python ${./tests/m1-live-state-product.py} ${kernel}/bin/phenix-kernel $out "$TMPDIR/metadata" --metadata-only
        cp "$TMPDIR/metadata/metadata.json" $out/metadata.json
      '';
      stateProduct = pkgs.runCommand "phenix-m1-live-state-product" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/m1-live-state-product.py} ${kernel}/bin/phenix-kernel ${stateCheck} $out
      '';
      agentPlugin = plugin "example-agent";
      agentModel = plugin "example-agent-model";
      agentAlternate = plugin "example-agent-alternate";
      agentTool = plugin "example-agent-tool";
      agentCheck = pkgs.runCommand "phenix-a0-agent" { } ''
        ${conformanceBinary}/bin/phenix-a0-agent ${agentPlugin} ${agentModel} ${agentAlternate} ${agentTool} $out
      '';
      agentProduct = pkgs.runCommand "phenix-a0-agent-product" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/a0-agent-product.py} ${kernel}/bin/phenix-kernel ${agentCheck} $out
      '';
      agentRequest = pkgs.runCommand "phenix-a0-request" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python - "$out" <<'PYCODE'
        import sys
        def string(s):
            b = s.encode()
            return bytes([5]) + len(b).to_bytes(4, "big") + b
        fields = [("max_turns", bytes([4]) + bytes(8)), ("prompt", string("phenix"))]
        open(sys.argv[1], "wb").write(b"PV\x02\0"+bytes([8]) + len(fields).to_bytes(4, "big") + b"".join(len(k).to_bytes(4, "big") + k.encode() + v for k, v in fields))
        PYCODE
      '';
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
      p0Fat = (individualPlugin "example-provider").overrideAttrs (_: {
        pname = "example-provider-fat";
        CARGO_PROFILE_RELEASE_LTO = "fat";
      });
      p0Off = (individualPlugin "example-provider").overrideAttrs (_: {
        pname = "example-provider-no-lto";
        CARGO_PROFILE_RELEASE_LTO = "off";
      });
      p0Lld = (individualPlugin "example-provider").overrideAttrs (old: {
        pname = "example-provider-lld";
        nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.lld ];
        RUSTFLAGS = "-C link-arg=-fuse-ld=lld -C link-arg=-Wl,--gc-sections";
      });
      p0Repeat = (individualPlugin "example-provider").overrideAttrs (_: {
        pname = "example-provider-repeat";
      });
      descriptorCheck =
        pkgs.runCommand "phenix-m1-metadata-retention"
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
        buildInputs = (old.buildInputs or []) ++ [ pkgs.sqlite pkgs.curl pkgs.zlib ];
        nativeBuildInputs = old.nativeBuildInputs ++ [
          pkgs.pkg-config
          pkgs.rustfmt
          pkgs.python3
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
        installPhase = "mkdir -p $out; echo passed > $out/contract; python tests/m1-architecture.py > $out/architecture.json";
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
          rustc --edition=2021 --target x86_64-unknown-linux-gnu -C lto=thin -C opt-level=3 ${./tests/m1-state-abi.rs} --extern phenix_contract="$contract" -L dependency="$(dirname "$contract")" -L dependency=target/release/deps -C link-arg="$TMPDIR/state.o" -o "$TMPDIR/state-abi"
          "$TMPDIR/state-abi" > $out/native.json
          python -m json.tool $out/metadata.json > /dev/null
          python -m json.tool $out/native.json > /dev/null
        '';
      });
      deployment = pkgs.writeText "phenix-m1-deployment.json" (
        builtins.toJSON {
          format = "phenix.composition.v3";
          artifacts = [
            "${provider}"
            "${consumer}"
          ];
          entry = {
            interface = "example.consumer@1";
            operation = "run";
            provider = "example.consumer";
          };
          definitions = [ ];
          slots = [{ caller = "example.consumer"; slot = "Provider"; index = 0; provider = "example.provider"; export = "Transform"; }];
          excluded = [ ];
          substitutions = [ ];
          resource = null;
          authority = { operations = [ "example.consumer@1/run" "example.transform.apply@1/apply" ]; resources = [ ]; };
          grants = [
            {
              caller = "root";
              resources = [ ];
              interface = "example.consumer@1";
              operation = "run";
            }
            {
              caller = "example.consumer";
              resources = [ ];
              interface = "example.transform.apply@1";
              operation = "apply";
            }
          ];
        }
      );
      request = pkgs.runCommand "phenix-m1-request" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python - "$out" <<'PY'
        import sys
        # Canonical value {"text": "phenix"} from the normative value profile.
        open(sys.argv[1], "wb").write(b"PV\x02\0"+bytes([8])+ (1).to_bytes(4,"big")+(4).to_bytes(4,"big")+b"text"+bytes([5])+(6).to_bytes(4,"big")+b"phenix")
        PY
      '';
      nativeCheck = pkgs.runCommand "phenix-m1-native" { } ''
        ${conformanceBinary}/bin/phenix-conformance ${provider} ${consumer} ${alternate} ${cycle} ${badAbi 1} ${badAbi 2} ${badAbi 3} ${badAbi 4} ${badAbi 5} $out
      '';
      productCheck = pkgs.runCommand "phenix-m1-product" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/product.py} ${kernel}/bin/phenix-kernel ${deployment} ${request} ${./tests/inspection-golden.json} $out
      '';
      compositionCheck = pkgs.runCommand "phenix-m1-composition" { } ''
        ${conformanceBinary}/bin/phenix-m1-composition ${provider} ${alternate} ${choice} ${other} ${restricted} ${relay} ${resources} ${cycle} $out
      '';
      compositionProduct =
        pkgs.runCommand "phenix-m1-composition-product" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            python ${./tests/composition-product.py} ${kernel}/bin/phenix-kernel ${compositionCheck} ${./tests/composition-golden.json} $out
          '';
      lifecycleCheck = pkgs.runCommand "phenix-m1-lifecycle" { } ''
        ${conformanceBinary}/bin/phenix-m1-lifecycle ${lifecycleProvider} ${lifecycleConsumer} $out
      '';
      lifecycleProduct =
        pkgs.runCommand "phenix-m1-lifecycle-product" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            python ${./tests/lifecycle-product.py} ${kernel}/bin/phenix-kernel ${lifecycleCheck} $out
          '';
      pendingCheck =
        pkgs.runCommand "phenix-m1-pending"
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
            ${conformanceBinary}/bin/phenix-m1-pending ${pendingPlugin} ${provider} $out
          '';
      pendingProduct =
        pkgs.runCommand "phenix-m1-pending-product" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            python ${./tests/pending-product.py} ${kernel}/bin/phenix-kernel ${pendingCheck} $out
          '';
      importsCheck = pkgs.runCommand "phenix-m1-imports" { } ''
        ${conformanceBinary}/bin/phenix-m1-imports ${pendingConsumer} ${pendingPlugin} ${provider} ${pendingImmediate} ${plugin "example-pending-security"} $out
      '';
      generationsCheck =
        pkgs.runCommand "phenix-m1-generations" { nativeBuildInputs = [ pkgs.python3 ]; }
          ''
            mkdir -p $out
            python ${./tests/generation-identity.py} ${./tests/generation-identity-v2.json} > $out/identity-vector.json
            ${conformanceBinary}/bin/phenix-m1-generations ${revisionA} ${revisionB} ${importsCheck} $out
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
      shippingCheck = pkgs.runCommand "phenix-m1-shipping" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        python ${./tests/shipping-product.py} ${kernel} ${publicSdkCheck} ${conformance} ${productionClosure} ${exampleSource}/examples $out
      '';
      # P1 is not accepted by this Nix fixture alone: the independent
      # official ACP SDK CI workflow must also pass at the final exact head.
      p1ReleasePreflight = pkgs.runCommand "phenix-p1-release-preflight" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p $out
        python ${./tests/p1-release-evidence.py} --self-test
        python ${./tests/p1-release-evidence.py} ${basicProductCheck} ${basicPendingHttpProductCheck} ${shippingCheck} ${p1AuthLauncherProductCheck} ${p1CombinedAuthProductCheck} $out
        test -s "$out/evidence.json"
      '';
      release = pkgs.runCommand "phenix-m1-release" { } ''
        mkdir -p $out
        cp -rs ${conformance}/. $out/
        ln -s ${publicSdkCheck} $out/public-sdk
        ln -s ${shippingCheck} $out/shipping
        ln -s ${productionClosure} $out/production-closure
      '';
      conformance = pkgs.runCommand "phenix-m1-conformance" { } ''
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
        ln -s ${referenceWireCheck} $out/wire
        ln -s ${metadataStateCheck} $out/metadata-state
        ln -s ${authoringCheck} $out/authoring
        ln -s ${graphCheck} $out/graph
        ln -s ${basicContractsCheck} $out/basic-contracts
        ln -s ${environmentProductCheck} $out/environment
        ln -s ${workspaceProductCheck} $out/workspace
        ln -s ${scriptedModelCheck} $out/scripted-model
        ln -s ${httpModelCheck} $out/http-model
        ln -s ${basicToolsCheck} $out/basic-tools
        ln -s ${eventJournalCheck} $out/event-journal
        ln -s ${basicSessionCheck} $out/basic-session
        ln -s ${sqliteRestartCheck} $out/sqlite-restart
        ln -s ${basicMemoryCheck} $out/basic-memory
        ln -s ${basicAgentCheck} $out/basic-agent
        ln -s ${frontendCheck} $out/frontends
        ln -s ${acpCheck} $out/acp-codec
        ln -s ${basicProductCheck} $out/basic-host-smoke
        ln -s ${basicPendingHttpProductCheck} $out/basic-pending-http-product
        ln -s ${stateCheck} $out/live-state
        ln -s ${stateProduct} $out/live-state-product
        ln -s ${valueSdkCheck} $out/value-sdk
        ln -s ${valueSdkProduct} $out/value-sdk-product
        ln -s ${agentCheck} $out/agent
        ln -s ${agentProduct} $out/agent-product
      '';
      pluginSourceCheck = pkgs.runCommand "phenix-plugin-source-preparation" {
        nativeBuildInputs = [ pkgs.python3 pkgs.cargo pkgs.rustc pkgs.stdenv.cc ];
      } ''
        export PHENIX_SOURCE_TOOL=${./tools/phenix-plugin-sources.py}
        ${pkgs.python3}/bin/python ${./tests/test-plugin-sources.py}
        export HOME="$TMPDIR"
        export PHENIX_PLUGIN_CACHE_DIR="$TMPDIR/plugin-cache"
        export PHENIX_PLUGIN_PREPARER=${pluginSourceTool}/bin/phenix-plugin-sources
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/plugin-sources.sqlite"
        ${pkgs.python3}/bin/python - "${basicComposition}" "${scriptedModel}" "$TMPDIR/source-config.json" <<'PY'
        import json, sys
        original, plugin, destination = sys.argv[1:]
        with open(original) as stream:
            config = json.load(stream)
        assert config["format"] == "phenix.composition.v3"
        config["artifacts"].remove(plugin)
        config["plugin_sources"] = [{"source": plugin + "/lib/plugin.so"}]
        with open(destination, "w") as stream:
            json.dump(config, stream)
        PY
        mkdir -p "$out"
        printf 'hello\n' | ${basicRuntime}/bin/phenix-basic-runtime terminal \
          "$TMPDIR/source-config.json" > "$out/terminal.txt"
        grep -F 'answer: mock:hello' "$out/terminal.txt"
      '';
      # Test definitions refer only to existing immutable packages.
      # All checks are discovered from checks.${system} by the same CI planner.
      declarativeCliChecks = testLib.mkTestCases {
        inherit pkgs;
        namePrefix = "phenix-";
        defaults.command.program = "${kernel}/bin/phenix-kernel";
        cases = {
          kernel-inspect-contract = {
            command.args = [ "inspect" "${basicComposition}" ];
            expect = {
              exitCode = a.eq 0;
              stdout.json."/format" = a.eq "phenix.inspect.v2";
            };
          };
          kernel-invalid-composition = {
            command.args = [ "inspect" "/not-a-phenix-composition.json" ];
            expect.exitCode = a.ne 0;
          };
        };
      };
    in
    {
      packages.${system} = {
        # Runnable test.* packages are discovered by generic CI automatically.
        # They are thin executable wrappers over already-built artifacts:
        # test jobs never compile a second kernel or plugin composition.
        "test.basic-composition-smoke" = pkgs.writeShellApplication {
          name = "test-basic-composition-smoke";
          runtimeInputs = [ pkgs.coreutils pkgs.gnugrep ];
          text = ''
            state_dir="$(mktemp -d)"
            trap 'rm -rf "$state_dir"' EXIT
            export HOME="$state_dir"
            export XDG_STATE_HOME="$state_dir/state"
            printf 'hello\\n' |
              ${basicRuntime}/bin/phenix-basic-runtime terminal ${basicComposition} |
              grep -F 'answer: mock:hello'
          '';
        };
        "test.kernel-composition-inspect" = pkgs.writeShellApplication {
          name = "test-kernel-composition-inspect";
          runtimeInputs = [ pkgs.jq ];
          text = ''
            ${kernel}/bin/phenix-kernel inspect ${basicComposition} |
              jq -e '.format == "phenix.inspect.v2"'
          '';
        };
        default = kernel;
        phenix-kernel = kernel;
        # Shared Cargo output for Nix CI prebuild; plugins remain independent packages.
        phenix-core-example-plugins = coreExamplesBuild;
        phenix-describe = describe;
        phenix-plugin-sources = pluginSourceTool;
        example-provider = provider;
        example-consumer = consumer;
        phenix-m1-conformance = release;
        # Expose the existing runner to the CI matrix without changing its semantics.
        phenix-m1-conformance-runner = conformanceBinary;
        phenix-m1-release = release;
        phenix-p1-release-preflight = p1ReleasePreflight;
        phenix-basic-contracts = basicDefinitions;
        phenix-plugin-provider-builtins = providerBuiltinsPlugin;
        phenix-plugin-providers = providerHostPlugin;
        # Temporary packaging alias; same executable, not a second plugin.
        phenix-plugin-provider-host = providerHostPlugin;
        phenix-plugin-provider-openai = providerOpenAiDeclarationPlugin;
        phenix-plugin-provider-standards = providerStandardDeclarationsPlugin;
        phenix-plugin-provider-openai-api = providerOpenAiApi;
        phenix-plugin-provider-chatgpt = providerChatGpt;
        phenix-plugin-provider-credential-files = providerCredentialStore;
        phenix-plugin-provider-secret-service = providerSecretService;
        phenix-provider-contracts = providerDefinitions;
        phenix-environment-contracts = environmentDefinitions;
        phenix-plugin-environment = environmentPlugin;
        phenix-workspace-contracts = workspaceDefinitions;
        phenix-plugin-workspace-selection = workspacePlugin;
        phenix-plugin-model-scripted = scriptedModel;
        phenix-plugin-model-http = httpModel;
        phenix-plugin-basic-tools = basicTools;
        phenix-plugin-event-journal = eventJournal;
        phenix-plugin-basic-session = basicSession;
        phenix-plugin-storage-sqlite = storageSqlite;
        phenix-plugin-basic-memory = basicMemory;
        phenix-plugin-basic-agent = basicAgent;
        phenix-plugin-frontend-terminal = frontendTerminal;
        phenix-plugin-frontend-jsonl = frontendJsonl;
        phenix-plugin-frontend-acp = frontendAcp;
        phenix-basic-runtime = basicRuntime;
        phenix-basic-composition = basicComposition;
        phenix-basic = basicBinary;
        phenix-basic-http = basicHttpBinary;
        phenix-basic-http-composition = basicHttpComposition;
        phenix-basic-verification-composition = historyVerificationComposition;
        # Explicit opt-in test binary: Basic runtime plus a history-only Model.
        phenix-basic-verification = historyVerificationBinary;
        # P2-0 scaffold executables; release acceptance remains false.
        phenix-advanced = advancedBinary;
        phenix-advanced-contracts = advancedDefinitions;
        phenix-plugin-evidence-journal = evidenceJournal;
        phenix-plugin-evidence-store = evidenceStore;
        phenix-plugin-project-storage-sqlite = projectStorageSqlite;
        phenix-advanced-composition = advancedComposition;
        phenix-advanced-verification = advancedVerificationBinary;
        phenix-advanced-verification-composition = advancedVerificationComposition;
        phenix-p2-composition-foundation-manifest = p2CompositionFoundationManifest;
        phenix-basic-terminal = pkgs.writeShellApplication {
          name = "phenix-basic-terminal";
          text = ''
            ${basicSqliteEnvironment "basic.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime terminal ${basicComposition} "$@"
          '';
        };
        phenix-basic-acp = pkgs.writeShellApplication {
          name = "phenix-basic-acp";
          text = ''
            ${basicSqliteEnvironment "basic.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime acp ${basicComposition} "$@"
          '';
        };
        phenix-basic-managed-acp = pkgs.writeShellApplication {
          name = "phenix-basic-managed-acp";
          text = ''
            ${basicSqliteEnvironment "basic.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime acp ${basicComposition} --provider-management ${providerManagementComposition} "$@"
          '';
        };
        # The executable reads credentials only at invocation. Nothing secret
        # enters the Nix expression or store; OPENAI_API_KEY is an optional
        # compatibility alias for the HTTP plugin's credential variable.
        phenix-basic-http-acp = pkgs.writeShellApplication {
          name = "phenix-basic-http-acp";
          text = ''
            set -eu
            if [ -z "''${PHENIX_MODEL_HTTP_API_KEY:-}" ] && [ -n "''${OPENAI_API_KEY:-}" ]; then
              export PHENIX_MODEL_HTTP_API_KEY="$OPENAI_API_KEY"
            fi
            export PHENIX_MODEL_HTTP_ENDPOINT="''${PHENIX_MODEL_HTTP_ENDPOINT:-https://api.openai.com/v1/chat/completions}"
            export PHENIX_MODEL_HTTP_MODEL="''${PHENIX_MODEL_HTTP_MODEL:-gpt-4.1-mini}"
            ${basicSqliteEnvironment "basic-http.sqlite"}
            exec ${basicHttpBinary}/bin/phenix-basic-http acp "$@"
          '';
        };
        # Explicit opt-in authenticated provider: no key or vault bytes are
        # embedded in the Nix store. The launcher selects one exact profile.
        # A key becomes usable only after :Phenix auth verifies it with the
        # provider and :Phenix model admits the actual selected HTTP binding.
        phenix-basic-openai-api-acp = pkgs.writeShellApplication {
          name = "phenix-basic-openai-api-acp";
          text = ''
            set -eu
            if [ "''${PHENIX_OPENAI_API_PROFILE:-personal}" != "personal" ]; then
              echo "This opt-in composition authorizes only openai/api/personal." >&2
              exit 2
            fi
            # The provider's ApiKeyOptions advertises OPENAI_API_KEY, but
            # deployments may select a different *name*, never a key value.
            # The exact-root credential backend reads that value only when a
            # provider operation has already been authorized by the kernel.
            export PHENIX_CREDENTIAL_API_KEY_ENV_NAME="''${PHENIX_CREDENTIAL_API_KEY_ENV_NAME:-OPENAI_API_KEY}"
            if { [ "''${PHENIX_CREDENTIAL_FILE_CONSENT:-}" != "1" ] ||
                 [ -z "''${PHENIX_CREDENTIAL_FILE_DIRECTORY:-}" ]; } &&
               ! printenv "$PHENIX_CREDENTIAL_API_KEY_ENV_NAME" >/dev/null; then
              echo "Provide an API-key environment variable, or configure a protected 0700 credential directory for masked input." >&2
              echo "Set PHENIX_CREDENTIAL_FILE_DIRECTORY and PHENIX_CREDENTIAL_FILE_CONSENT=1, or supply PHENIX_CREDENTIAL_API_KEY_ENV_NAME (default OPENAI_API_KEY)." >&2
              exit 2
            fi
            export PHENIX_OPENAI_API_PROFILE=personal
            # Disable unrelated legacy model credentials. The selected native
            # HTTP model can receive only the exact authorized resolved key.
            unset PHENIX_MODEL_HTTP_API_KEY
            export PHENIX_MODEL_HTTP_ENDPOINT=http://127.0.0.1:1/not-selected
            export PHENIX_MODEL_HTTP_MODEL=unselected
            ${basicSqliteEnvironment "basic-openai-api.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime acp ${basicHttpComposition} \
              --provider-bundle ${openAiApiProviderBundle} "$@"
          '';
        };
        # OAuth login becomes available to :Phenix auth without any Lua
        # credential or callback parser. The full selected-model integration is
        # a later gate; never advertise it as an API-key entitlement.
        phenix-basic-chatgpt-acp = pkgs.writeShellApplication {
          name = "phenix-basic-chatgpt-acp";
          text = ''
            set -eu
            if [ "''${PHENIX_CREDENTIAL_FILE_CONSENT:-}" != "1" ] ||
               [ -z "''${PHENIX_CREDENTIAL_FILE_DIRECTORY:-}" ]; then
              echo "ChatGPT OAuth requires an opted-in protected credential backend." >&2
              exit 2
            fi
            # This provider stores structured OAuth state under its exact root;
            # the generic API-key environment override must not intercept it.
            unset PHENIX_CREDENTIAL_API_KEY_ENV_NAME
            unset PHENIX_MODEL_HTTP_API_KEY
            export PHENIX_MODEL_HTTP_ENDPOINT=http://127.0.0.1:1/not-selected
            export PHENIX_MODEL_HTTP_MODEL=unselected
            ${basicSqliteEnvironment "basic-chatgpt.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime acp ${basicHttpComposition} \
              --provider-management ${chatGptManagementComposition} \
              --provider-model-resolver ${chatGptResolverComposition} "$@"
          '';
        };
        # P1 combined authentication product: the ACP host selects exactly
        # one provider+connection per method call and checks its existing
        # independent kernel authority. The runtime multi-management support
        # is introduced by #1117; neither plugin can borrow the other's vault.
        phenix-basic-p1-auth-acp = pkgs.writeShellApplication {
          name = "phenix-basic-p1-auth-acp";
          text = ''
            set -eu
            # Invoking the explicitly selected P1 authentication product
            # opts in to its local, permission-protected credential backend.
            # The user never needs to configure storage just to use :Phenix auth.
            # This fallback is NOT encrypted at rest; never put it in the Nix store.
            umask 077
            if [ "''${PHENIX_CREDENTIAL_FILE_CONSENT:-}" = "0" ]; then
              echo "Credential file storage explicitly disabled; P1 authentication cannot start." >&2
              exit 2
            fi
            if [ -z "''${PHENIX_CREDENTIAL_FILE_DIRECTORY:-}" ]; then
              if [ -n "''${XDG_STATE_HOME:-}" ]; then
                case "$XDG_STATE_HOME" in
                  /*) state_home="$XDG_STATE_HOME" ;;
                  *) echo "XDG_STATE_HOME must be an absolute path." >&2; exit 2 ;;
                esac
              else
                case "''${HOME:-}" in
                  /*) state_home="$HOME/.local/state" ;;
                  *) echo "HOME must be an absolute path when XDG_STATE_HOME is unset." >&2; exit 2 ;;
                esac
              fi
              export PHENIX_CREDENTIAL_FILE_DIRECTORY="$state_home/phenix/p1-auth-credentials"
              # A new directory is private; an existing directory keeps its
              # permissions and must pass the backend's ownership/0700 check.
              mkdir -p -- "$state_home/phenix"
              if [ ! -e "$PHENIX_CREDENTIAL_FILE_DIRECTORY" ] &&
                 [ ! -L "$PHENIX_CREDENTIAL_FILE_DIRECTORY" ]; then
                # Concurrent launchers can race on the initial mkdir.
                mkdir -m 700 -- "$PHENIX_CREDENTIAL_FILE_DIRECTORY" ||
                  [ -d "$PHENIX_CREDENTIAL_FILE_DIRECTORY" ]
              fi
            fi
            case "$PHENIX_CREDENTIAL_FILE_DIRECTORY" in
              /*) ;;
              *) echo "Credential directory must be an absolute path." >&2; exit 2 ;;
            esac
            if [ -L "$PHENIX_CREDENTIAL_FILE_DIRECTORY" ]; then
              echo "Credential directory must not be a symbolic link." >&2
              exit 2
            fi
            # The native plugin checks directory ownership and exact 0700
            # permissions again, and checks each stored credential at use.
            export PHENIX_CREDENTIAL_FILE_CONSENT=1
            unset PHENIX_CREDENTIAL_API_KEY_ENV_NAME
            # Keep operator-chosen key variables such as OPENAI_API_KEY intact.
            # The selected API credential source alone decides whether to read it.
            unset PHENIX_MODEL_HTTP_API_KEY
            export PHENIX_MODEL_HTTP_ENDPOINT=http://127.0.0.1:1/not-selected
            export PHENIX_MODEL_HTTP_MODEL=unselected
            ${basicSqliteEnvironment "basic-p1-auth.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime acp ${basicHttpComposition} \
              --provider-management ${openAiApiManagementComposition} \
              --provider-management ${chatGptManagementComposition} \
              --provider-credentials ${openAiApiCredentialComposition} \
              --provider-credentials ${chatGptLogoutComposition} \
              --provider-model-resolver ${openAiApiResolverComposition} \
              --provider-model-resolver ${chatGptResolverComposition} "$@"
          '';
        };
        phenix-basic-jsonl = pkgs.writeShellApplication {
          name = "phenix-basic-jsonl";
          text = ''
            ${basicSqliteEnvironment "basic.sqlite"}
            exec ${basicRuntime}/bin/phenix-basic-runtime jsonl ${basicComposition} "$@"
          '';
        };
        phenix-m1-deployment = deployment;
        phenix-sdk = sdk;
        phenix-m1-value-sdk-evidence = valueSdkCheck;
        phenix-m1-value-sdk-product = valueSdkProduct;
        phenix-m1-live-state-evidence = stateCheck;
        phenix-m1-live-state-product = stateProduct;
        phenix-a0-agent-evidence = agentCheck;
        phenix-a0-agent-product = agentProduct;
        phenix-a0-demo = pkgs.writeShellApplication {
          name = "phenix-a0-demo";
          text = "exec ${kernel}/bin/phenix-kernel invoke ${agentCheck}/composition.json ${agentRequest}";
        };
        phenix-m1-plugins = pkgs.runCommand "phenix-m1-plugins" { } ''
          mkdir -p $out
          ln -s ${provider} $out/provider
          ln -s ${consumer} $out/consumer
        '';
        phenix-m1-demo = pkgs.writeShellApplication {
          name = "phenix-m1-demo";
          text = "exec ${kernel}/bin/phenix-kernel invoke ${deployment} ${request}";
        };
      };
      checks.${system} = {
        m1-native-authoring = authoringCheck;
        plugin-source-preparation = pluginSourceCheck;
        m1-graph = graphCheck;
        basic-contracts = basicContractsCheck;
        environment-product = environmentProductCheck;
        workspace-product = workspaceProductCheck;
        provider-contracts = providerContractsCheck;
        provider-runtime-product = providerRuntimeProductCheck;
        provider-discovery = providerDiscoveryCheck;
        provider-protocols = providerProtocolsCheck;
        provider-credential-files = providerCredentialFilesCheck;
        provider-credential-store = providerCredentialStoreCheck;
        provider-secret-service = providerSecretServiceCheck;
        provider-builtins = providerBuiltinsCheck;
        provider-builtins-plugin = providerBuiltinsPluginCheck;
        provider-population-product = providerPopulationProductCheck;
        provider-openai-declaration = providerOpenAiDeclarationPlugin;
        provider-standard-declarations = providerStandardDeclarationsPlugin;
        provider-host-plugin = providerHostPlugin;
        provider-host-api-key-product = providerHostApiKeyProductCheck;
        provider-openai-api = providerOpenAiApi;
        provider-chatgpt = providerChatGpt;
        p1-auth-launcher-product = p1AuthLauncherProductCheck;
        p1-combined-auth-product = p1CombinedAuthProductCheck;
        provider-openai-api-product = providerOpenAiApiProductCheck;
        provider-openai-api-launcher = providerOpenAiApiLauncherCheck;
        b4-scripted = scriptedModelCheck;
        b4-http = httpModelCheck;
        b3-tools = basicToolsCheck;
        tool-contributors = toolContributorsCheck;
        tool-population-product = toolPopulationProductCheck;
        b2-journal = eventJournalCheck;
        b2-session = basicSessionCheck;
        b2-sqlite = sqliteRestartCheck;
        b3-memory = basicMemoryCheck;
        b5-agent = basicAgentCheck;
        f1-frontends = frontendCheck;
        f2-acp = acpCheck;
        basic-host-smoke = basicProductCheck;
        basic-acp-credentials = basicCredentialAcpCheck;
        basic-history-verification = historyVerificationCheck;
        basic-streaming = basicStreamingCheck;
        basic-http-product = basicHttpProductCheck;
        basic-launcher-default-state = basicLauncherDefaultCheck;
        p2-composition-foundation = p2CompositionFoundationCheck;
        p2-model-adapter-product = p2ModelAdapterProductCheck;
        p2-model-adapter-pending-http = p2ModelAdapterPendingHttpCheck;
        p2-preset-overrides = p2PresetOverridesCheck;
        p2-evidence-contracts = advancedContractsCheck;
        p2-compaction-replay = p2CompactionReplayCheck;
        p2-model-token-meter-fixture = p2ModelMeterFixtureCheck;
        p2-selected-model-summarizer = p2SelectedModelSummarizer;
        p2-basic-staged-prepare = p2BasicStagedPrepareCheck;
        p2-basic-staged-plugin = basicStagedAgent;
        p2-selected-graph-preparer = basicSelectedGraphPrepareCheck;
        p2-graph-preparer-plugin = basicGraphPreparerPlugin;
        p2-noop-preparer-plugin = basicNoopPreparerPlugin;
        p2-model-token-meter-product = p2ModelMeterProductCheck;
        p2-metered-model-adapter-product = p2MeteredAdapterProductCheck;
        p2-compaction-checkpoints = compactionCheckpointStore;
        p2-compaction-checkpoints-product = p2CompactionProductCheck;
        p2-source-evidence-product = p2SourceEvidenceProductCheck;
        p2-evidence-store-product = p2EvidenceStoreProductCheck;
        p2-project-storage-product = p2ProjectStorageProductCheck;
        p2-project-membership-product = p2ProjectMembershipProductCheck;
        p2-project-source-product = p2ProjectSourceProductCheck;
        p2-project-evidence-product = p2ProjectEvidenceProductCheck;
        basic-pending-http-product = basicPendingHttpProductCheck;
        p1-release-preflight = p1ReleasePreflight;
        m1-value-sdk = valueSdkCheck;
        m1-value-sdk-product = valueSdkProduct;
        m1-live-state = stateCheck;
        m1-live-state-product = stateProduct;
        a0-agent = agentCheck;
        a0-agent-product = agentProduct;
        m1-reference-wire = referenceWireCheck;
        m1-metadata-state = metadataStateCheck;
        m1-metadata-retention = descriptorCheck;
        m1-contract = contractCheck;
        m1-native = nativeCheck;
        m1-product = productCheck;
        m1-composition = compositionCheck;
        m1-composition-product = compositionProduct;
        m1-public-sdk = publicSdkCheck;
        m1-shipping = shippingCheck;
        m1-generations = generationsCheck;
        m1-imports = importsCheck;
        m1-pending = pendingCheck;
        m1-pending-product = pendingProduct;
        m1-lifecycle = lifecycleCheck;
        m1-lifecycle-product = lifecycleProduct;
      } // declarativeCliChecks;
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
