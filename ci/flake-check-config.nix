{
  schemaVersion = 1;
  system = "x86_64-linux";
  shardCount = 3;
  ci.backend = "matrix";
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
