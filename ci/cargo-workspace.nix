{ lib, root }:
let
  fail = message: throw "Cargo workspace: ${message}";
  manifest = builtins.fromTOML (builtins.readFile (root + /Cargo.toml));
  workspace = manifest.workspace;
  validate =
    pattern:
    if
      !builtins.isString pattern
      || pattern == ""
      || lib.hasPrefix "/" pattern
      || builtins.any (
        part:
        builtins.elem part [
          ""
          "."
          ".."
        ]
      ) (lib.splitString "/" pattern)
      || builtins.match ".*[][{}\\\\].*" pattern != null
      || builtins.any (part: lib.hasInfix "**" part && part != "**") (lib.splitString "/" pattern)
    then
      fail "unsupported membership path or glob: ${builtins.toJSON pattern}"
    else
      pattern;
  componentMatches =
    pattern: name:
    builtins.match (lib.concatMapStrings (
      part:
      if builtins.isList part then
        if builtins.head part == "*" then ".*" else "."
      else
        lib.escapeRegex part
    ) (builtins.split "([*?])" pattern)) name != null;
  # Traverse only directories selected by the pattern, never symlink aliases.
  expand =
    pattern:
    let
      walk =
        relative: parts:
        let
          directory = root + "/${relative}";
          entries = builtins.readDir directory;
          children = builtins.filter (name: entries.${name} == "directory") (builtins.attrNames entries);
          aliases = builtins.filter (name: entries.${name} == "symlink" && componentMatches head name) (
            builtins.attrNames entries
          );
          append = name: if relative == "" then name else "${relative}/${name}";
          head = builtins.head parts;
          tail = builtins.tail parts;
        in
        if parts == [ ] then
          if (entries."Cargo.toml" or null) == "regular" then
            [ relative ]
          else
            fail "member has no regular package manifest: ${relative}"
        else if head == "**" then
          if aliases != [ ] then
            fail "symlink membership paths are unsupported: ${relative}"
          else
            walk relative tail ++ lib.concatMap (name: walk (append name) parts) children
        else if aliases != [ ] then
          fail "symlink membership paths are unsupported: ${relative}"
        else
          lib.concatMap (name: walk (append name) tail) (builtins.filter (componentMatches head) children);
      result = lib.unique (walk "" (lib.splitString "/" (validate pattern)));
    in
    if result == [ ] then fail "membership path/glob matches no package: ${pattern}" else result;
  declared = lib.unique (lib.concatMap expand workspace.members);
  # Cargo exclusions are literal directory prefixes, not member globs.
  exclusions = workspace.exclude or [ ];
  isExcluded = path: builtins.any (entry: path == entry || lib.hasPrefix "${entry}/" path) exclusions;
  excluded = builtins.filter isExcluded declared;
  members = builtins.sort builtins.lessThan (
    builtins.filter (path: !(builtins.elem path excluded)) declared
  );
  defaults = builtins.sort builtins.lessThan (
    lib.unique (lib.concatMap expand (workspace.default-members or [ ]))
  );
  names = map (
    path:
    let
      package = (builtins.fromTOML (builtins.readFile (root + "/${path}/Cargo.toml"))).package or { };
    in
    package.name or (fail "member has no package name: ${path}")
  ) members;
  packageManifest = path: builtins.fromTOML (builtins.readFile (root + "/${path}/Cargo.toml"));
  dependencyTables =
    value:
    lib.concatMap (key: builtins.attrValues (value.${key} or { })) [
      "dependencies"
      "dev-dependencies"
      "build-dependencies"
    ];
  validatePackage =
    path:
    let
      value = packageManifest path;
      dependencies =
        dependencyTables value
        ++ lib.concatMap dependencyTables (builtins.attrValues (value.target or { }));
      validDependency =
        dependency:
        if !builtins.isAttrs dependency then
          true
        else
          let
            inherited = dependency.workspace or false;
            # Inherited entries are checked separately, relative to the root.
            target = toString (root + "/${path}/${dependency.path or ""}");
          in
          if inherited || !(dependency ? path) then
            true
          else if builtins.any (member: target == toString (root + "/${member}")) members then
            true
          else
            fail "unsupported non-member local path dependency in ${path}: ${dependency.path}";
      parts = lib.splitString "/" path;
      ancestors = builtins.genList (index: lib.concatStringsSep "/" (lib.take (index + 1) parts)) (
        builtins.length parts - 1
      );
      nested = builtins.any (
        parent:
        builtins.pathExists (root + "/${parent}/Cargo.toml")
        && (builtins.fromTOML (builtins.readFile (root + "/${parent}/Cargo.toml"))) ? workspace
      ) ancestors;
    in
    if value ? workspace || nested then
      fail "unsupported nested workspace at ${path}"
    else if (value.package or { }) ? workspace then
      fail "unsupported package.workspace at ${path}"
    else
      builtins.all validDependency dependencies;
  validWorkspaceDependency =
    dependency:
    if !builtins.isAttrs dependency || !(dependency ? path) then
      true
    else if
      builtins.any (
        member: toString (root + "/${dependency.path}") == toString (root + "/${member}")
      ) members
    then
      true
    else
      fail "unsupported non-member local path dependency in workspace.dependencies: ${dependency.path}";
  valid =
    if manifest ? package then
      fail "root packages are not supported"
    else if builtins.length names != builtins.length (lib.unique names) then
      fail "duplicate package names in members: ${
        builtins.toJSON (
          map (path: {
            inherit path;
            name = (packageManifest path).package.name;
          }) members
        )
      }"
    else if !(builtins.all (path: builtins.elem path members) defaults) then
      fail "default-members includes a non-member or excluded package"
    else
      builtins.all validatePackage members
      && builtins.all validWorkspaceDependency (builtins.attrValues (workspace.dependencies or { }));
in
assert valid;
{
  inherit manifest members;
  normalize =
    included:
    manifest
    // {
      workspace =
        workspace
        // {
          members = builtins.filter (path: builtins.elem path included) members;
        }
        // lib.optionalAttrs (workspace ? default-members) {
          default-members = builtins.filter (path: builtins.elem path included) defaults;
        }
        // lib.optionalAttrs (workspace ? exclude) {
          exclude = builtins.filter (path: builtins.elem path included) excluded;
        };
    };
  requireWhole =
    included:
    if builtins.all (path: builtins.elem path included) members then
      true
    else
      fail "wholeWorkspace source omits declared members: ${
        builtins.toJSON (builtins.filter (path: !(builtins.elem path included)) members)
      }";
}
