{
  description = "Phenix graph, storage, plugin, and kernel foundation";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  inputs.phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/e83c6029afc0140a242ae8240dbff6efe5979725";

  outputs =
    {
      self,
      nixpkgs,
      phenix-flake-ci,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      inherit (pkgs) lib;
      workspace = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      members = workspace.workspace.members;
      manifests = lib.genAttrs members (
        path: builtins.fromTOML (builtins.readFile (./. + "/${path}/Cargo.toml"))
      );
      memberFor =
        name:
        lib.findFirst (
          path: manifests.${path}.package.name == name
        ) (throw "Missing Cargo package ${name}") members;
      # Include dev/build dependencies too: semantic tests and examples need both.
      closure =
        roots:
        map (item: item.key) (
          builtins.genericClosure {
            startSet = map (key: { inherit key; }) roots;
            operator =
              { key }:
              map (key: { inherit key; }) (
                lib.concatMap
                  (
                    kind:
                    lib.concatMap (
                      dep:
                      lib.optional (builtins.isAttrs dep && dep ? path) (
                        lib.removePrefix (toString ./. + "/") (toString (./. + "/${key}/${dep.path}"))
                      )
                    ) (builtins.attrValues (manifests.${key}.${kind} or { }))
                  )
                  [
                    "dependencies"
                    "dev-dependencies"
                    "build-dependencies"
                  ]
              );
          }
        );
      sourceFor =
        paths:
        lib.cleanSourceWith {
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions ([ ./Cargo.lock ] ++ map (path: ./. + "/${path}") paths);
          };
          filter =
            path: type:
            !(lib.elem (baseNameOf path) [
              "target"
              ".git"
            ]);
        };
      cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
      mkRust =
        {
          name,
          packages,
          command,
          release ? false,
          installPhase ? "mkdir -p $out; touch $out/passed",
        }:
        let
          paths = closure (map memberFor packages);
          manifest = (pkgs.formats.toml { }).generate "Cargo.toml" (
            workspace
            // {
              workspace = workspace.workspace // {
                members = paths;
              };
            }
          );
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "phenix-${name}";
          version = "0.1.0";
          src = sourceFor paths;
          inherit cargoDeps installPhase;
          postUnpack = ''cp ${manifest} "$sourceRoot/Cargo.toml"'';
          nativeBuildInputs = [ pkgs.clippy ];
          buildType = if release then "release" else "debug";
          buildPhase = ''
            runHook preBuild
            # Prune workspace-only records after narrowing members; registry inputs stay vendored.
            cargo metadata --offline --format-version 1 > /dev/null
            ${command}
            runHook postBuild
          '';
          doCheck = false;
        };
      test =
        name: packages: flags:
        mkRust {
          inherit name packages;
          command = ''
            echo "Starting ${name} test targets (bounded to 480 seconds)"
            ${pkgs.coreutils}/bin/timeout --signal=TERM --kill-after=15s 480s \
              cargo test --locked --offline ${lib.concatMapStringsSep " " (p: "-p ${p}") packages} ${flags} -- --nocapture
          '';
        };
      allPackages = map (path: manifests.${path}.package.name) members;
      integrationPackages = lib.filter (
        name:
        lib.hasInfix "integration" name || name == "phenix-storage-graph" || name == "phenix-storage-plugin"
      ) allPackages;
      checkConfig = import ./ci/flake-check-config.nix;
    in
    {
      lib.flakeCiWorkflow = phenix-flake-ci.lib.mkFlakeCiWorkflow {
        ci = checkConfig.ci;
        configPath = "ci/flake-check-config.nix";
        libraryRef = phenix-flake-ci.rev;
      };
      packages.${system}.kernel-smoke = mkRust {
        name = "kernel-smoke";
        packages = [ "phenix-kernel" ];
        release = true;
        command = "cargo build --locked --offline -p phenix-kernel --release --example basic";
        installPhase = ''
          mkdir -p "$out/bin"
          install -m755 target/release/examples/basic "$out/bin/kernel-smoke"
          "$out/bin/kernel-smoke"
        '';
      };
      checks.${system} = {
        workflow = pkgs.runCommand "phenix-workflow-sync" { } ''
          diff -u ${pkgs.writeText "flake-ci.yml" self.lib.flakeCiWorkflow} ${./.github/workflows/flake-ci.yml}
          touch $out
        '';
        kernel-smoke = self.packages.${system}.kernel-smoke;
        format =
          pkgs.runCommand "phenix-format"
            {
              nativeBuildInputs = [
                pkgs.cargo
                pkgs.rustfmt
                pkgs.nixfmt
              ];
            }
            ''
              cp -r ${sourceFor members} source
              chmod -R u+w source
              cp ${./Cargo.toml} source/Cargo.toml
              cd source
              cargo fmt --all --check
              nixfmt --check ${./flake.nix} ${./ci/flake-check-config.nix}
              touch $out
            '';
        clippy = mkRust {
          name = "clippy";
          packages = allPackages;
          command = "cargo clippy --locked --offline --workspace --all-targets -- -D warnings";
        };
        storage = test "storage" [ "phenix-storage" "phenix-schema" ] "--all-targets";
        graph = test "graph" [ "phenix-graph" "phenix-plugin" ] "--all-targets";
        kernel = test "kernel" [ "phenix-kernel" ] "--all-targets";
        derived = test "derived" [ "phenix-storage-graph" ] "--test derived --test derived_plugins";
        no-default = test "no-default" allPackages "--no-default-features --lib --tests";
        integration =
          if integrationPackages == [ ] then
            test "integration" [ "phenix-kernel" ] "--test storage"
          else
            test "integration" ([ "phenix-kernel" ] ++ integrationPackages) "--all-targets";
      };
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rustfmt
          pkgs.clippy
          pkgs.binutils
          pkgs.lld
        ];
      };
      formatter.${system} = pkgs.nixfmt;
    };
}
