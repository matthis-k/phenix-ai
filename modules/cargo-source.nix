{
  pkgs,
  rustRoot ? ../rust,
}:
let
  crateEntries = builtins.readDir (rustRoot + "/crates");
  localCrates = map (name: "crates/${name}") (
    builtins.filter (name: crateEntries.${name} == "directory") (builtins.attrNames crateEntries)
  );

  manifestPaths = [
    "Cargo.toml"
    "Cargo.lock"
  ]
  ++ map (crate: "${crate}/Cargo.toml") localCrates;

  manifestSources = map (relative: {
    inherit relative;
    source = pkgs.writeText "phenix-dependency-${
      builtins.replaceStrings [ "/" "." ] [ "-" "-" ] relative
    }" (builtins.readFile (rustRoot + "/${relative}"));
  }) manifestPaths;

  # Cargo manifests define path dependency edges. Parse them at evaluation time
  # rather than maintaining a separate dependency list.
  workspaceManifest = builtins.fromTOML (builtins.readFile (rustRoot + "/Cargo.toml"));
  workspaceDependencies = workspaceManifest.workspace.dependencies or { };
  manifestIndex = builtins.listToAttrs (
    map (
      member:
      let
        manifest = builtins.fromTOML (builtins.readFile (rustRoot + "/${member}/Cargo.toml"));
      in
      {
        name = manifest.package.name;
        value = {
          inherit manifest member;
        };
      }
    ) localCrates
  );

  dependencySets =
    manifest:
    [
      (manifest.dependencies or { })
      (manifest."build-dependencies" or { })
      (manifest."dev-dependencies" or { })
    ]
    ++ pkgs.lib.concatMap (target: [
      (target.dependencies or { })
      (target."build-dependencies" or { })
      (target."dev-dependencies" or { })
    ]) (builtins.attrValues (manifest.target or { }));

  localDependencies =
    manifest:
    pkgs.lib.unique (
      pkgs.lib.concatMap (
        declarations:
        pkgs.lib.concatMap (
          alias:
          let
            declared = builtins.getAttr alias declarations;
            inherited =
              if builtins.isAttrs declared && (declared.workspace or false) then
                builtins.getAttr alias workspaceDependencies
              else
                declared;
          in
          if builtins.isAttrs inherited && inherited ? path then [ (inherited.package or alias) ] else [ ]
        ) (builtins.attrNames declarations)
      ) (dependencySets manifest)
    );

  membersFor =
    root:
    let
      visit =
        seen: pending:
        if pending == [ ] then
          seen
        else
          let
            package = builtins.head pending;
            entry =
              if builtins.hasAttr package manifestIndex then
                builtins.getAttr package manifestIndex
              else
                throw "Unknown Cargo path dependency ${package} in the requested package closure";
          in
          if builtins.elem package seen then
            visit seen (builtins.tail pending)
          else
            visit (seen ++ [ package ]) ((builtins.tail pending) ++ localDependencies entry.manifest);
    in
    map (package: (builtins.getAttr package manifestIndex).member) (visit [ ] [ root ]);

  explicitTargetPaths =
    member:
    let
      manifest = builtins.fromTOML (builtins.readFile (rustRoot + "/${member}/Cargo.toml"));
      pathFrom = target: if target ? path then [ "${member}/${target.path}" ] else [ ];
    in
    (if manifest ? lib then pathFrom manifest.lib else [ ])
    ++ builtins.concatLists (map pathFrom (manifest.bin or [ ]))
    ++ builtins.concatLists (map pathFrom (manifest.example or [ ]))
    ++ builtins.concatLists (map pathFrom (manifest.test or [ ]))
    ++ builtins.concatLists (map pathFrom (manifest.bench or [ ]));

  existingTargetPaths =
    paths: builtins.filter (relative: builtins.pathExists (rustRoot + "/${relative}")) paths;

  targetPaths = builtins.concatLists (
    map (
      member:
      existingTargetPaths [
        "${member}/src/lib.rs"
        "${member}/src/main.rs"
        "${member}/build.rs"
      ]
      ++ explicitTargetPaths member
    ) localCrates
  );

  executablePlaceholderPaths = builtins.concatLists (
    map (
      member:
      existingTargetPaths [
        "${member}/src/main.rs"
        "${member}/build.rs"
      ]
    ) localCrates
  );

  dependencySkeleton = pkgs.runCommand "phenix-rust-dependency-skeleton" { } ''
    set -euo pipefail
    mkdir -p "$out"

    ${pkgs.lib.concatMapStringsSep "\n" (entry: ''
      mkdir -p "$out/${builtins.dirOf entry.relative}"
      cp ${entry.source} "$out/${entry.relative}"
    '') manifestSources}

    ${pkgs.lib.concatMapStringsSep "\n" (relative: ''
      mkdir -p "$out/${builtins.dirOf relative}"
      : > "$out/${relative}"
    '') targetPaths}

    ${pkgs.lib.concatMapStringsSep "\n" (relative: ''
      printf '%s\n' 'fn main() {}' > "$out/${relative}"
    '') executablePlaceholderPaths}
  '';

  # Keep workspace manifests and empty targets from the dependency skeleton.
  # Copy real files only for crates reachable from the requested package.
  sourceFor =
    root:
    pkgs.runCommand "phenix-${root}-selected-rust-source" { } ''
      set -euo pipefail
      mkdir -p "$out"
      cp -a ${dependencySkeleton}/. "$out/"
      chmod -R u+w "$out"

      ${pkgs.lib.concatMapStringsSep "\n" (member: ''
        rm -rf "$out/${member}"
        cp -a ${rustRoot + "/${member}"} "$out/${member}"
      '') (membersFor root)}
    '';

in
{
  inherit dependencySkeleton membersFor sourceFor;
}
