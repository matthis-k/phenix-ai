# Experimental first-party Cargo-artifact reuse backend. Not deployed by default.
# This package is intentionally separate from the production phenix-core output.
{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    let
      craneLib = inputs.crane.mkLib pkgs;
      cargoSources = import ./cargo-source.nix { inherit pkgs; };
      # Identical unpack directory names keep Cargo's path fingerprints
      # stable between the core and downstream workspace source closures.
      namedSource =
        name:
        pkgs.runCommand "phenix-crane-reuse-source" { } ''
          mkdir -p "$out"
          cp -a ${cargoSources.sourceFor name}/. "$out/"
        '';
      common = {
        pname = "phenix-crane-core-probe";
        version = "0";
        src = namedSource "phenix-core";
        cargoLock = ../rust/Cargo.lock;
        cargoExtraArgs = "--package phenix-core";
        doCheck = false;
        strictDeps = true;
      };

      # Manifest and dummy-target inputs give the dependency build an identity
      # independent of changes to first-party source files.
      dependencies = craneLib.buildDepsOnly (
        common
        // {
          pname = "phenix-crane-core-deps";
          src = cargoSources.dependencySkeleton;
          # Only release build artifacts are imported downstream. Crane's
          # default check-then-build compiles the dummy graph twice.
          buildPhaseCargoCommand = ''
            cargoWithProfile build --locked --package phenix-core
          '';
        }
      );
      # Cargo progress text can be suppressed, while structured artifacts
      # record an actual compilation even in quiet mode. A "fresh" artifact
      # is reused from Cargo's cache and must not count as a rebuild.
      rejectCoreRebuild = ''
        if ${pkgs.jq}/bin/jq -e '
          select(.reason == "compiler-artifact"
                 and .target.name == "phenix_core"
                 and .fresh == false)
        ' "$cargoBuildLog" >/dev/null; then
          echo "Previously compiled phenix-core was rebuilt instead of reused" >&2
          exit 1
        fi
      '';
      # Package the reusable compiled core; the final package must not
      # compile the same first-party crate a second time.
      artifact = craneLib.buildPackage (
        common
        // {
          cargoArtifacts = coreArtifacts;
          buildPhaseCargoCommand = ''
            cargoBuildLog=$(mktemp cargoBuildLogXXXX.json)
            cargoWithProfile build --locked --package phenix-core \
              --message-format json-render-diagnostics >"$cargoBuildLog" 2>cargo-build-stderr
            cat cargo-build-stderr >&2
            ${rejectCoreRebuild}
          '';
          installPhase = ''
            runHook preInstall
            mkdir -p "$out/share/phenix-rust-package"
            printf '%s\n' phenix-core > "$out/share/phenix-rust-package/name"
            runHook postInstall
          '';
        }
      );
      # Unlike buildPackage's installed library output, this derivation
      # exports the compiled real core's Cargo target archive for reuse.
      coreArtifacts = craneLib.mkCargoDerivation (
        common
        // {
          pname = "phenix-crane-core-reusable";
          cargoArtifacts = dependencies;
          buildPhaseCargoCommand = ''
            cargoWithProfile build --locked --package phenix-core
            test -n "$(find "''${CARGO_TARGET_DIR:-target}" -name 'libphenix_core-*.rlib' -print -quit)"
          '';
          doInstallCargoArtifacts = true;
        }
      );

      # Build a real dependent crate from its own, larger source closure.
      # A Cargo compile of phenix-core here means cross-package reuse failed.
      consumer = craneLib.buildPackage {
        pname = "phenix-crane-application-interface-probe";
        version = "0";
        src = namedSource "phenix-application-interface";
        cargoLock = ../rust/Cargo.lock;
        cargoArtifacts = coreArtifacts;
        cargoExtraArgs = "--package phenix-application-interface";
        doCheck = false;
        strictDeps = true;
        buildPhaseCargoCommand = ''
          cargoBuildLog=$(mktemp cargoBuildLogXXXX.json)
          cargoWithProfile build --locked --package phenix-application-interface \
            --message-format json-render-diagnostics >"$cargoBuildLog" 2>cargo-build-stderr
          cat cargo-build-stderr >&2
            ${rejectCoreRebuild}
          # Require a real downstream compilation, not only a cached artifact.
          if ! ${pkgs.jq}/bin/jq -e '
            select(.reason == "compiler-artifact"
                   and .target.name == "phenix_application_interface"
                   and .fresh == false)
          ' "$cargoBuildLog" >/dev/null; then
            echo "Downstream crate did not compile in the reuse probe" >&2
            exit 1
          fi
          test -n "$(find "''${CARGO_TARGET_DIR:-target}" -name 'libphenix_application_interface-*.rlib' -print -quit)"
          test -n "$(find "''${CARGO_TARGET_DIR:-target}" -path '*/release/phenix-application-descriptor' -type f -print -quit)"
        '';
        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin" "$out/share/phenix/interfaces"
          executable="$(find "''${CARGO_TARGET_DIR:-target}" -path '*/release/phenix-application-descriptor' -type f -print -quit)"
          test -n "$executable"
          cp "$executable" "$out/bin/phenix-application-descriptor"
          "$out/bin/phenix-application-descriptor" \
            > "$out/share/phenix/interfaces/phenix.application@1.json"
          runHook postInstall
        '';
      };
      # Test whether the same compiled core survives a larger real product
      # closure rather than only the small application-interface library.
      runtime = craneLib.buildPackage {
        pname = "phenix-crane-runtime-probe";
        version = "0";
        src = namedSource "phenix-runtime";
        cargoLock = ../rust/Cargo.lock;
        cargoArtifacts = coreArtifacts;
        cargoExtraArgs = "--package phenix-runtime --bin phenix-runtime";
        doCheck = false;
        strictDeps = true;
        buildPhaseCargoCommand = ''
          cargoBuildLog=$(mktemp cargoBuildLogXXXX.json)
          cargoWithProfile build --locked --package phenix-runtime --bin phenix-runtime \
            --message-format json-render-diagnostics >"$cargoBuildLog" 2>cargo-build-stderr
          cat cargo-build-stderr >&2
            ${rejectCoreRebuild}
          test -n "$(find "''${CARGO_TARGET_DIR:-target}" -path '*/release/phenix-runtime' -type f -print -quit)"
        '';
        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin"
          executable="$(find "''${CARGO_TARGET_DIR:-target}" -path '*/release/phenix-runtime' -type f -print -quit)"
          test -n "$executable"
          cp "$executable" "$out/bin/phenix-runtime"
          runHook postInstall
        '';
      };
    in
    {
      packages = {
        phenix-crane-core-deps-probe = dependencies;
        phenix-crane-core-probe = artifact;
        phenix-crane-core-artifacts-probe = coreArtifacts;
        phenix-crane-application-interface-probe = consumer;
        phenix-crane-runtime-probe = runtime;
      };
    };
}
