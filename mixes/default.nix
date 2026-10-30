# Explicit selection of production presets versus isolated domain testing mixes.
# Only the selected test domain changes a product composition.
{ lib }:
{
  production = {
    basic = import ./production/basic.nix { inherit lib; };
    advanced = import ./production/advanced.nix { inherit lib; };
  };
  testing = {
    modelHistory = import ./testing/model.nix;
    providerLoopback = import ./testing/providers.nix;
    graph = import ./testing/graph.nix;
    storage = import ./testing/storage.nix;
    frontend = import ./testing/frontend.nix;
    advanced = import ./testing/advanced.nix;
    pythonPoc = import ./testing/python-poc.nix;
  };
}
