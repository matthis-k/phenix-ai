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
          check,
          name,
          needs ? [ ],
          cache ? false,
        }:
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
            nix build --no-link --print-build-logs ".#checks.$system.${check}"
          '';
        };

      ciCommands = maintenanceLib.mkCi {
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
            key = "phenix-rust-\${{ runner.os }}-\${{ github.sha }}";
            restoreKeys = [ "phenix-rust-\${{ runner.os }}-" ];
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
          unit = {
            name = "Rust unit tests";
            needs = [ ];
            runtimeInputs = pkgs: [
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
            exec = ''
              ${rustRoot}

              if [ "''${GITHUB_ACTIONS:-}" = "true" ] \
                && [ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ] \
                && [ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns)" = "1" ]; then
                /usr/bin/sudo -n /usr/sbin/sysctl \
                  -w kernel.apparmor_restrict_unprivileged_userns=0 >/dev/null
              fi

              # `phenix-binding-lua-load` covers the host-linked LuaJIT module.
              timeout --signal=KILL 300 \
                cargo test --quiet --workspace --lib --bins --exclude phenix-binding-lua --locked
            '';
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

          adapter-domain = {
            name = "Adapter and domain tests";
            needs = [ ];
            runtimeInputs = pkgs: [
              pkgs.cargo
              pkgs.git
              pkgs.rustc
            ];
            exec = ''
              ${rustRoot}
              cargo test --quiet --locked \
                -p phenix-adapter-acp \
                -p phenix-domain \
                --tests
            '';
          };

          harness = {
            name = "Harness code tests";
            needs = [ ];
            runtimeInputs = pkgs: [
              pkgs.cargo
              pkgs.git
              pkgs.rustc
            ];
            exec = ''
              ${rustRoot}
              cargo test --quiet --locked -p phenix-harness \
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
          backend-acp = {
            name = "ACP backend integration";
            needs = [ ];
            runtimeInputs = pkgs: [
              pkgs.cargo
              pkgs.git
              pkgs.rustc
            ];
            exec = ''
              ${rustRoot}
              cargo test --quiet --locked -p phenix-backend-acp --tests
            '';
          };

          plugin-packaging = mkNixCheckSuite {
            check = "phenix-plugin-packaging";
            name = "Plugin packaging integration";
          };
        };

        product = {
          phenix-runtime = mkNixCheckSuite {
            check = "phenix-product-runtime-smoke";
            name = "Phenix supported runtime journey";
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
        };
        gitHooks = {
          enable = true;
          preCommit = [ "fix" ];
        };

        commands = ciCommands // {
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
                      pkgs.git
                      pkgs.jq
                    ];
                    exec = ''
                      ${repositoryRoot}
                      bash scripts/check-plugin-architecture.sh
                      bash scripts/check-spec-lifecycle-fixtures.sh
                      bash scripts/check-spec-lifecycle.sh
                    '';
                  };

                  structural-boundaries = {
                    description = "Canonical structural boundary enforcement";
                    ci = sourceCi // {
                      stepName = "Structural boundaries";
                    };
                    runtimeInputs = pkgs: [
                      pkgs.bash
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
                    description = "Committed GitHub workflow matches the Nix CI declaration";
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
                    '';
                  };
                };
              };

              rust = {
                description = "Rust static analysis with Clippy";
                ci = {
                  enable = true;
                  stage = "clippy";
                  name = "Clippy";
                  stepName = "Clippy";
                  timeoutMinutes = 60;
                };
                runtimeInputs = pkgs: [
                  pkgs.cargo
                  pkgs.clippy
                  pkgs.git
                  pkgs.rustc
                ];
                exec = ''
                  ${rustRoot}
                  cargo clippy --quiet --workspace --all-targets --locked -- -D warnings
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
