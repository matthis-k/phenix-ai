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

  mkPhenix =
    {
      pkgs,
      runtimeOnly ? false,
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
      base =
        if runtimeOnly then
          self.packages.${pkgs.system}.phenix-runtime
        else
          self.packages.${pkgs.system}.phenix-harness-runtime;
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
                  ${pkgs.lib.optionalString (resources != [ ]) ''
                    wrapProgram "$out/bin/$program" \
                      --set PHENIX_SKILL_PATH "$out/share/phenix/skills"
                  ''}
                fi
              done
            '';
      };
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
      checks.phenix-plugin-packaging =
        pkgs.runCommand "phenix-plugin-packaging-check" { nativeBuildInputs = [ pkgs.jq ]; }
          ''
            set -euxo pipefail
            test -x "${defaultComposition}/bin/phenix"
            test -x "${defaultComposition}/bin/phenix-harness"
            test -f "${defaultComposition}/share/phenix/runtime.json"
            test -f "${defaultComposition}/share/phenix/skills/write/SKILL.md"
            test -f "${defaultComposition}/share/phenix/skills/pstack-LICENSE"
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

            for policy in working-dir workdir-write; do
              export PHENIX_STATE_DB="$TMPDIR/environment-$policy.sqlite"
              PHENIX_LOCAL_FILESYSTEM_POLICY="$policy" \
                "${defaultComposition}/bin/phenix" --list-services \
                > "$TMPDIR/environment-$policy.json"
              jq -e '(.plugins | index("phenix.environment.local")) != null' \
                "$TMPDIR/environment-$policy.json" >/dev/null
            done

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

            export PHENIX_STATE_DB="$TMPDIR/session-only.sqlite"
            "${sessionOnlyComposition}/bin/phenix" --list-services > "$TMPDIR/session-only.json"
            jq -e '(.plugins == ["phenix.sessions"]) and (.services | index("phenix.sessions@1") != null) and (.services | index("phenix.context@1") == null)' "$TMPDIR/session-only.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/context-only.sqlite"
            "${contextOnlyComposition}/bin/phenix" --list-services > "$TMPDIR/context-only.json"
            jq -e '((.plugins | sort) == ["phenix.context", "phenix.execution"]) and (.services | index("phenix.context@1") != null) and (.services | index("phenix.sessions@1") == null)' "$TMPDIR/context-only.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/adapter-only.sqlite"
            "${adapterOnlyComposition}/bin/phenix" --list-services > "$TMPDIR/adapter-only.json"
            jq -e '(.plugins == ["phenix.adapter.acp"]) and (.services == [])' "$TMPDIR/adapter-only.json" >/dev/null

            export PHENIX_STATE_DB="$TMPDIR/resource.sqlite"
            test -e "${resourceComposition}/share/phenix-plugin/resources/README.txt"
            "${resourceComposition}/bin/phenix" --list-services > "$TMPDIR/resource-services.json"
            jq -e '(.plugins | index("fixture.resources")) != null' "$TMPDIR/resource-services.json" >/dev/null

            test -x "${runtimeComposition}/bin/phenix-runtime"
            test ! -e "${runtimeComposition}/bin/phenix"
            test ! -e "${runtimeComposition}/bin/phenix-harness"
            "${runtimeComposition}/bin/phenix-runtime" --list-services > "$TMPDIR/runtime-services.json"
            jq -e '(.plugins == []) and (.services == [])' "$TMPDIR/runtime-services.json" >/dev/null
            touch "$out"
          '';
    };
}
