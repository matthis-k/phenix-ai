{ self, ... }:
{
  perSystem =
    { pkgs, config, ... }:
    let
      luaBinding = config.packages.phenix-binding-lua;
      phenixAcp = config.packages.phenix-acp;
      revision = self.rev or self.dirtyRev or "unknown";
      frontendSource = pkgs.lib.cleanSource ../clients/nvim;
      nvimClient = pkgs.vimUtils.buildVimPlugin {
        pname = "phenix-nvim";
        version = "0";
        src = frontendSource;
        postInstall = ''
          install -Dm755 ${luaBinding}/lib/lua/5.1/phenix.so "$out/lua/phenix.so"
          substituteInPlace "$out/lua/phenix_nvim/config.lua" \
            --replace-fail 'command = "phenix-acp"' \
            'command = "${phenixAcp}/bin/phenix-acp"'
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
          pkgs.runCommand "phenix-nvim-load-check"
            {
              nativeBuildInputs = [
                pkgs.neovim
                phenixAcp
              ];
            }
            ''
              test ! -e ${frontendSource}/lua/phenix/init.lua
              test "$(grep -R -l 'require(\"phenix\")' ${frontendSource}/lua | wc -l)" -eq 1
              if grep -R '_phenix/' ${frontendSource}/lua; then
                echo "frontend Lua must not contain raw Phenix wire method ids" >&2
                exit 1
              fi
              grep -F ${pkgs.lib.escapeShellArg "command = \"${phenixAcp}/bin/phenix-acp\""} \
                ${nvimClient}/lua/phenix_nvim/config.lua >/dev/null

              nvim --headless -u NONE \
                --cmd ${pkgs.lib.escapeShellArg "set rtp^=${nvimClient}"} \
                -c ${pkgs.lib.escapeShellArg "lua dofile('${frontendSource}/tests/headless.lua')"} \
                -c qa

              nvim --headless -u NONE \
                --cmd ${pkgs.lib.escapeShellArg "set rtp^=${nvimClient}"} \
                -c ${pkgs.lib.escapeShellArg "lua dofile('${frontendSource}/tests/image.lua')"} \
                -c qa

              export PHENIX_STATE_DB="$TMPDIR/phenix-nvim-acp.sqlite"
              nvim --headless -u NONE \
                --cmd ${pkgs.lib.escapeShellArg "set rtp^=${nvimClient}"} \
                -c ${pkgs.lib.escapeShellArg ''lua local frontend = require("phenix_nvim"); frontend.setup({ auto_connect = false }); local connected = false; local failure = nil; frontend.connect(function(_, err) failure = err; connected = true end); assert(vim.wait(10000, function() return connected end, 10), "packaged phenix-acp connection timed out"); assert(failure == nil, vim.inspect(failure)); local created = false; frontend.new_session(); assert(vim.wait(10000, function() return require("phenix_nvim.runtime").active_session() ~= nil end, 10), "packaged session creation timed out"); frontend.disconnect()''} \
                -c qa
              test -s "$PHENIX_STATE_DB"
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