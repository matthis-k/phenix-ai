{
  description = "Phenix graph, storage, plugin, and kernel foundation";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3";
  inputs.phenix-flake-ci.url = "github:matthis-k/phenix-flake-ci/a8177632e578629da0de551d248079b7c99ed245";

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
        let
          packageArgs = lib.concatMapStringsSep " " (p: "-p ${lib.escapeShellArg p}") packages;
        in
        mkRust {
          inherit name packages;
          command = ''
            # Fast path: compile and test once, running every test binary even
            # after failures. On any failure, Cargo's keep-going build collects
            # independent compilation errors; probe packages only when that
            # compilation also fails. Other Nix roots continue independently.
            if cargo test --no-fail-fast --locked --offline ${packageArgs} ${flags}; then
              :
            else
              echo "Suite failed; attempting every independent compilation" >&2
              if cargo build --keep-going --locked --offline ${packageArgs} ${flags}; then
                echo "Compilation passed; failing test binaries were already run" >&2
              else
                echo "Compilation failed; probing packages independently" >&2
                for package in ${lib.escapeShellArgs packages}; do
                  if cargo test --no-fail-fast --locked --offline -p "$package" ${flags}; then
                    printf 'PASS %s\n' "$package"
                  else
                    printf 'FAIL %s\n' "$package" >&2
                  fi
                done
              fi
              exit 1
            fi
          '';
        };
      allPackages = map (path: manifests.${path}.package.name) members;
      # Default-feature suites are a disjoint partition: no test executable
      # should run in two Nix checks with identical Cargo feature settings.
      # The no-default profile is intentionally separate.
      storagePackages = [
        "phenix-schema"
        "phenix-storage"
      ];
      graphPackages = [
        "phenix-graph"
        "phenix-plugin"
        "phenix_port_args_derive"
      ];
      kernelPackages = [ "phenix-kernel" ];
      derivedPackages = [ "phenix-storage-graph" ];
      integrationPackages = [ "phenix-storage-plugin" ];
      defaultPackages = lib.concatLists [
        storagePackages
        graphPackages
        kernelPackages
        derivedPackages
        integrationPackages
      ];
      completeDefaultCoverage =
        (lib.sort builtins.lessThan defaultPackages) == (lib.sort builtins.lessThan allPackages);
      # The runtime pilot intentionally reuses the existing kernel-smoke
      # package and leaves its original build-time check in place.
      # No second Cargo compilation or test inventory is introduced.
      runtimeSuite = phenix-flake-ci.lib.mkSuite {
        inherit pkgs;
        tests.kernel-smoke = {
          command.program = lib.getExe' self.packages.${system}.kernel-smoke "kernel-smoke";
          expect.exitCode = 0;
        };
      };
      checkConfig = import ./ci/flake-check-config.nix;
    in
    {
      lib.flakeCiWorkflows = phenix-flake-ci.lib.mkFlakeCiWorkflows {
        ci = checkConfig.ci;
        configPath = "ci/flake-check-config.nix";
        libraryRef = phenix-flake-ci.rev;
      };
      # Local execution reuses the same pinned DAG planner, runtime runner,
      # and exact-head evidence contract as the GitHub workflows.
      apps.${system}.ci = {
        type = "app";
        program = toString (
          pkgs.writeShellScript "phenix-ci-local" ''
            exec ${lib.getExe pkgs.python3} ${phenix-flake-ci.outPath}/lib/flake-ci-local.py "$@"
          ''
        );
      };
      packages.${system} = {
        kernel-smoke = mkRust {
          name = "kernel-smoke";
          packages = [ "phenix-kernel" ];
          release = true;
          command = "cargo build --locked --offline -p phenix-kernel --release --example basic";
          installPhase = ''
            mkdir -p "$out/bin"
            install -m755 target/release/examples/basic "$out/bin/kernel-smoke"
            # The prebuilt runtime test package executes the binary once,
            # after all builds reach the synchronization barrier.
          '';
        };
        "test.kernel-smoke" = runtimeSuite.packages."test.kernel-smoke";
      };
      checks.${system} = {
        # A new workspace member must belong to exactly one default test suite.
        workflow =
          assert completeDefaultCoverage;
          pkgs.runCommand "phenix-workflow-sync" { } ''
            diff -u ${pkgs.writeText "flake-ci.yml" self.lib.flakeCiWorkflows.trusted} ${./.github/workflows/flake-ci.yml}
            diff -u ${pkgs.writeText "flake-ci-pr.yml" self.lib.flakeCiWorkflows.pullRequest} ${./.github/workflows/flake-ci-pr.yml}
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
              # Print a real patch on formatting failure so CI is actionable.
              for file in ${./flake.nix} ${./ci/flake-check-config.nix}; do
                expected="$(mktemp --suffix=.nix)"
                cp "$file" "$expected"
                chmod u+w "$expected"
                nixfmt "$expected"
                diff -u --label "$file" "$file" --label "nixfmt expected" "$expected"
              done
              touch $out
            '';
        clippy = mkRust {
          name = "clippy";
          packages = allPackages;
          command = "cargo clippy --locked --offline --workspace --all-targets -- -D warnings";
        };
        storage = test "storage" storagePackages "--all-targets";
        graph = test "graph" graphPackages "--all-targets";
        kernel = test "kernel" kernelPackages "--all-targets";
        derived = test "derived" derivedPackages "--all-targets";
        no-default = test "no-default" allPackages "--no-default-features --lib --tests";
        integration = test "integration" integrationPackages "--all-targets";
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
