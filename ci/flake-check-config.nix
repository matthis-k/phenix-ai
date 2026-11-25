{
  schemaVersion = 1;
  system = "x86_64-linux";
  shardCount = 3;
  ci = {
    trusted = {
      repository = "matthis-k/phenix-ai";
      branches = [ "main" ];
      pullRequestAuthors = [ "matthis-k" ];
      backend = "nix-remote";
      workflowDispatch = true;
      remote = {
        transport = "tailscale";
        credentials = "oidc";
        workers = 4;
        slotsPerWorker = 2;
      };
    };
    pullRequest.backend = "nix-dag";
  };
  strategy = "generic";
  strategyOptions = { };
  checkHints = { };
  prebuildTargets = [ ];
  cache = {
    keyPrefix = "phenix-checks";
    keyFiles = [
      "flake.lock"
      "Cargo.lock"
    ];
  };
}
