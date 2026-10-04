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
            --bin phenix-harness \
            --bin phenix-acp-fixture
          cargo build --release --locked \
            --package phenix-runtime \
            --bin phenix-runtime
          cargo build --release --locked \
            --package phenix-application-interface \
            --bin phenix-application-descriptor
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin"
          for binary in \
            phenix-harness \
            phenix-acp-fixture \
            phenix-runtime \
            phenix-application-descriptor
          do
            built_binary="$(find target -path "*/release/$binary" -type f -print -quit)"
            test -n "$built_binary"
            cp "$built_binary" "$out/bin/$binary"
          done
          runHook postInstall
        '';
      };

      luaRustArtifacts = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-lua-rust-artifacts";
        version = "0";
        src = rustSource;
        cargoLock.lockFile = ../rust/Cargo.lock;
        doCheck = false;

        buildPhase = ''
          runHook preBuild
          cargo build --release --locked --package phenix-binding-lua
          cargo build --release --locked --package phenix-acp-stdio \
            --example observable_callback_fixture
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          module="$(find target -path '*/release/libphenix.so' -type f -print -quit)"
          fixture="$(find target -path '*/release/examples/observable_callback_fixture' -type f -print -quit)"
          test -n "$module"
          test -n "$fixture"
          mkdir -p "$out/lib/lua/5.1" "$out/bin"
          cp "$module" "$out/lib/lua/5.1/phenix.so"
          cp "$fixture" "$out/bin/observable_callback_fixture"
          runHook postInstall
        '';
      };
    in
    {
      packages = {
        phenix-product-rust-artifacts = productRustArtifacts;
        phenix-lua-rust-artifacts = luaRustArtifacts;
      };
    };
}
