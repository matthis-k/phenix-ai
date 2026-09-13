{ self, ... }: {
  perSystem =
    { pkgs, system, ... }:
    let
      rustSource = pkgs.lib.cleanSource ../rust;

      phenixHarnessRuntime = pkgs.rustPlatform.buildRustPackage {
        pname = "phenix-harness-runtime";
        version = "0";
        src = rustSource;

        cargoLock.lockFile = ../rust/Cargo.lock;
        cargoBuildFlags = [
          "--package"
          "phenix-harness"
          "--bin"
          "phenix-harness"
        ];
        doCheck = false;

        installPhase = ''
          runHook preInstall
          mkdir -p "$out/bin"
          harness_binary="$(find target -path '*/release/phenix-harness' -type f -print -quit)"
          test -n "$harness_binary"
          cp "$harness_binary" "$out/bin/phenix-harness"
          ln -s phenix-harness "$out/bin/phenix"
          runHook postInstall
        '';
      };

      runtimeConfig = pkgs.writeText "phenix-runtime.json" (
        builtins.toJSON (import ../config/phenix/runtime.nix)
      );

      phenixHarnessResources = pkgs.runCommand "phenix-harness-resources" { } ''
        mkdir -p "$out/share/phenix/skills"
        cp ${runtimeConfig} "$out/share/phenix/runtime.json"
        cp -r ${../config/phenix/skills}/* "$out/share/phenix/skills/"
        cp ${../config/phenix/NOTICE.md} "$out/share/phenix/NOTICE.md"
      '';

      supportedPhenix = self.packages.${system}.phenix;
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
              '{"id":1,"service":"phenix.sessions@1","input":{"type":"variant","value":{"tag":"Create","value":{"type":"table","value":{"id":{"type":"string","value":"product-smoke"}}}}}}' \
              '{"id":2,"service":"phenix.sessions@1","input":{"type":"variant","value":{"tag":"Get","value":{"type":"table","value":{"id":{"type":"string","value":"product-smoke"}}}}}}' \
              | ${supportedPhenix}/bin/phenix-harness > "$TMPDIR/product-smoke.jsonl"
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
            test -f ${supportedPhenix}/share/phenix/skills/write/SKILL.md
            test -f ${supportedPhenix}/share/phenix/skills/pstack-LICENSE
            test -f ${supportedPhenix}/share/phenix/NOTICE.md

            touch "$out"
          '';

      phenixProductLuaSmoke = pkgs.runCommand "phenix-product-lua-smoke" { } ''
        # Exercise the host-linked module through its packaged ACP fixture.
        test -f ${self.checks.${system}.phenix-binding-lua-observable-callback}
        touch "$out"
      '';
    in
    {
      packages = {
        phenix-harness-runtime = phenixHarnessRuntime;
        phenix-harness-resources = phenixHarnessResources;
      };

      checks = {
        phenix-product-runtime-smoke = phenixProductRuntimeSmoke;
        phenix-product-lua-smoke = phenixProductLuaSmoke;
      };
    };
}
