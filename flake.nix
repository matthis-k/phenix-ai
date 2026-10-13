{
  description = "Phenix M1 typed contract kernel";
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
      # Compatible example cdylibs share one Cargo compilation. Each package
      # below still has its own descriptor and native package metadata. Keep
      # linker/profile variants on independent Rust derivations.
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
        assert len(report["definitions"]) == 11
        assert report["plugin"] == "phenix.basic-contracts"
        (Path(sys.argv[2]) / "evidence.json").write_text(json.dumps(report, indent=2, sort_keys=True))
        PY
      '';
      # Keep independent source closures for feature-variant and system-linked
      # plugins. Compatible, default-feature plugins compile in small cohesive
      # Cargo groups, so changes do not invalidate the entire Rust workspace.
      basicPluginBatches = [
        {
          id = "agent";
          names = [
            "phenix-plugin-model-scripted"
            "phenix-plugin-basic-tools"
            "phenix-plugin-basic-agent"
          ];
          paths = [
            ./crates/phenix-plugin-model-scripted
            ./crates/phenix-plugin-basic-tools
            ./crates/phenix-plugin-basic-agent
          ];
        }
        {
          id = "state";
          names = [
            "phenix-plugin-event-journal"
            "phenix-plugin-basic-session"
            "phenix-plugin-basic-memory"
          ];
          paths = [
            ./crates/phenix-plugin-event-journal
            ./crates/phenix-plugin-basic-session
            ./crates/phenix-plugin-basic-memory
          ];
        }
      ];
      basicPluginBatch = group:
        rust.buildRustPackage {
          pname = "phenix-basic-${group.id}-plugins";
          version = "0.1.0";
          src = subset (publicPaths ++ [ ./crates/phenix-basic-contracts ] ++ group.paths);
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = pkgs.lib.concatMap (name: [ "-p" name ]) group.names;
          preBuild = "export CARGO_HOME=$TMPDIR/cargo-home";
          doCheck = false;
          installPhase = ''
            mkdir -p "$out/lib"
            ${pkgs.lib.concatMapStringsSep "\n" (name: ''
              cp target/*/release/lib${builtins.replaceStrings [ "-" ] [ "_" ] name}.so "$out/lib/"
            '') group.names}
          '';
        };
      basicAgentBatch = basicPluginBatch (builtins.elemAt basicPluginBatches 0);
      basicStateBatch = basicPluginBatch (builtins.elemAt basicPluginBatches 1);
      basicBatchFor = name:
        let
          matches = builtins.filter (g: builtins.elem name g.names) basicPluginBatches;
        in
        if matches == [] then null
        else if (builtins.head matches).id == "agent" then basicAgentBatch
        else basicStateBatch;
      # The output contract is identical to independently built native plugins.
      # runCommand's script must generate the descriptor and package metadata;
      # unlike buildRustPackage, it does not run postFixup.
      basicNativePlugin = name: library: pluginPath:
        let batch = basicBatchFor name;
        in
        if batch != null then
          pkgs.runCommand "${name}-native-package" { } ''
            mkdir -p "$out/lib"
            cp ${batch}/lib/lib${library}.so "$out/lib/plugin.so"
            ${describe}/bin/phenix-describe "$out/lib/plugin.so" "$out/descriptor.bin" > digests.json
            ${pkgs.python3}/bin/python ${recordWriter} "$out"
            test -s "$out/lib/plugin.so"
            test -s "$out/descriptor.bin"
            test -s "$out/package.json"
          ''
        else
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
      scriptedModel = basicNativePlugin
        "phenix-plugin-model-scripted"
        "phenix_plugin_model_scripted"
        ./crates/phenix-plugin-model-scripted;
      httpModel = (basicNativePlugin
        "phenix-plugin-model-http"
        "phenix_plugin_model_http"
        ./crates/phenix-plugin-model-http).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or []) ++ [ pkgs.pkg-config ];
          buildInputs = (old.buildInputs or []) ++ [ pkgs.curl pkgs.zlib ];
        });
      httpModelCheck = pkgs.runCommand "phenix-b4-http" {
        nativeBuildInputs = [ pkgs.python3 pkgs.openssl ];
      } ''
        mkdir -p $out
        python ${./tests/http-model.py} ${conformanceBinary}/bin/phenix-b4-http ${httpModel} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
        test -s "$out/http-model.json"
      '';
      basicTools = basicNativePlugin
        "phenix-plugin-basic-tools"
        "phenix_plugin_basic_tools"
        ./crates/phenix-plugin-basic-tools;
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
          buildInputs = (old.buildInputs or []) ++ [ pkgs.sqlite ];
        });
      sqliteRestartCheck = pkgs.runCommand "phenix-b2-sqlite" { } ''
        mkdir -p $out
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-sessions.db"
        ${conformanceBinary}/bin/phenix-b2-sqlite seed ${basicSession} ${eventJournal} ${storageSqlite} ${basicDefinitions} $out
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
        ./crates/phenix-basic-runtime
        ./tests/generation-identity-v2.json
      ];
      basicRuntime = build "phenix-basic-runtime" basicRuntimeSource;
      basicGrantSpecs = [
        { caller = "root"; key = "phenix.basic.input@1/submit"; }
        { caller = "root"; key = "phenix.basic.output@1/read"; }
        { caller = "root"; key = "phenix.basic.output@1/observer"; }
        { caller = "root"; key = "phenix.basic.output@1/checkpoint_observer"; }
        { caller = "root"; key = "phenix.basic.frontend@1/decode"; }
        { caller = "root"; key = "phenix.basic.frontend@1/render"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.input@1/submit"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/append"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/read"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/observer"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.journal@1/checkpoint_observer"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.model@1/complete"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.catalog@1/list"; }
        { caller = "phenix.basic.agent"; key = "phenix.basic.tool@1/execute"; }
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
      basicComposition = pkgs.writeText "phenix-basic-composition.json" (builtins.toJSON {
        format = "phenix.composition.v2";
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
      });
      basicProductCheck = pkgs.runCommand "phenix-basic-host-smoke" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        mkdir -p $out
        export PHENIX_BASIC_SQLITE_PATH="$TMPDIR/basic-product.db"
        printf 'hello\n' | ${basicRuntime}/bin/phenix-basic-runtime terminal ${basicComposition} > $out/terminal.txt
        grep -F 'answer: mock:hello' $out/terminal.txt
        printf '%s\n' '{"type":"prompt","session":"s1","text":"tools:hello","limits":{"model_turns":2,"tool_calls":2}}' | ${basicRuntime}/bin/phenix-basic-runtime jsonl ${basicComposition} > $out/jsonl.txt
        grep -F 'mock:call-1:HELLO|call-2:5' $out/jsonl.txt
        python ${./tests/observer-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} $out
        test -s "$out/observer-product.json"
        python ${./tests/acp-stdio-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} $out
        test -s "$out/acp-product.json"
        test -s "$PHENIX_BASIC_SQLITE_PATH"
        echo '{"format":"phenix.basic.host-smoke.v1","terminal":true,"jsonl":true,"acp_v1_session_reload":true,"p1_accepted":false}' > $out/evidence.json
      '';
      basicPendingHttpProductCheck = pkgs.runCommand "phenix-basic-pending-http-product" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        mkdir -p $out
        python ${./tests/basic-pending-http-product.py} ${basicRuntime}/bin/phenix-basic-runtime ${basicComposition} ${httpModel} $out
        test -s "$out/pending-basic-http.json"
      '';
      scriptedModelCheck = pkgs.runCommand "phenix-b4-scripted" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-b4-scripted ${scriptedModel} ${basicDefinitions} $out
      '';
      basicToolsCheck = pkgs.runCommand "phenix-b3-tools" { } ''
        mkdir -p $out
        ${conformanceBinary}/bin/phenix-b3-tools ${basicTools} ${basicDefinitions} $out
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
          format = "phenix.composition.v2";
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
    in
    {
      packages.${system} = {
        default = kernel;
        phenix-kernel = kernel;
        # Shared Cargo output for Nix CI prebuild; plugins remain independent packages.
        phenix-core-example-plugins = coreExamplesBuild;
        phenix-basic-agent-batch = basicAgentBatch;
        phenix-basic-state-batch = basicStateBatch;
        phenix-describe = describe;
        example-provider = provider;
        example-consumer = consumer;
        phenix-m1-conformance = release;
        # Expose the existing runner to the CI matrix without changing its semantics.
        phenix-m1-conformance-runner = conformanceBinary;
        phenix-m1-release = release;
        phenix-basic-contracts = basicDefinitions;
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
        phenix-basic-terminal = pkgs.writeShellApplication {
          name = "phenix-basic-terminal";
          text = "exec ${basicRuntime}/bin/phenix-basic-runtime terminal ${basicComposition}";
        };
        phenix-basic-acp = pkgs.writeShellApplication {
          name = "phenix-basic-acp";
          text = "exec ${basicRuntime}/bin/phenix-basic-runtime acp ${basicComposition}";
        };
        phenix-basic-jsonl = pkgs.writeShellApplication {
          name = "phenix-basic-jsonl";
          text = "exec ${basicRuntime}/bin/phenix-basic-runtime jsonl ${basicComposition}";
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
        m1-graph = graphCheck;
        basic-contracts = basicContractsCheck;
        b4-scripted = scriptedModelCheck;
        b4-http = httpModelCheck;
        b3-tools = basicToolsCheck;
        b2-journal = eventJournalCheck;
        b2-session = basicSessionCheck;
        b2-sqlite = sqliteRestartCheck;
        b3-memory = basicMemoryCheck;
        b5-agent = basicAgentCheck;
        f1-frontends = frontendCheck;
        f2-acp = acpCheck;
        basic-host-smoke = basicProductCheck;
        basic-pending-http-product = basicPendingHttpProductCheck;
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
