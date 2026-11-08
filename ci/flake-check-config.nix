{
  schemaVersion = 1;
  system = "x86_64-linux";
  shardCount = 3;
  ci.backend = "matrix";
  strategy = "generic";
  strategyOptions = { };
  checkHints = { };
  # Discover ci-prebuild-* packages exported by the shared CI library.
  # No Cargo-derived installables or check inventory are duplicated here.
  autoPrebuild = true;
  prebuildTargets = [ ];
  cache = {
    keyPrefix = "phenix-checks";
    keyFiles = [
      "flake.lock"
      "Cargo.lock"
    ];
  };
}
