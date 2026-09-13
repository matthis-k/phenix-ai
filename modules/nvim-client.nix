{ self, ... }:
{
  perSystem =
    { pkgs, config, ... }:
    let
      luaBinding = config.packages.phenix-binding-lua;
      revision = self.rev or self.dirtyRev or "unknown";
      frontendSource = pkgs.lib.cleanSource ../clients/nvim;
      nvimClient = pkgs.vimUtils.buildVimPlugin {
        pname = "phenix-nvim";
        version = "0";
        src = frontendSource;
        postInstall = ''
          install -Dm755 ${luaBinding}/lib/lua/5.1/phenix.so "$out/lua/phenix.so"
          mkdir -p "$out/share/phenix-nvim"
          printf '%s\n' ${pkgs.lib.escapeShellArg revision} > "$out/share/phenix-nvim/conductor-revision"
        '';
      };
      frontendExport = pkgs.runCommand "phenix-nvim-export" { } ''
        mkdir -p "$out"
        cp -R ${frontendSource}/. "$out/"
        chmod -R u+w "$out"
        printf '%s\n' ${pkgs.lib.escapeShellArg revision} > "$out/.phenix-conductor-revision"
        test ! -e "$out/lua/phenix.so"
      '';
    in
    {
      packages = {
        phenix-nvim = nvimClient;
        phenix-nvim-export = frontendExport;
      };

      checks = {
        phenix-nvim-load =
          pkgs.runCommand "phenix-nvim-load-check" { nativeBuildInputs = [ pkgs.neovim ]; }
            ''
              test ! -e ${frontendSource}/lua/phenix/init.lua
              test "$(grep -R -l 'require(\"phenix\")' ${frontendSource}/lua | wc -l)" -eq 1
              if grep -R '_phenix/' ${frontendSource}/lua; then
                echo "frontend Lua must not contain raw Phenix wire method ids" >&2
                exit 1
              fi
              nvim --headless -u NONE \
                --cmd ${pkgs.lib.escapeShellArg "set rtp^=${nvimClient}"} \
                -c ${pkgs.lib.escapeShellArg "lua dofile('${frontendSource}/tests/headless.lua')"} \
                -c qa
              touch "$out"
            '';
        phenix-nvim-export = pkgs.runCommand "phenix-nvim-export-check" { } ''
          test -f ${frontendExport}/lua/phenix_nvim/init.lua
          test -f ${frontendExport}/.phenix-conductor-revision
          test ! -e ${frontendExport}/lua/phenix.so
          touch "$out"
        '';
      };
    };
}
