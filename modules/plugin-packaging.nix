{ self, ... }:
let
  mkPhenixPlugin =
    {
      pkgs,
      name,
      manifest,
      package ? null,
      resources ? null,
    }:
    let
      execution = manifest.execution or null;
      executionKind = if builtins.isAttrs execution then execution.kind or null else null;
      isEmbedded = executionKind == "embedded";
      isRuntime = executionKind == "runtime";
      isResourceOnly = executionKind == "resource_only";
      metadataDirectory = if isEmbedded then "share/phenix-plugins/${name}" else "share/phenix-plugin";
    in
    assert isEmbedded || isRuntime || isResourceOnly;
    assert (!isEmbedded) || package != null;
    assert (!isRuntime) || package == null;
    assert (!isResourceOnly) || package == null;
    pkgs.runCommand "phenix-plugin-${name}"
      {
        nativeBuildInputs = [ pkgs.jq ];
        passAsFile = [ "manifestJson" ];
        manifestJson = builtins.toJSON manifest;
        passthru = {
          phenixPluginId = manifest.id;
          phenixPluginExecution = executionKind;
        };
      }
      ''
        set -euo pipefail
        mkdir -p "$out/${metadataDirectory}"
        jq -e 'type == "object" and (.id | type == "string" and length > 0)' \
          "$manifestJsonPath" >/dev/null
        cp "$manifestJsonPath" "$out/${metadataDirectory}/manifest.json"

        ${pkgs.lib.optionalString isEmbedded ''
          ln -s "${package}" "$out/${metadataDirectory}/embedded-package"
        ''}
        ${pkgs.lib.optionalString (resources != null) ''
          test -e "${resources}"
          ln -s "${resources}" "$out/share/phenix-plugin/resources"
        ''}
      '';

  mkPhenixWithBase =
    {
      pkgs,
      base,
      runtimeOnly ? false,
      plugins ? [ ],
      resources ? [ ],
      enabledPlugins ? null,
      layerPolicies ? [ ],
      settings ? { },
      configDirectory ? null,
      configFile ? null,
      settingsPrecedence ? "nix",
      ...
    }:
    let
      isEmbedded = plugin: (plugin.phenixPluginExecution or null) == "embedded";
      embeddedPlugins = builtins.filter isEmbedded plugins;
      packagedPlugins = builtins.filter (plugin: !isEmbedded plugin) plugins;
      selectedEmbeddedIds = map (plugin: plugin.phenixPluginId) embeddedPlugins;
      nixSettingsFile = pkgs.writeText "phenix-nix-settings.json" (builtins.toJSON settings);
      validSettingsPrecedence = builtins.elem settingsPrecedence [
        "nix"
        "file"
      ];
      selectedIds =
        if enabledPlugins != null then
          enabledPlugins
        else if embeddedPlugins != [ ] then
          selectedEmbeddedIds
        else
          null;
    in
    if !validSettingsPrecedence then
      throw "mkPhenix settingsPrecedence must be either 'nix' or 'file'"
    else if
      plugins == [ ]
      && resources == [ ]
      && selectedIds == null
      && layerPolicies == [ ]
      && settings == { }
      && configDirectory == null
      && configFile == null
      && settingsPrecedence == "nix"
    then
      base
    else
      pkgs.symlinkJoin {
        name = if runtimeOnly then "phenix-runtime-composed" else "phenix-composed";
        paths = [ base ] ++ plugins ++ resources;
        nativeBuildInputs = [ pkgs.makeWrapper ];
        postBuild =
          if runtimeOnly then
            ""
          else
            let
              pluginPackages = pkgs.lib.concatStringsSep ":" (map toString packagedPlugins);
              enabledPluginIds = if selectedIds == null then null else pkgs.lib.concatStringsSep "," selectedIds;
              layerPolicyJson = builtins.toJSON layerPolicies;
            in
            ''
              for program in phenix phenix-harness; do
                if [ -e "$out/bin/$program" ]; then
                  ${pkgs.lib.optionalString (resources != [ ]) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_DEFAULT_CONFIG_DIR "$out/share/phenix"
                  ''}
                  ${pkgs.lib.optionalString (configDirectory != null) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_CONFIG_DIR ${pkgs.lib.escapeShellArg (toString configDirectory)}
                  ''}
                  ${pkgs.lib.optionalString (configDirectory == null && resources != [ ]) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_CONFIG_DIR "$out/share/phenix"
                  ''}
                  ${pkgs.lib.optionalString (configFile != null) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_CONFIG_FILE ${pkgs.lib.escapeShellArg (toString configFile)}
                  ''}
                  ${pkgs.lib.optionalString (settings != { }) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_NIX_SETTINGS ${pkgs.lib.escapeShellArg (toString nixSettingsFile)}
                  ''}
                  ${pkgs.lib.optionalString (configDirectory != null || resources != [ ] || settings != { }) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_SETTINGS_PRECEDENCE ${pkgs.lib.escapeShellArg settingsPrecedence}
                  ''}
                  ${pkgs.lib.optionalString (packagedPlugins != [ ]) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_PLUGIN_PACKAGES ${pkgs.lib.escapeShellArg pluginPackages}
                  ''}
                  ${pkgs.lib.optionalString (selectedIds != null) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_ENABLED_PLUGINS ${pkgs.lib.escapeShellArg enabledPluginIds}
                  ''}
                  ${pkgs.lib.optionalString (layerPolicies != [ ]) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_LAYER_POLICY ${pkgs.lib.escapeShellArg layerPolicyJson}
                  ''}
                  wrapProgram "$out/bin/$program" \
                    --prefix PATH : ${pkgs.lib.escapeShellArg (pkgs.lib.makeBinPath [ pkgs.bubblewrap ])}
                fi
              done
            '';
      };

  mkPhenix =
    args@{
      pkgs,
      runtimeOnly ? false,
      ...
    }:
    let
      base =
        if runtimeOnly then
          self.packages.${pkgs.system}.phenix-runtime
        else
          self.packages.${pkgs.system}.phenix-harness-runtime;
    in
    mkPhenixWithBase (args // { inherit base; });
in
{
  flake = {
    lib = {
      inherit mkPhenix mkPhenixPlugin;
    };
    wrappers.phenix.wrap = mkPhenix;
  };

  perSystem =
    { pkgs, ... }:
    let
      fixtureResources = pkgs.writeTextDir "README.txt" "resource plugin fixture";
      resourcePlugin = mkPhenixPlugin {
        inherit pkgs;
        name = "resource-fixture";
        manifest = {
          id = "fixture.resources";
          version = 1;
          execution.kind = "resource_only";
          dependencies = [ ];
          services = [ ];
          resource_namespaces = [ ];
          maximum_authority = [ ];
        };
        resources = fixtureResources;
      };
      harnessResources = self.packages.${pkgs.system}.phenix-harness-resources;
      basicComposition = mkPhenix {
        inherit pkgs;
        enabledPlugins = [ "phenix.product.basic" ];
        resources = [ harnessResources ];
      };
      fullComposition = mkPhenix {
        inherit pkgs;
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
      };
      defaultComposition = fullComposition;
      settingsConfigDirectory = pkgs.writeTextDir "settings.json" (
        builtins.toJSON {
          global = {
            "session.auto_create" = true;
          };
          agents = {
            "agent.scout" = {
              "agent.max_parallel_tasks" = 7;
            };
          };
        }
      );
      settingsComposition = mkPhenix {
        inherit pkgs;
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
        configDirectory = settingsConfigDirectory;
        settings = {
          global = {
            "session.auto_create" = false;
          };
          agents = {
            "agent.scout" = {
              "agent.max_parallel_tasks" = 4;
            };
          };
        };
      };
      filePrecedenceComposition = mkPhenix {
        inherit pkgs;
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
        configDirectory = settingsConfigDirectory;
        settingsPrecedence = "file";
        settings = {
          global = {
            "session.auto_create" = false;
          };
        };
      };
      resourceComposition = mkPhenix {
        inherit pkgs;
        plugins = [ resourcePlugin ];
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
      };
      runtimeComposition = mkPhenix {
        inherit pkgs;
        runtimeOnly = true;
      };
      sessionOnlyComposition = mkPhenix {
        inherit pkgs;
        plugins = [ self.phenixPlugins.${pkgs.system}.sessions ];
      };
      contextOnlyComposition = mkPhenix {
        inherit pkgs;
        plugins = [ self.phenixPlugins.${pkgs.system}.context ];
      };
      adapterOnlyComposition = mkPhenix {
        inherit pkgs;
        plugins = [ self.phenixPlugins.${pkgs.system}.adapter-acp ];
      };

      fixtureHarnessProgram = pkgs.writeShellScriptBin "phenix-harness" ''
        exec ${pkgs.jq}/bin/jq -cn \
          --arg default_config "''${PHENIX_DEFAULT_CONFIG_DIR:-}" \
          --arg config "''${PHENIX_CONFIG_DIR:-}" \
          --arg composition_file "''${PHENIX_CONFIG_FILE:-}" \
          --arg settings "''${PHENIX_NIX_SETTINGS:-}" \
          --arg settings_precedence "''${PHENIX_SETTINGS_PRECEDENCE:-}" \
          --arg plugin_packages "''${PHENIX_PLUGIN_PACKAGES:-}" \
          --arg enabled_plugins "''${PHENIX_ENABLED_PLUGINS:-}" \
          --arg layer_policy "''${PHENIX_LAYER_POLICY:-}" \
          --arg path "''${PATH:-}" \
          '{
            default_config: $default_config,
            config: $config,
            settings: $settings,
            composition_file: $composition_file,
            settings_precedence: $settings_precedence,
            plugin_packages: $plugin_packages,
            enabled_plugins: $enabled_plugins,
            layer_policy: $layer_policy,
            path: $path
          }'
      '';
      fixtureBase = pkgs.runCommand "phenix-composition-fixture-base" { } ''
        mkdir -p "$out/bin"
        ln -s ${fixtureHarnessProgram}/bin/phenix-harness "$out/bin/phenix-harness"
        ln -s phenix-harness "$out/bin/phenix"
      '';
      fixtureRuntimeBase = pkgs.writeShellScriptBin "phenix-runtime" ''
        exit 0
      '';
      mkFixturePhenix =
        args:
        mkPhenixWithBase (
          args
          // {
            inherit pkgs;
            base = fixtureBase;
          }
        );

      fullFixtureComposition = mkFixturePhenix {
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
      };
      basicFixtureComposition = mkFixturePhenix {
        enabledPlugins = [ "phenix.product.basic" ];
        resources = [ harnessResources ];
      };
      portableCompositionFile = pkgs.writeText "phenix-composition.json" (
        builtins.toJSON {
          profile = "phenix.product.basic";
          plugins.enable = [ "phenix.debug" ];
          providers.bind."fixture.memory@1" = "fixture.memory.external";
        }
      );
      portableConfigFixtureComposition = mkFixturePhenix {
        configFile = portableCompositionFile;
      };
      settingsFixtureComposition = mkFixturePhenix {
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
        configDirectory = settingsConfigDirectory;
        settings = {
          global = {
            "session.auto_create" = false;
          };
          agents = {
            "agent.scout" = {
              "agent.max_parallel_tasks" = 4;
            };
          };
        };
      };
      filePrecedenceFixtureComposition = mkFixturePhenix {
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
        configDirectory = settingsConfigDirectory;
        settingsPrecedence = "file";
        settings = {
          global = {
            "session.auto_create" = false;
          };
        };
      };
      resourceFixtureComposition = mkFixturePhenix {
        plugins = [ resourcePlugin ];
        enabledPlugins = [ "phenix.product.full" ];
        resources = [ harnessResources ];
      };
      adapterFixtureComposition = mkFixturePhenix {
        plugins = [ self.phenixPlugins.${pkgs.system}.adapter-acp ];
      };
      runtimeFixtureComposition = mkPhenixWithBase {
        inherit pkgs;
        base = fixtureRuntimeBase;
        runtimeOnly = true;
      };

      pluginPackagingWrapper =
        pkgs.runCommand "phenix-plugin-packaging-wrapper-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euo pipefail

            test -x "${fullFixtureComposition}/bin/phenix"
            test -x "${fullFixtureComposition}/bin/phenix-harness"
            test -f "${fullFixtureComposition}/share/phenix/runtime.json"

            "${fullFixtureComposition}/bin/phenix" > "$TMPDIR/full.json"
            jq -e '
              .enabled_plugins == "phenix.product.full"
              and (.default_config | length > 0)
              and (.config | length > 0)
              and (.path | contains("${pkgs.bubblewrap}/bin"))
            ' "$TMPDIR/full.json" >/dev/null

            "${basicFixtureComposition}/bin/phenix" > "$TMPDIR/basic.json"
            jq -e '.enabled_plugins == "phenix.product.basic"' "$TMPDIR/basic.json" >/dev/null

            "${portableConfigFixtureComposition}/bin/phenix" > "$TMPDIR/portable.json"
            portable_path="$(jq -r '.composition_file' "$TMPDIR/portable.json")"
            test "$portable_path" = "${portableCompositionFile}"
            jq -e '.profile == "phenix.product.basic"
              and .plugins.enable == ["phenix.debug"]
              and .providers.bind["fixture.memory@1"] == "fixture.memory.external"' "$portable_path" >/dev/null

            "${settingsFixtureComposition}/bin/phenix" > "$TMPDIR/settings.json"
            jq -e '
              .settings_precedence == "nix"
              and (.settings | length > 0)
              and (.config | length > 0)
            ' "$TMPDIR/settings.json" >/dev/null
            settings_path="$(jq -r '.settings' "$TMPDIR/settings.json")"
            jq -e '
              .global["session.auto_create"] == false
              and .agents["agent.scout"]["agent.max_parallel_tasks"] == 4
            ' "$settings_path" >/dev/null

            "${filePrecedenceFixtureComposition}/bin/phenix" > "$TMPDIR/settings-file-first.json"
            jq -e '.settings_precedence == "file" and (.config | length > 0)'               "$TMPDIR/settings-file-first.json" >/dev/null
            config_path="$(jq -r '.config' "$TMPDIR/settings-file-first.json")/settings.json"
            jq -e '.global["session.auto_create"] == true' "$config_path" >/dev/null

            test -e "${resourceFixtureComposition}/share/phenix-plugin/resources/README.txt"
            "${resourceFixtureComposition}/bin/phenix" > "$TMPDIR/resource.json"
            jq -e '(.plugin_packages | length > 0) and .enabled_plugins == "phenix.product.full"'               "$TMPDIR/resource.json" >/dev/null

            "${adapterFixtureComposition}/bin/phenix" > "$TMPDIR/adapter.json"
            jq -e '.enabled_plugins == "phenix.adapter.acp"' "$TMPDIR/adapter.json" >/dev/null

            test -x "${runtimeFixtureComposition}/bin/phenix-runtime"
            test ! -e "${runtimeFixtureComposition}/bin/phenix"
            test ! -e "${runtimeFixtureComposition}/bin/phenix-harness"

            touch "$out"
          '';

      pluginPackagingProducts =
        pkgs.runCommand "phenix-plugin-packaging-products-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euxo pipefail
            test -x "${defaultComposition}/bin/phenix"
            test -x "${defaultComposition}/bin/phenix-harness"
            test -f "${defaultComposition}/share/phenix/runtime.json"

            export PHENIX_STATE_DB="$TMPDIR/composition.sqlite"
            "${fullComposition}/bin/phenix" --mode=jsonl --list-services > "$TMPDIR/full-services.json"
            jq -e '
              (.plugins | index("phenix.product.full") != null)
              and (.plugins | index("phenix.agent.advanced") != null)
              and (.plugins | index("phenix.agent.basic") != null)
              and (.plugins | index("phenix.providers") != null)
              and (.plugins | index("openai-api") != null)
              and (.plugins | index("openai-codex") != null)
              and (.plugins | index("phenix.workspace") != null)
              and (.services | index("phenix.sessions@1") != null)
              and (.services | index("phenix.models.routing@1") != null)
            ' "$TMPDIR/full-services.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/basic.sqlite"
            "${basicComposition}/bin/phenix" --mode=jsonl --list-services > "$TMPDIR/basic-services.json"
            jq -e '
              (.plugins | index("phenix.product.basic") != null)
              and (.plugins | index("phenix.agent.basic") != null)
              and (.plugins | index("phenix.agent.advanced") == null)
              and (.plugins | index("phenix.api") != null)
              and (.plugins | index("phenix.options") != null)
              and (.plugins | index("phenix.providers") != null)
              and (.plugins | index("openai-api") != null)
              and (.plugins | index("openai-codex") != null)
              and (.plugins | index("phenix.sessions") != null)
              and (.plugins | index("phenix.memory") == null)
              and (.plugins | index("phenix.planning") == null)
              and (.plugins | index("phenix.workspace") == null)
            ' "$TMPDIR/basic-services.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/resource.sqlite"
            test -e "${resourceComposition}/share/phenix-plugin/resources/README.txt"
            "${resourceComposition}/bin/phenix" --list-services > "$TMPDIR/resource-services.json"
            jq -e '(.plugins | index("fixture.resources")) != null' "$TMPDIR/resource-services.json" >/dev/null
            touch "$out"
          '';

      pluginPackagingEnvironment =
        pkgs.runCommand "phenix-plugin-packaging-environment-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euxo pipefail
            for policy in working-dir workdir-write; do
              export PHENIX_STATE_DB="$TMPDIR/environment-$policy.sqlite"
              PHENIX_LOCAL_FILESYSTEM_POLICY="$policy" \
                "${defaultComposition}/bin/phenix" --list-services \
                > "$TMPDIR/environment-$policy.json"
              jq -e '(.plugins | index("phenix.environment.local")) != null' \
                "$TMPDIR/environment-$policy.json" >/dev/null
            done
            touch "$out"
          '';

      pluginPackagingSettings =
        pkgs.runCommand "phenix-plugin-packaging-settings-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euxo pipefail
            export PHENIX_STATE_DB="$TMPDIR/settings.sqlite"
            printf '%s\n' '{"id":1,"service":"phenix.api.sessions@1","input":{"type":"variant","value":{"tag":"Open","value":{"type":"table","value":{"id":{"type":"string","value":"settings-nix-disabled"},"agent":{"type":"option","value":null}}}}}}' \
              | "${settingsComposition}/bin/phenix" > "$TMPDIR/settings-session.json"
            jq -e '.status == "error" and (.error | contains("auto-create is disabled"))' "$TMPDIR/settings-session.json" >/dev/null
            printf '%s\n' '{"id":2,"service":"phenix.api.config@1","input":{"type":"variant","value":{"tag":"Read","value":{"type":"table","value":{"path":{"type":"string","value":"settings.json"}}}}}}' \
              | "${settingsComposition}/bin/phenix" > "$TMPDIR/settings-config.json"
            jq -e '.status == "ok" and .output.type == "variant" and .output.value.tag == "File" and ((.output.value.value.value.content.value | implode | fromjson).global["session.auto_create"] == true)' \
              "$TMPDIR/settings-config.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/settings-file-first.sqlite"
            printf '%s\n' '{"id":1,"service":"phenix.api.sessions@1","input":{"type":"variant","value":{"tag":"Open","value":{"type":"table","value":{"id":{"type":"string","value":"settings-file-created"},"agent":{"type":"option","value":null}}}}}}' \
              | "${filePrecedenceComposition}/bin/phenix" > "$TMPDIR/settings-file-first.json"
            jq -e '.status == "ok" and .output.type == "variant" and .output.value.tag == "Opened" and .output.value.value.value.created.type == "bool" and .output.value.value.value.created.value == true' "$TMPDIR/settings-file-first.json" >/dev/null
            touch "$out"
          '';

      pluginPackagingIsolation =
        pkgs.runCommand "phenix-plugin-packaging-isolation-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euxo pipefail
            export PHENIX_STATE_DB="$TMPDIR/session-only.sqlite"
            "${sessionOnlyComposition}/bin/phenix" --list-services > "$TMPDIR/session-only.json"
            jq -e '(.plugins == ["phenix.sessions"]) and (.services | index("phenix.sessions@1") != null) and (.services | index("phenix.context@1") == null)' "$TMPDIR/session-only.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/context-only.sqlite"
            "${contextOnlyComposition}/bin/phenix" --list-services > "$TMPDIR/context-only.json"
            jq -e '((.plugins | sort) == ["phenix.context", "phenix.execution"]) and (.services | index("phenix.context@1") != null) and (.services | index("phenix.sessions@1") == null)' "$TMPDIR/context-only.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/adapter-only.sqlite"
            "${adapterOnlyComposition}/bin/phenix" --list-services > "$TMPDIR/adapter-only.json"
            jq -e '(.plugins == ["phenix.adapter.acp"]) and (.services == [])' "$TMPDIR/adapter-only.json" >/dev/null

            test -x "${runtimeComposition}/bin/phenix-runtime"
            test ! -e "${runtimeComposition}/bin/phenix"
            test ! -e "${runtimeComposition}/bin/phenix-harness"
            "${runtimeComposition}/bin/phenix-runtime" --list-services > "$TMPDIR/runtime-services.json"
            jq -e '(.plugins == []) and (.services == [])' "$TMPDIR/runtime-services.json" >/dev/null
            touch "$out"
          '';
    in
    {
      packages = {
        phenix-basic = basicComposition;
        phenix-full = fullComposition;
        phenix-harness = fullComposition;
        phenix = fullComposition;
        default = fullComposition;
      };
      apps = {
        phenix-basic.program = "${basicComposition}/bin/phenix";
        phenix-full.program = "${fullComposition}/bin/phenix";
        phenix-harness.program = "${fullComposition}/bin/phenix-harness";
        phenix.program = "${fullComposition}/bin/phenix";
        default.program = "${fullComposition}/bin/phenix";
        phenix-runtime.program = "${self.packages.${pkgs.system}.phenix-runtime}/bin/phenix-runtime";
      };
      checks = {
        phenix-plugin-packaging-products = pluginPackagingProducts;
        phenix-plugin-packaging-environment = pluginPackagingEnvironment;
        phenix-plugin-packaging-settings = pluginPackagingSettings;
        phenix-plugin-packaging-isolation = pluginPackagingIsolation;

        phenix-plugin-packaging = pluginPackagingWrapper;
      };
    };
}
