_: {
  perSystem =
    { pkgs, ... }:
    let
      rustRoot = ../rust;
      rustSource = selectedProductSource;
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

      # Cargo manifests define path dependency edges. Parse them at evaluation time
      # rather than maintaining another list of packages needed by the product.
      workspaceManifest = builtins.fromTOML (builtins.readFile (rustRoot + "/Cargo.toml"));
      workspaceDependencies = workspaceManifest.workspace.dependencies or { };
      manifestIndex = builtins.listToAttrs (
        map (
          member:
          let
            manifest = builtins.fromTOML (builtins.readFile (rustRoot + "/${member}/Cargo.toml"));
          in
          {
            name = manifest.package.name;
            value = {
              inherit manifest member;
            };
          }
        ) localCrates
      );

      dependencySets =
        manifest:
        [
          (manifest.dependencies or { })
          (manifest."build-dependencies" or { })
          (manifest."dev-dependencies" or { })
        ]
        ++ pkgs.lib.concatMap (
          target:
          [
            (target.dependencies or { })
            (target."build-dependencies" or { })
            (target."dev-dependencies" or { })
          ]
        ) (builtins.attrValues (manifest.target or { }));

      localDependencies =
        manifest:
        pkgs.lib.unique (
          pkgs.lib.concatMap (
            declarations:
            pkgs.lib.concatMap (
              alias:
              let
                declared = builtins.getAttr alias declarations;
                inherited =
                  if builtins.isAttrs declared && (declared.workspace or false) then
                    builtins.getAttr alias workspaceDependencies
                  else
                    declared;
              in
              if builtins.isAttrs inherited && inherited ? path then
                [ (inherited.package or alias) ]
              else
                [ ]
            ) (builtins.attrNames declarations)
          ) (dependencySets manifest)
        );

      productClosure =
        let
          visit =
            seen: pending:
            if pending == [ ] then
              seen
            else
              let
                package = builtins.head pending;
                entry =
                  if builtins.hasAttr package manifestIndex then
                    builtins.getAttr package manifestIndex
                  else
                    throw "Unknown Cargo path dependency ${package} in the Phenix product closure";
              in
              if builtins.elem package seen then
                visit seen (builtins.tail pending)
              else
                visit (seen ++ [ package ]) (
                  (builtins.tail pending) ++ localDependencies entry.manifest
                );
        in
        map (package: (builtins.getAttr package manifestIndex).member) (
          visit [ ] [ "phenix-harness" ]
        );

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

      # Keep workspace manifests and empty targets from the dependency skeleton.
      # Copy real files only for crates reachable from the product.
      selectedProductSource = pkgs.runCommand "phenix-product-selected-rust-source" { } ''
        set -euo pipefail
        mkdir -p "$out"
        cp -a ${dependencySkeleton}/. "$out/"
        chmod -R u+w "$out"

        ${pkgs.lib.concatMapStringsSep "\n" (member: ''
          rm -rf "$out/${member}"
          cp -a ${rustRoot + "/${member}"} "$out/${member}"
        '') productClosure}
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
          # Cargo lock sentinels are process synchronization state, not reusable
          # artifacts. Nix store optimisation may hardlink identical empty lock
          # files; preserving those links into the next build can make Cargo
          # acquire two logical locks on one inode and block on itself.
          find "$out/target" -type f \
            \( -name '.cargo-lock' -o -name '.cargo-build-lock' -o -name '.cargo-artifact-lock' \) \
            -delete
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
          # Defend against older cached dependency outputs that still contain
          # Cargo's lock sentinels.
          find target -type f \
            \( -name '.cargo-lock' -o -name '.cargo-build-lock' -o -name '.cargo-artifact-lock' \) \
            -delete

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
