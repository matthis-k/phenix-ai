{ self, ... }: {
  perSystem =
    { pkgs, system, ... }:
    let
      rustSource = pkgs.lib.cleanSource ../rust;
      productRustArtifacts = self.packages.${system}.phenix-product-rust-artifacts;

      phenixHarnessRuntime = pkgs.runCommand "phenix-harness-runtime" { } ''
        mkdir -p "$out/bin"
        cp "${productRustArtifacts}/bin/phenix-harness" "$out/bin/phenix-harness"
        ln -s phenix-harness "$out/bin/phenix"
      '';

      phenixAcpFixture = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-acp-fixture";
        version = "0";
        src = rustSource;

        cargoLock.lockFile = ../rust/Cargo.lock;
        cargoBuildFlags = [
          "--package"
          "phenix-harness"
          "--bin"
          "phenix-acp-fixture"
        ];
        doCheck = false;

        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin"
          acp_binary="$(find target -path '*/release/phenix-acp-fixture' -type f -print -quit)"
          test -n "$acp_binary"
          cp "$acp_binary" "$out/bin/phenix-acp-fixture"
          runHook postInstall
        '';
      };

      runtimeConfig = pkgs.writeText "phenix-runtime.json" (
        builtins.toJSON (import ../config/phenix/runtime.nix)
      );

      phenixHarnessResources = pkgs.runCommand "phenix-harness-resources" { } ''
        mkdir -p "$out/share/phenix/licenses"
        cp ${runtimeConfig} "$out/share/phenix/runtime.json"
        cp ${../config/phenix/NOTICE.md} "$out/share/phenix/NOTICE.md"
        cp ${../rust/crates/phenix-plugin-basic-skills/skills/pstack-LICENSE} \
          "$out/share/phenix/licenses/pstack-LICENSE"
      '';

      supportedPhenix = self.packages.${system}.phenix;
      standaloneRuntime = self.packages.${system}.phenix-runtime;
      phenixProductRuntimeSmoke =
        pkgs.runCommand "phenix-product-runtime-smoke"
          {
            nativeBuildInputs = [
              supportedPhenix
              pkgs.jq
            ];
          }
          ''
            export PHENIX_STATE_DB="$TMPDIR/product-smoke.sqlite"
            printf '%s\n' \
              '{"id":1,"service":"phenix.sessions@1","input":{"type":"variant","value":{"tag":"Create","value":{"type":"table","value":{"session":{"type":"table","value":{"id":{"type":"string","value":"product-smoke"},"working_directory":{"type":"option","value":null},"title":{"type":"option","value":null},"lifecycle":{"type":"variant","value":{"tag":"Open","value":{"type":"unit"}}}}}}}}}}' \
              '{"id":2,"service":"phenix.sessions@1","input":{"type":"variant","value":{"tag":"Get","value":{"type":"table","value":{"id":{"type":"string","value":"product-smoke"}}}}}}' \
              | ${supportedPhenix}/bin/phenix --mode jsonl > "$TMPDIR/product-smoke.jsonl"
            if ! jq -se '
              length == 2
              and .[0].id == 1
              and .[0].status == "ok"
              and .[0].output.type == "variant"
              and .[0].output.value.tag == "Created"
              and (.[0].output | tostring | contains("product-smoke"))
              and .[1].id == 2
              and .[1].status == "ok"
              and .[1].output.type == "variant"
              and .[1].output.value.tag == "Session"
              and (.[1].output | tostring | contains("product-smoke"))
            ' "$TMPDIR/product-smoke.jsonl" >/dev/null; then
              cat "$TMPDIR/product-smoke.jsonl" >&2
              exit 1
            fi

            test -f ${supportedPhenix}/share/phenix/runtime.json
            test -f ${supportedPhenix}/share/phenix/licenses/pstack-LICENSE
            test -f ${supportedPhenix}/share/phenix/NOTICE.md

            touch "$out"
          '';

      phenixStandaloneRuntimeSmoke =
        pkgs.runCommand "phenix-product-standalone-runtime-smoke"
          {
            nativeBuildInputs = [
              standaloneRuntime
              pkgs.jq
            ];
          }
          ''
            phenix-runtime --list-services > "$TMPDIR/runtime-services.json"
            jq -e '
              (.plugins | type == "array")
              and (.services | type == "array")
            ' "$TMPDIR/runtime-services.json" >/dev/null
            touch "$out"
          '';

      phenixProductLuaSmoke = pkgs.runCommand "phenix-product-lua-smoke" { } ''
        test -f ${self.checks.${system}.phenix-binding-lua-observable-callback}
        touch "$out"
      '';
    in
    {
      packages = {
        phenix-acp-fixture = phenixAcpFixture;
        phenix-harness-runtime = phenixHarnessRuntime;
        phenix-harness-resources = phenixHarnessResources;
      };

      checks = {
        phenix-product-runtime-smoke = phenixProductRuntimeSmoke;
        phenix-product-standalone-runtime-smoke = phenixStandaloneRuntimeSmoke;
        phenix-product-lua-smoke = phenixProductLuaSmoke;
      };
    };
}
