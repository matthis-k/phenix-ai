{ inputs, ... }:

{
  perSystem =
    { system, ... }:
    {
      phenixWrapped = {
        phenix = inputs.self.packages.${system}.phenix;
        kernel = inputs.self.packages.${system}.phenix-core;
        runtime = inputs.self.packages.${system}.phenix-runtime;
        harness = inputs.self.packages.${system}.phenix-harness;
        stitch = inputs.self.packages.${system}.stitch;
        stitchMcp = inputs.self.packages.${system}.stitch-mcp;
      };
    };
}
