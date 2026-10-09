{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      self',
      system,
      ...
    }:
    let
      maintenanceLib = inputs.phenix-flake-ci.lib;
      candidateCommit = import ./commit-candidate.nix;

      repositoryRoot = ''
        repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
        cd "$repo_root"
      '';
      rustRoot = ''
        ${repositoryRoot}
        cd rust
      '';

      sourceCi = {
        enable = true;
        stage = "source";
        name = "Source";
        timeoutMinutes = 60;
      };

      mkNixCheckSuite =
        {
          check ? null,
          checks ? [ ],
          name,
          needs ? [ ],
          cache ? false,
        }:
        let
          selectedChecks = if check != null then [ check ] else checks;
          checkTargets = pkgs.lib.concatMapStringsSep " " (
            selectedCheck: ''".#checks.$system.${selectedCheck}"''
          ) selectedChecks;
        in
        assert (check != null) != (checks != [ ]);
        {
          inherit
            cache
            name
            needs
            ;
          runtimeInputs = pkgs: [
            pkgs.git
            pkgs.nix
          ];
          exec = ''
            ${repositoryRoot}
            system="$(nix eval --impure --raw --expr builtins.currentSystem)"
            nix build --no-link --print-build-logs \
              ${checkTargets}
          '';
        };

      rustUnitPackageShards = {
        core = [
          "phenix-application-interface"
          "phenix-model-adapter"
          "phenix-client"
          "phenix-contract"
          "phenix-core"
          "phenix-domain"
          "phenix-provider-sdk"
          "phenix-runtime"
        ];

        protocolSdk = [
          "phenix-acp-stdio"
          "phenix-adapter-acp"
          "phenix-model-adapter-acp"
          "phenix-model-adapter-native"
          "phenix-binding-generator"
          "phenix-client-acp"
          "phenix-sdk"
          "phenix-sdk-macros"
        ];

        pluginFoundation = [
          "phenix-plugin-artifacts"
          "phenix-plugin-basic-context"
          "phenix-plugin-basic-model"
          "phenix-plugin-basic-skills"
          "phenix-plugin-basic-tools"
          "phenix-plugin-catalog"
          "phenix-plugin-command-toolbelt"
          "phenix-plugin-debug"
          "phenix-plugin-environment-local"
          "phenix-plugin-frontend"
          "phenix-plugin-interactive-ui"
          "phenix-plugin-hooks"
          "phenix-plugin-invocation-defaults"
        ];

        pluginState = [
          "phenix-plugin-api"
          "phenix-plugin-context"
          "phenix-plugin-execution"
          "phenix-plugin-jobs"
          "phenix-plugin-options"
          "phenix-plugin-session-tree"
          "phenix-plugin-sessions"
          "phenix-plugin-step-runner"
          "phenix-plugin-workspace"
        ];

        agentProduct = [
          "phenix-agent-configurations"
          "phenix-harness"
          "phenix-plugin-basic-agent"
          "phenix-plugin-efficiency-evaluation"
          "phenix-plugin-memory"
          "phenix-plugin-models"
          "phenix-plugin-openai-codex"
          "phenix-plugin-planning"
          "phenix-plugin-providers"
          "phenix-plugin-repository-workers"
        ];
      };

      rustUnitWorkspacePackages = map builtins.baseNameOf (builtins.fromTOML (
        builtins.readFile ../rust/Cargo.toml
      )).workspace.members;
      rustUnitExpectedPackages = builtins.filter (
        package: package != "phenix-binding-lua"
      ) rustUnitWorkspacePackages;
      rustUnitShardedPackages = builtins.concatLists (builtins.attrValues rustUnitPackageShards);
      rustUnitShardsValid =
        if
          builtins.sort builtins.lessThan rustUnitShardedPackages
          != builtins.sort builtins.lessThan rustUnitExpectedPackages
        then
          throw "phenix-ai: Rust unit test shards must cover every workspace package except phenix-binding-lua exactly once"
        else
          true;

      rustUnitRuntimeInputs = pkgs: [
        pkgs.bash
        pkgs.bubblewrap
        pkgs.cargo
        pkgs.coreutils
        pkgs.git
        pkgs.iproute2
        pkgs.ripgrep
        pkgs.rsync
        pkgs.rustc
        pkgs.slirp4netns
        pkgs.socat
        pkgs.util-linux
      ];

      mkRustUnitSuite =
        {
          name,
          packages,
        }:
        let
          packageFlags = pkgs.lib.concatMapStringsSep " " (package: "-p ${package}") packages;
        in
        {
          inherit name;
          needs = [ ];
          runtimeInputs = rustUnitRuntimeInputs;
          impact = {
            kind = "cargo";
            inherit packages;
          };
          exec = ''
            ${rustRoot}

            if [ "''${GITHUB_ACTIONS:-}" = "true" ] \
              && [ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ] \
              && [ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)" = "1" ]; then
              /usr/bin/sudo -n /usr/sbin/sysctl \
                -w kernel.apparmor_restrict_unprivileged_userns=0 >/dev/null
            fi

            timeout --signal=KILL 900 \
              cargo test --quiet --locked ${packageFlags} --lib --bins -- --nocapture
          '';
        };

      ciCommands =
        assert rustUnitShardsValid;
        maintenanceLib.mkCi {
          ci = {
            name = "CI";
            timeoutMinutes = 120;
            env = {
              CARGO_HOME = "\${{ runner.temp }}/phenix-cargo-home";
              CARGO_TARGET_DIR = "\${{ runner.temp }}/phenix-cargo-target";
              CARGO_TERM_QUIET = "true";
            };
            cache = {
              paths = [
                "\${{ runner.temp }}/phenix-cargo-home"
                "\${{ runner.temp }}/phenix-cargo-target"
              ];
              key = "phenix-rust-v2-\${{ runner.os }}-\${{ hashFiles('rust/Cargo.lock', 'flake.lock') }}";
              restoreKeys = [ "phenix-rust-v2-\${{ runner.os }}-" ];
              writer = "build.rust-workspace";
              saveOnDefaultBranch = true;
            };
          };

          build = {
            rust-workspace = {
              name = "Rust workspace";
              runtimeInputs = pkgs: [
                pkgs.cargo
                pkgs.git
                pkgs.rustc
              ];
              exec = ''
                ${rustRoot}
                cargo build --workspace --locked --quiet
              '';
            };

            stitch-mcp = mkNixCheckSuite {
              check = "stitch-mcp-package";
              name = "Stitch MCP package";
            };
          };

          test = {
            unit-core = mkRustUnitSuite {
              name = "Rust unit tests / core";
              packages = rustUnitPackageShards.core;
            };

            unit-protocol-sdk = mkRustUnitSuite {
              name = "Rust unit tests / protocol and SDK";
              packages = rustUnitPackageShards.protocolSdk;
            };

            unit-plugin-foundation = mkRustUnitSuite {
              name = "Rust unit tests / plugin foundation";
              packages = rustUnitPackageShards.pluginFoundation;
            };

            unit-plugin-state = mkRustUnitSuite {
              name = "Rust unit tests / plugin state";
              packages = rustUnitPackageShards.pluginState;
            };

            unit-agent-product = mkRustUnitSuite {
              name = "Rust unit tests / agent and product";
              packages = rustUnitPackageShards.agentProduct;
            };

            docs = {
              name = "Rust doc tests";
              needs = [ ];
              runtimeInputs = pkgs: [
                pkgs.cargo
                pkgs.git
                pkgs.rustc
              ];
              exec = ''
                ${rustRoot}
                cargo test --quiet --workspace --doc --locked
              '';
            };

            sdk = {
              name = "SDK tests";
              needs = [ ];
              runtimeInputs = pkgs: [
                pkgs.cargo
                pkgs.git
                pkgs.rustc
              ];
              exec = ''
                ${rustRoot}
                cargo test --quiet --locked -p phenix-sdk --tests
              '';
            };

            adapter-domain =
              let
                packages = [
                  "phenix-adapter-acp"
                  "phenix-domain"
                ];
              in
              {
                name = "Adapter and domain tests";
                impact = {
                  kind = "cargo";
                  inherit packages;
                };
                needs = [ ];
                runtimeInputs = pkgs: [
                  pkgs.cargo
                  pkgs.git
                  pkgs.rustc
                ];
                exec = ''
                  ${rustRoot}
                  cargo test --quiet --locked \
                    ${pkgs.lib.concatMapStringsSep " " (package: "-p ${package}") packages} \
                    --tests
                '';
              };

            harness =
              let
                package = "phenix-harness";
              in
              {
                name = "Harness code tests";
                impact = {
                  kind = "cargo";
                  packages = [ package ];
                };
                needs = [ ];
                runtimeInputs = pkgs: [
                  pkgs.cargo
                  pkgs.git
                  pkgs.rustc
                ];
                exec = ''
                  ${rustRoot}
                  cargo test --quiet --locked -p ${package} \
                    --test acp_session_lifecycle \
                    --test component_graph \
                    --test supported_product_journeys
                '';
              };
          };

          runtime = {
            process-roundtrip = {
              name = "Harness process roundtrip";
              needs = [ ];
              runtimeInputs = pkgs: [
                pkgs.cargo
                pkgs.git
                pkgs.rustc
              ];
              exec = ''
                ${rustRoot}
                cargo test --quiet --locked -p phenix-harness --test process_roundtrip
              '';
            };

            stitch = mkNixCheckSuite {
              check = "stitch-runtime-smoke";
              name = "Stitch runtime smoke";
            };
          };

          integration = {
            model-adapter-acp = {
              name = "ACP model adapter integration";
              needs = [ ];
              runtimeInputs = pkgs: [
                pkgs.cargo
                pkgs.git
                pkgs.rustc
              ];
              exec = ''
                ${rustRoot}
                cargo test --quiet --locked -p phenix-model-adapter-acp --tests
              '';
            };

            plugin-packaging = mkNixCheckSuite {
              check = "phenix-plugin-packaging";
              name = "Plugin packaging integration";
            };
          };

          product = {
            phenix-runtime = {
              name = "Phenix supported runtime journey";
              needs = [ ];
              cache = false;
              runtimeInputs = pkgs: [
                pkgs.git
                pkgs.nix
              ];
              exec = ''
                ${repositoryRoot}
                system="$(nix eval --impure --raw --expr builtins.currentSystem)"
                dependency_root="''${RUNNER_TEMP:-$TMPDIR}/phenix-product-rust-dependencies"

                nix build --out-link "$dependency_root" \
                  ".#packages.$system.phenix-product-rust-dependencies"

                nix build --no-link --print-build-logs \
                  ".#checks.$system.phenix-product-runtime-smoke" \
                  ".#checks.$system.phenix-plugin-packaging-products" \
                  ".#checks.$system.phenix-plugin-packaging-environment" \
                  ".#checks.$system.phenix-plugin-packaging-settings" \
                  ".#checks.$system.phenix-plugin-packaging-isolation"
              '';
            };

            phenix-standalone-runtime = mkNixCheckSuite {
              check = "phenix-product-standalone-runtime-smoke";
              name = "Phenix standalone runtime package";
            };

            phenix-lua-binding = mkNixCheckSuite {
              check = "phenix-product-lua-smoke";
              name = "Phenix Lua binding product fixture";
            };
          };
        };

      maintenance = maintenanceLib.mkMaintenance {
        name = "maintenance";
        description = "Phenix maintenance";
        ci.github = {
          enable = true;
          outputName = "phenix-maintenance";
          # Fast feedback on PRs; full semantic CI runs on main and workflow_dispatch.
          prImpact = {
            enable = true;
            workspace = "rust";
            verifiedShardChange = {
              source = "modules/development.nix";
              list = "pluginFoundation";
              job = "test-unit-plugin-foundation";
              workflow = ".github/workflows/ci.yml";
            };
          };
          pullRequestJobs = [
            "source"
            "clippy"
            "test-unit-core"
            "test-unit-protocol-sdk"
            "test-unit-plugin-foundation"
            "test-unit-agent-product"
            "test-harness"
            "test-adapter-domain"
          ];
          nixCache = {
            enable = true;
            jobs = [ "product-phenix-runtime" ];
            primaryKey = "phenix-product-nix-\${{ runner.os }}-\${{ hashFiles('modules/rust-artifacts.nix', 'flake.lock', 'rust/Cargo.lock', 'rust/**/Cargo.toml') }}";
            restorePrefixesFirstMatch = [ "phenix-product-nix-\${{ runner.os }}-" ];
            gcMaxStoreSizeLinux = "4G";
            saveOnDefaultBranch = true;
          };
        };
        gitHooks = {
          enable = true;
          path = ".githooks";
          preCommit = [ "fix" ];
        };

        commands = ciCommands // {
          commit = candidateCommit.command;
          all = {
            description = "Run static validation and the complete semantic CI pipeline";
            dependencies = [
              [ "check" ]
              [ "pipeline" ]
            ];
            exec = ''
              "$0" check
              "$0" pipeline
            '';
          };

          check = {
            description = "Run static/source validation";
            order = [
              "source"
              "rust"
            ];
            commands = {
              source = {
                description = "Formatting, source analysis, and workflow consistency";
                order = [
                  "nix-format"
                  "rust-format"
                  "statix"
                  "actionlint"
                  "plugin-architecture"
                  "structural-boundaries"
                  "application-interface"
                  "workflow-sync"
                ];
                commands = {
                  nix-format = {
                    description = "Nix formatting";
                    ci = sourceCi // {
                      stepName = "Nix formatting";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.findutils
                      pkgs.git
                      pkgs.nixfmt
                    ];
                    exec = ''
                      ${repositoryRoot}
                      find . -type f -name '*.nix' \
                        -not -path './.git/*' \
                        -print0 |
                        xargs -0 -r nixfmt --check
                    '';
                  };

                  rust-format = {
                    description = "Rust formatting";
                    ci = sourceCi // {
                      stepName = "Rust formatting";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.cargo
                      pkgs.git
                      pkgs.rustfmt
                    ];
                    exec = ''
                      ${rustRoot}
                      cargo fmt --all --check
                    '';
                  };

                  statix = {
                    description = "Nix static analysis";
                    ci = sourceCi // {
                      stepName = "Statix";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.git
                      pkgs.statix
                    ];
                    exec = ''
                      ${repositoryRoot}
                      statix check --ignore '.git/**'
                    '';
                  };

                  actionlint = {
                    description = "GitHub Actions syntax";
                    ci = sourceCi // {
                      stepName = "Actionlint";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.actionlint
                      pkgs.findutils
                      pkgs.git
                    ];
                    exec = ''
                      ${repositoryRoot}
                      find .github/workflows -type f \
                        \( -name '*.yml' -o -name '*.yaml' \) -print0 |
                        xargs -0 -r actionlint
                    '';
                  };

                  plugin-architecture = {
                    description = "Plugin package roles and runtime dependency boundaries";
                    ci = sourceCi // {
                      stepName = "Plugin architecture";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.bash
                      pkgs.cargo
                      pkgs.coreutils
                      pkgs.git
                      pkgs.jq
                    ];
                    exec = ''
                      ${repositoryRoot}
                      bash scripts/check-plugin-architecture.sh
                      bash scripts/check-rust-safety-policy.sh
                      bash scripts/check-spec-lifecycle-fixtures.sh
                      bash scripts/check-spec-lifecycle.sh
                      ${candidateCommit.test}
                    '';
                  };

                  structural-boundaries = {
                    description = "Canonical structural boundary enforcement";
                    ci = sourceCi // {
                      stepName = "Structural boundaries";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.bash
                      pkgs.findutils
                      pkgs.git
                    ];
                    exec = ''
                      ${repositoryRoot}
                      bash scripts/check-structural-boundaries.sh
                    '';
                  };

                  application-interface = {
                    description = "Fixed application descriptor snapshot";
                    ci = sourceCi // {
                      stepName = "Application interface";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.cargo
                      pkgs.git
                      pkgs.rustc
                      pkgs.stdenv.cc
                    ];
                    exec = ''
                      ${rustRoot}
                      cargo run --quiet --locked -p phenix-application-interface \
                        --bin phenix-application-descriptor -- \
                        --check ../share/phenix/interfaces/phenix.application@1.json
                    '';
                  };

                  workflow-sync = {
                    description = "Committed generated repository integrations match the Nix declarations";
                    ci = sourceCi // {
                      stepName = "Generated workflow";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.diffutils
                      pkgs.git
                      pkgs.nix
                    ];
                    exec = ''
                      ${repositoryRoot}
                      system="$(nix eval --impure --raw --expr builtins.currentSystem)"
                      generated="$(mktemp)"
                      trap 'rm -f "$generated"' EXIT
                      nix eval --raw \
                        ".#packages.$system.phenix-maintenance.phenixMaintenance.ci.github.workflow" \
                        > "$generated"
                      diff -u .github/workflows/ci.yml "$generated"

                      hooks="$(nix build --no-link --print-out-paths \
                        ".#packages.$system.phenix-maintenance-git-hooks")"
                      diff -u "$hooks/.githooks/pre-commit" .githooks/pre-commit
                    '';
                  };
                };
              };

              rust = {
                description = "Rust static analysis with Clippy";
                ci = {
                  enable = true;
                  stage = "clippy";
                  impact = {
                    kind = "cargo";
                    packages = null;
                  };
                  name = "Clippy";
                  stepName = "Clippy";
                  timeoutMinutes = 60;
                };
                runtimeInputs = pkgs: [
                  pkgs.cargo
                  pkgs.clippy
                  pkgs.git
                  pkgs.jq
                  pkgs.rustc
                ];
                exec = ''
                  ${rustRoot}
                  # The impact preflight uses Cargo metadata and includes all reverse
                  # dependents. A missing or invalid plan keeps full CI coverage.
                  clippyTargets=(--workspace)
                  impactPackages="''${PHENIX_IMPACT_PACKAGES:-null}"
                  if [ "$impactPackages" != null ] && [ -n "$impactPackages" ]; then
                    if printf '%s' "$impactPackages" |
                      jq -e 'type == "array" and all(.[]; type == "string")' >/dev/null; then
                      mapfile -t selectedPackages < <(
                        printf '%s' "$impactPackages" | jq -r '.[]'
                      )
                      if [ "''${#selectedPackages[@]}" -gt 0 ]; then
                        clippyTargets=()
                        for package in "''${selectedPackages[@]}"; do
                          clippyTargets+=(--package "$package")
                        done
                      fi
                    else
                      echo "Invalid Cargo impact package list; linting workspace" >&2
                    fi
                  fi
                  cargo clippy --quiet "''${clippyTargets[@]}" --all-targets --locked -- -D warnings
                  cargo clippy --quiet "''${clippyTargets[@]}" --lib --bins --locked -- \
                    -D warnings \
                    -D clippy::unwrap_used \
                    -D clippy::panic
                '';
              };
            };
          };

          fix = {
            description = "Apply deterministic Nix and Rust normalization";
            runtimeInputs = pkgs: [
              pkgs.cargo
              pkgs.findutils
              pkgs.git
              pkgs.nixfmt
              pkgs.rustfmt
              pkgs.statix
            ];
            exec = ''
              ${repositoryRoot}

              statix fix

              find . -type f -name '*.nix' \
                -not -path './.git/*' \
                -print0 |
                xargs -0 -r nixfmt

              (
                cd rust
                cargo fmt --all
              )
            '';
          };
        };
      };

      maintenancePackage = maintenanceLib.mkMaintenancePackage {
        inherit pkgs maintenance;
      };
      maintenanceOutputs = maintenanceLib.mkMaintenanceOutputs {
        inherit maintenance;
        systems = [ system ];
        pkgsFor = _: pkgs;
        outputName = "phenix-maintenance";
      };
    in
    {
      packages = maintenanceOutputs.packages.${system};
      apps = maintenanceOutputs.apps.${system};

      devShells.default = pkgs.mkShell {
        name = "phenix-dev";
        packages = [
          pkgs.actionlint
          pkgs.bubblewrap
          pkgs.cargo
          pkgs.cargo-audit
          pkgs.cargo-deny
          pkgs.clippy
          pkgs.git
          pkgs.jq
          pkgs.lua-language-server
          pkgs.nixd
          pkgs.nixfmt
          pkgs.rsync
          pkgs.rust-analyzer
          pkgs.rustc
          pkgs.rustfmt
          pkgs.slirp4netns
          pkgs.statix
          pkgs.taplo
          maintenancePackage.package
          self'.packages.stitch
          self'.packages.stitch-mcp
        ];
        shellHook = ''
          ${maintenancePackage.shellHook}

          echo "phenix dev shell"
          echo "  all:          maintenance all"
          echo "  static:       maintenance check"
          echo "  build:        maintenance build"
          echo "  tests:        maintenance test"
          echo "  runtime:      maintenance runtime"
          echo "  integration:  maintenance integration"
          echo "  product:      maintenance product"
          echo "  fixes:        maintenance fix"
        '';
      };
    };
}
