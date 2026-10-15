{
  description = "External Phenix preset override consumer";
  inputs.phenix.url = "path:../..";

  outputs = { self, phenix }:
    let
      eachSystem = build: builtins.listToAttrs (builtins.map (system: {
        name = system;
        value = build system;
      }) (builtins.attrNames phenix.packages));
    in
    {
      packages = eachSystem (system:
        let
          packages = phenix.packages.${system};
          base = packages.phenix-advanced;
          httpModel = packages.phenix-plugin-model-http;
        in
        {
          default = base.override (old: {
            artifacts = old.artifacts ++ [ "${httpModel}" ];
            slots = builtins.map (binding:
              if binding.caller == "phenix.basic.agent" && binding.slot == "Model"
              then binding // { provider = "phenix.model.http"; }
              else binding
            ) old.slots ++ [
              {
                caller = "phenix.model.http";
                slot = "Journal";
                index = 0;
                provider = "phenix.basic.event-journal";
                export = "Journal";
              }
            ];
            grants = old.grants ++ [
              {
                caller = "phenix.model.http";
                interface = "phenix.basic.journal@1";
                operation = "append";
                resources = [ "phenix.session.s1" ];
              }
            ];
          });
        });
    };
}
