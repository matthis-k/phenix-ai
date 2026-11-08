{
  schemaVersion = 1;
  system = "x86_64-linux";
  shardCount = 3;
  ci.backend = "matrix";
  strategy = "generic";
  strategyOptions = { };
  checkHints = { };
  # Same corpus derivations all generated Nix check groups reference.
  # The matrix backend publishes these two closures once, then its shards
  # only execute precompiled test binaries without invoking Cargo.
  prebuildTargets = [
    ".#packages.x86_64-linux.cargo-tests-default"
    ".#packages.x86_64-linux.cargo-tests-no-default"
  ];
  cache = {
    keyPrefix = "phenix-checks";
    keyFiles = [
      "flake.lock"
      "Cargo.lock"
    ];
  };
}
