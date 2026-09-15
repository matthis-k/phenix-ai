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
      conductorOnly ? false,
      plugins ? [ ],
      resources ? [ ],
      enabledPlugins ? null,
      layerPolicies ? [ ],
      settings ? { },
      configDirectory ? null,
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
      && settingsPrecedence == "nix"
    then
      base
    else
      pkgs.symlinkJoin {
        name = if conductorOnly then "phenix-conductor-composed" else "phenix-composed";
        paths = [ base ] ++ plugins ++ resources;
        nativeBuildInputs = [ pkgs.makeWrapper ];
        postBuild =
          if conductorOnly then
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
                  ${pkgs.lib.optionalString (resources != [ ]) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_SKILL_PATH "$out/share/phenix/skills"
                  ''}
                fi
              done
            '';
      };

  mkPhenix =
    args@{
      pkgs,
      conductorOnly ? false,
      ...
    }:
    let
      base =
        if conductorOnly then
          self.packages.${pkgs.system}.phenix-conductor
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
      defaultPluginNames = [
        "artifacts"
        "api"
        "command-toolbelt"
        "context"
        "debug"
        "execution"
        "frontend"
        "hooks"
        "jobs"
        "language"
        "models"
        "options"
        "planning"
        "repository-workers"
        "session-tree"
        "sessions"
        "workspace"
      ];
      defaultPlugins = map (name: self.phenixPlugins.${pkgs.system}.${name}) defaultPluginNames;
      harnessResources = self.packages.${pkgs.system}.phenix-harness-resources;
      defaultComposition = mkPhenix {
        inherit pkgs;
        plugins = defaultPlugins;
        resources = [ harnessResources ];
      };

      fixtureHarnessProgram = pkgs.writeShellScriptBin "phenix-harness" ''
        exec ${pkgs.jq}/bin/jq -cn \
          --arg default_config "''${PHENIX_DEFAULT_CONFIG_DIR:-}" \
          --arg config "''${PHENIX_CONFIG_DIR:-}" \
          --arg settings "''${PHENIX_NIX_SETTINGS:-}" \
          --arg settings_precedence "''${PHENIX_SETTINGS_PRECEDENCE:-}" \
          --arg plugin_packages "''${PHENIX_PLUGIN_PACKAGES:-}" \
          --arg enabled_plugins "''${PHENIX_ENABLED_PLUGINS:-}" \
          --arg layer_policy "''${PHENIX_LAYER_POLICY:-}" \
          --arg skill_path "''${PHENIX_SKILL_PATH:-}" \
          '{
            default_config: $default_config,
            config: $config,
            settings: $settings,
            settings_precedence: $settings_precedence,
            plugin_packages: $plugin_packages,
            enabled_plugins: $enabled_plugins,
            layer_policy: $layer_policy,
            skill_path: $skill_path
          }'
      '';
      fixtureBase = pkgs.runCommand "phenix-composition-fixture-base" { } ''
        mkdir -p "$out/bin"
        ln -s ${fixtureHarnessProgram}/bin/phenix-harness "$out/bin/phenix-harness"
        ln -s phenix-harness "$out/bin/phenix"
      '';
      fixtureConductorBase = pkgs.writeShellScriptBin "phenix-conductor" ''
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

      defaultFixtureComposition = mkFixturePhenix {
        plugins = defaultPlugins;
        resources = [ harnessResources ];
      };
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
      settingsComposition = mkFixturePhenix {
        plugins = defaultPlugins;
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
      filePrecedenceComposition = mkFixturePhenix {
        plugins = defaultPlugins;
        resources = [ harnessResources ];
        configDirectory = settingsConfigDirectory;
        settingsPrecedence = "file";
        settings = {
          global = {
            "session.auto_create" = false;
          };
        };
      };
      resourceComposition = mkFixturePhenix {
        plugins = defaultPlugins ++ [ resourcePlugin ];
        resources = [ harnessResources ];
      };
      adapterOnlyComposition = mkFixturePhenix {
        plugins = [ self.phenixPlugins.${pkgs.system}.adapter-acp ];
      };
      conductorFixtureComposition = mkPhenixWithBase {
        inherit pkgs;
        base = fixtureConductorBase;
        conductorOnly = true;
      };
    in
    {
      packages = {
        phenix-harness = defaultComposition;
        phenix = defaultComposition;
        default = defaultComposition;
      };
      apps = {
        phenix-harness.program = "${defaultComposition}/bin/phenix-harness";
        phenix.program = "${defaultComposition}/bin/phenix";
        default.program = "${defaultComposition}/bin/phenix";
        phenix-conductor.program = "${self.packages.${pkgs.system}.phenix-conductor}/bin/phenix-conductor";
      };
      checks.phenix-plugin-packaging =
        pkgs.runCommand "phenix-plugin-packaging-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euo pipefail

            test -x "${defaultFixtureComposition}/bin/phenix"
            test -x "${defaultFixtureComposition}/bin/phenix-harness"
            test -f "${defaultFixtureComposition}/share/phenix/runtime.json"
            test -f "${defaultFixtureComposition}/share/phenix/skills/write/SKILL.md"
            test -f "${defaultFixtureComposition}/share/phenix/skills/pstack-LICENSE"
            "${defaultFixtureComposition}/bin/phenix" > "$TMPDIR/default.json"
            jq -e '
              (.enabled_plugins | split(",") | length) == 17
              and (.enabled_plugins | contains("phenix.adapter.acp") | not)
              and (.default_config | length > 0)
              and (.config | length > 0)
              and (.skill_path | length > 0)
            ' "$TMPDIR/default.json" >/dev/null

            "${settingsComposition}/bin/phenix" > "$TMPDIR/settings.json"
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

            "${filePrecedenceComposition}/bin/phenix" > "$TMPDIR/settings-file-first.json"
            jq -e '.settings_precedence == "file" and (.config | length > 0)' \
              "$TMPDIR/settings-file-first.json" >/dev/null
            config_path="$(jq -r '.config' "$TMPDIR/settings-file-first.json")/settings.json"
            jq -e '.global["session.auto_create"] == true' "$config_path" >/dev/null

            test -e "${resourceComposition}/share/phenix-plugin/resources/README.txt"
            "${resourceComposition}/bin/phenix" > "$TMPDIR/resource.json"
            jq -e '(.plugin_packages | length > 0)' "$TMPDIR/resource.json" >/dev/null

            "${adapterOnlyComposition}/bin/phenix" > "$TMPDIR/adapter.json"
            jq -e '.enabled_plugins == "phenix.adapter.acp"' "$TMPDIR/adapter.json" >/dev/null

            test -x "${conductorFixtureComposition}/bin/phenix-conductor"
            test ! -e "${conductorFixtureComposition}/bin/phenix"
            test ! -e "${conductorFixtureComposition}/bin/phenix-harness"

            touch "$out"
          '';
    };
}
