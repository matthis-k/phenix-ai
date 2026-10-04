_: {
  perSystem =
    { pkgs, ... }:
    let
      rustSource = pkgs.lib.cleanSource ../rust;

      productRustArtifacts = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-product-rust-artifacts";
        version = "0";
        src = rustSource;
        cargoLock.lockFile = ../rust/Cargo.lock;
        # Keep release optimization; only replace the linker for this CI timing candidate.
        nativeBuildInputs = [ pkgs.mold ];
        RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
        doCheck = false;

        buildPhase = ''
          runHook preBuild
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
      packages.phenix-product-rust-artifacts = productRustArtifacts;
    };
}
