_: {
  perSystem =
    { pkgs, ... }:
    let
      rustRoot = ../rust;
      rustSource = pkgs.lib.cleanSource rustRoot;
      crateEntries = builtins.readDir (rustRoot + "/crates");
      localCrates = map (name: "crates/${name}") (
        builtins.filter (name: crateEntries.${name} == "directory") (builtins.attrNames crateEntries)
      );

      manifestPaths = [
        "Cargo.toml"
        "Cargo.lock"
      ]
      ++ map (crate: "${crate}/Cargo.toml") localCrates;

      manifestSources = map (relative: {
        inherit relative;
        source = pkgs.writeText "phenix-dependency-${
          builtins.replaceStrings [ "/" "." ] [ "-" "-" ] relative
        }" (builtins.readFile (rustRoot + "/${relative}"));
      }) manifestPaths;

      explicitTargetPaths =
        member:
        let
          manifest = builtins.fromTOML (builtins.readFile (rustRoot + "/${member}/Cargo.toml"));
          pathFrom = target: if target ? path then [ "${member}/${target.path}" ] else [ ];
        in
        (if manifest ? lib then pathFrom manifest.lib else [ ])
        ++ builtins.concatLists (map pathFrom (manifest.bin or [ ]))
        ++ builtins.concatLists (map pathFrom (manifest.example or [ ]))
        ++ builtins.concatLists (map pathFrom (manifest.test or [ ]))
        ++ builtins.concatLists (map pathFrom (manifest.bench or [ ]));

      existingTargetPaths =
        paths: builtins.filter (relative: builtins.pathExists (rustRoot + "/${relative}")) paths;

      targetPaths = builtins.concatLists (
        map (
          member:
          existingTargetPaths [
            "${member}/src/lib.rs"
            "${member}/src/main.rs"
            "${member}/build.rs"
          ]
          ++ explicitTargetPaths member
        ) localCrates
      );

      executablePlaceholderPaths = builtins.concatLists (
        map (
          member:
          existingTargetPaths [
            "${member}/src/main.rs"
            "${member}/build.rs"
          ]
        ) localCrates
      );

      dependencySkeleton = pkgs.runCommand "phenix-rust-dependency-skeleton" { } ''
        set -euo pipefail
        mkdir -p "$out"

        ${pkgs.lib.concatMapStringsSep "\n" (entry: ''
          mkdir -p "$out/${builtins.dirOf entry.relative}"
          cp ${entry.source} "$out/${entry.relative}"
        '') manifestSources}

        ${pkgs.lib.concatMapStringsSep "\n" (relative: ''
          mkdir -p "$out/${builtins.dirOf relative}"
          : > "$out/${relative}"
        '') targetPaths}

        ${pkgs.lib.concatMapStringsSep "\n" (relative: ''
          printf '%s\n' 'fn main() {}' > "$out/${relative}"
        '') executablePlaceholderPaths}
      '';

      productRustDependencies = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-product-rust-dependencies";
        version = "0";
        src = dependencySkeleton;
        cargoLock.lockFile = rustRoot + "/Cargo.lock";
        nativeBuildInputs = [ pkgs.mold ];
        RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
        doCheck = false;
        dontFixup = true;

        buildPhase = ''
          runHook preBuild
          cargo build --release --locked --offline \
            --package phenix-harness \
            --bin phenix-harness
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          mkdir -p "$out"
          cp -a target "$out/target"
          runHook postInstall
        '';
      };

      productRustArtifacts = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-product-rust-artifacts";
        version = "0";
        src = rustSource;
        cargoLock.lockFile = rustRoot + "/Cargo.lock";
        nativeBuildInputs = [ pkgs.mold ];
        RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
        doCheck = false;

        buildPhase = ''
          runHook preBuild

          cp -a ${productRustDependencies}/target ./target
          chmod -R u+w target

          # The dependency skeleton compiled empty local crates to materialize the
          # external graph. Make every real workspace source newer so Cargo rebuilds
          # Phenix crates while reusing those external artifacts.
          find . -type f \
            \( -name '*.rs' -o -name 'Cargo.toml' \) \
            -exec touch {} +

          cargo build --release --locked \
            --package phenix-harness \
            --bin phenix-harness
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin"
          built_binary="$(find target -path '*/release/phenix-harness' -type f -print -quit)"
          test -n "$built_binary"
          cp "$built_binary" "$out/bin/phenix-harness"
          runHook postInstall
        '';
      };
    in
    {
      packages = {
        phenix-product-rust-dependencies = productRustDependencies;
        phenix-product-rust-artifacts = productRustArtifacts;
      };
    };
}
