_: {
  perSystem =
    { pkgs, ... }:
    let
      rustRoot = ../rust;
      rustSource = selectedProductSource;
      cargoSource = import ./cargo-source.nix { inherit pkgs rustRoot; };
      inherit (cargoSource) dependencySkeleton;
      productClosure = cargoSource.membersFor "phenix-harness";
      selectedProductSource = cargoSource.sourceFor "phenix-harness";

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
        passthru.productCargoSourceMembers = productClosure;
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
