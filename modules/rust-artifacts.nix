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
        doCheck = false;

        buildPhase = ''
          runHook preBuild
          cargo build --release --locked \
            --package phenix-harness \
            --bin phenix-harness
          cargo build --release --locked \
            --package phenix-runtime \
            --bin phenix-runtime
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin"
          for binary in phenix-harness phenix-runtime; do
            built_binary="$(find target -path "*/release/$binary" -type f -print -quit)"
            test -n "$built_binary"
            cp "$built_binary" "$out/bin/$binary"
          done
          runHook postInstall
        '';
      };
    in
    {
      packages.phenix-product-rust-artifacts = productRustArtifacts;
    };
}
