{ inputs, self, ... }:
let
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];

  mkRustPackage =
    pkgs: package:
    pkgs.rustPlatform.buildRustPackage {
      pname = package;
      version = "0";
      src = pkgs.lib.cleanSource ../rust;
      cargoLock.lockFile = ../rust/Cargo.lock;
      cargoBuildFlags = [
        "--package"
        package
      ];
      doCheck = false;
      installPhase = ''
        runHook preInstall
        mkdir -p "$out/share/phenix-rust-package"
        printf '%s\n' ${pkgs.lib.escapeShellArg package} > "$out/share/phenix-rust-package/name"
        runHook postInstall
      '';
    };

  mkEmbeddedPluginPackage =
    pkgs: package: pkgs.writeTextDir "share/phenix-rust-package/name" "${package}\n";

  mkBinaryPackage =
    pkgs: package: binary:
    pkgs.rustPlatform.buildRustPackage {
      pname = package;
      version = "0";
      src = pkgs.lib.cleanSource ../rust;
      cargoLock.lockFile = ../rust/Cargo.lock;
      cargoBuildFlags = [
        "--package"
        package
        "--bin"
        binary
      ];
      doCheck = false;
      installPhase = ''
        runHook preInstall
        mkdir -p "$out/bin"
        executable="$(find target -path '*/release/${binary}' -type f -print -quit)"
        test -n "$executable"
        cp "$executable" "$out/bin/${binary}"
        runHook postInstall
      '';
    };

  mkLuaBindingPackage =
    pkgs:
    pkgs.rustPlatform.buildRustPackage {
      pname = "phenix-binding-lua";
      version = "0";
      src = pkgs.lib.cleanSource ../rust;
      cargoLock.lockFile = ../rust/Cargo.lock;
      cargoBuildFlags = [
        "--package"
        "phenix-binding-lua"
      ];
      doCheck = false;
      installPhase = ''
        runHook preInstall
        module="$(find target -path '*/release/libphenix.so' -type f -print -quit)"
        test -n "$module"
        mkdir -p "$out/lib/lua/5.1"
        cp "$module" "$out/lib/lua/5.1/phenix.so"
        runHook postInstall
      '';
    };

  pluginIds = {
    adapter-acp = "phenix.adapter.acp";
    api = "phenix.api";
    artifacts = "phenix.artifacts";
    basic-context = "phenix.basic-context";
    basic-model = "phenix.basic-model";
    basic-skills = "phenix.basic-skills";
    basic-tools = "phenix.basic-tools";
    command-toolbelt = "phenix.command-toolbelt";
    context = "phenix.context";
    debug = "phenix.debug";
    execution = "phenix.execution";
    frontend = "phenix.frontend-services";
    hooks = "phenix.hooks";
    jobs = "phenix.jobs";
    language = "phenix.language";
    memory = "phenix.memory";
    models = "phenix.models";
    options = "phenix.options";
    planning = "phenix.planning";
    repository-workers = "phenix.repository-workers";
    session-tree = "phenix.session-tree";
    sessions = "phenix.sessions";
    workspace = "phenix.workspace";
  };

  basicPluginCrates = {
    adapter-acp = "phenix-adapter-acp";
    api = "phenix-plugin-api";
    basic-context = "phenix-plugin-basic-context";
    basic-model = "phenix-plugin-basic-model";
    basic-skills = "phenix-plugin-basic-skills";
    basic-tools = "phenix-plugin-basic-tools";
  };

  pluginCrates = builtins.mapAttrs (
    name: _: basicPluginCrates.${name} or "phenix-plugin-${name}"
  ) pluginIds;

  pluginCrateValues = builtins.attrValues pluginCrates;
  uniquePluginCrates = builtins.attrNames (
    builtins.listToAttrs (
      map (package: {
        name = package;
        value = true;
      }) pluginCrateValues
    )
  );
  hasLegacyCliIdentity =
    pluginIds ? cli || builtins.elem "phenix.cli" (builtins.attrValues pluginIds);
  hasLegacySdkIdentity =
    pluginIds ? sdk || builtins.elem "phenix.sdk" (builtins.attrValues pluginIds);

  pluginRole =
    package:
    let
      manifest = builtins.fromTOML (builtins.readFile (../rust/crates + "/${package}/Cargo.toml"));
    in
    manifest.package.metadata.phenix.role or null;

  checkedPluginCrates =
    if hasLegacyCliIdentity then
      throw "phenixPlugins must use command-toolbelt rather than the legacy cli runtime identity"
    else if hasLegacySdkIdentity then
      throw "phenixPlugins must use api rather than the legacy sdk runtime identity"
    else if builtins.length pluginCrateValues != builtins.length uniquePluginCrates then
      throw "phenixPlugins entries must use unique implementation packages"
    else
      builtins.mapAttrs (
        name: package:
        let
          role = pluginRole package;
        in
        if role == "runtime-plugin" then
          package
        else
          throw "phenixPlugins.${name} uses ${package} with role ${builtins.toJSON role}; expected runtime-plugin"
      ) pluginCrates;

  pluginSets = builtins.listToAttrs (
    map (
      system:
      let
        pkgs = inputs.nixpkgs.legacyPackages.${system};
      in
      {
        name = system;
        value = builtins.mapAttrs (
          name: pluginId:
          self.lib.mkPhenixPlugin {
            inherit pkgs name;
            package = mkEmbeddedPluginPackage pkgs checkedPluginCrates.${name};
            manifest = {
              id = pluginId;
              version = 1;
              execution.kind = "embedded";
            };
          }
        ) pluginIds;
      }
    ) systems
  );

in
{
  flake = {
    phenixPlugins = pluginSets;
  };

  perSystem =
    { pkgs, system, ... }:
    let
      luaBinding = mkLuaBindingPackage pkgs;
      pluginPackageChecks = pkgs.lib.mapAttrs' (name: package: {
        name = "phenix-plugin-${name}-package";
        value = package;
      }) pluginSets.${system};
    in
    {
      packages = {
        phenix-core = mkRustPackage pkgs "phenix-core";
        phenix-client = mkRustPackage pkgs "phenix-client";
        phenix-application-interface =
          (mkBinaryPackage pkgs "phenix-application-interface" "phenix-application-descriptor").overrideAttrs
            (_: {
              postInstall = ''
                mkdir -p "$out/share/phenix/interfaces"
                "$out/bin/phenix-application-descriptor" \
                  > "$out/share/phenix/interfaces/phenix.application@1.json"
              '';
            });
        phenix-binding-lua = luaBinding;
        phenix-runtime = mkBinaryPackage pkgs "phenix-runtime" "phenix-runtime";
      };

      checks = pluginPackageChecks // {
        phenix-binding-lua-load =
          pkgs.runCommand "phenix-binding-lua-load-check"
            {
              nativeBuildInputs = [
                pkgs.luajit
                luaBinding
              ];
            }
            ''
              export LUA_CPATH="${luaBinding}/lib/lua/5.1/?.so;;"
              luajit -e '
                local phenix = require("phenix")
                assert(phenix.interface_id == "phenix.application@1")
                assert(type(phenix.connect) == "function")
                assert(type(phenix.descriptor) == "table")
                assert(type(phenix.descriptor.operations) == "table")
                local bindings = phenix.descriptor.bind({
                  _invoke_application = function(_, operation, _) return operation end,
                })
                for operation, metadata in pairs(phenix.descriptor.operations) do
                  assert(bindings[metadata.name](nil) == operation)
                end
              '
              touch "$out"
            '';
      };
    };
}
