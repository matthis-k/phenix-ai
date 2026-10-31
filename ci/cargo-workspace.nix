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
  packages = builtins.listToAttrs (
    map (path: {
      name = (packageManifest path).package.name;
      value = path;
    }) members
  );
  dependencyEntries =
    value:
    if !builtins.isAttrs value then
      fail "unsupported dependency table shape"
    else if value ? dev_dependencies || value ? build_dependencies then
      fail "unsupported legacy dependency table (use dev-dependencies/build-dependencies)"
    else
      lib.concatMap
        (
          key:
          if !builtins.isAttrs (value.${key} or { }) then
            fail "unsupported dependency table shape: ${key}"
          else
            lib.mapAttrsToList (name: dependency: { inherit name dependency; }) (value.${key} or { })
        )
        [
          "dependencies"
          "dev-dependencies"
          "build-dependencies"
        ];
  validateDependency =
    dependency:
    if builtins.isString dependency then
      true
    else if !builtins.isAttrs dependency then
      fail "unsupported dependency metadata shape"
    else if dependency ? workspace && dependency.workspace != true then
      fail "unsupported dependency workspace override"
    else if
      dependency ? workspace
      && builtins.any (
        key:
        !(builtins.elem key [
          "workspace"
          "features"
          "optional"
          "default-features"
        ])
      ) (builtins.attrNames dependency)
    then
      fail "unsupported dependency workspace override"
    else if
      dependency ? path
      && (
        !builtins.isString dependency.path
        || dependency ? git
        || dependency ? registry
        || dependency ? registry-index
      )
    then
      fail "unsupported dependency path/source combination"
    else
      true;
  localDependencies =
    path:
    let
      value = packageManifest path;
      entries =
        dependencyEntries value
        ++ lib.concatMap dependencyEntries (builtins.attrValues (value.target or { }));
    in
    lib.unique (
      lib.concatMap (
        entry:
        let
          inherited =
            assert validateDependency entry.dependency;
            builtins.isAttrs entry.dependency && (entry.dependency.workspace or false);
          dependency =
            if inherited then
              workspace.dependencies.${entry.name}
                or (fail "missing inherited workspace dependency: ${entry.name}")
            else
              entry.dependency;
          base = if inherited then "" else "${path}/";
          target = toString (root + "/${base}${dependency.path}");
          matches = builtins.filter (member: toString (root + "/${member}") == target) members;
        in
        if !(validateDependency dependency) then
          [ ]
        else if !builtins.isAttrs dependency || !(dependency ? path) then
          [ ]
        else if matches == [ ] then
          fail "unsupported non-member local path dependency in ${path}: ${dependency.path}"
        else
          let
            member = builtins.head matches;
            expected = dependency.package or entry.name;
          in
          if (packageManifest member).package.name != expected then
            fail "local dependency package name mismatch in ${path}: ${entry.name}"
          else
            [ member ]
      ) entries
    );
  closure =
    seeds:
    let
      visit =
        seen: pending:
        if pending == [ ] then
          builtins.sort builtins.lessThan seen
        else
          let
            path = builtins.head pending;
            rest = builtins.tail pending;
          in
          if builtins.elem path seen then
            visit seen rest
          else
            visit (seen ++ [ path ]) (rest ++ localDependencies path);
    in
    visit [ ] (
      map (name: packages.${name} or (fail "unknown package: ${name}")) (
        if builtins.isList seeds then seeds else [ seeds ]
      )
    );
  validatePackage =
    path:
    let
      value = packageManifest path;
      localFile =
        file:
        if
          !builtins.isString file
          || file == ""
          || lib.hasPrefix "/" file
          || lib.hasInfix "\\" file
          || builtins.elem ".." (lib.splitString "/" file)
        then
          fail "unsupported explicit target/build path outside package: ${path}"
        else
          true;
      targetPaths =
        lib.optional (value ? lib && value.lib ? path) value.lib.path
        ++
          lib.concatMap
            (key: map (target: target.path) (builtins.filter (target: target ? path) (value.${key} or [ ])))
            [
              "bin"
              "test"
              "bench"
              "example"
            ];
      parts = lib.splitString "/" path;
      ancestors = builtins.genList (index: lib.concatStringsSep "/" (lib.take (index + 1) parts)) (
        builtins.length parts - 1
      );
      nested = builtins.any (
        parent:
        builtins.pathExists (root + "/${parent}/Cargo.toml")
        && (builtins.fromTOML (builtins.readFile (root + "/${parent}/Cargo.toml"))) ? workspace
      ) ancestors;
      regularTree =
        relative:
        let
          entries = builtins.readDir (root + "/${relative}");
        in
        builtins.all (
          name:
          if entries.${name} == "symlink" then
            fail "unsupported symlink in package: ${relative}/${name}"
          else if entries.${name} == "directory" then
            regularTree "${relative}/${name}"
          else
            true
        ) (builtins.attrNames entries);
    in
    if
      !(
        builtins.all localFile targetPaths
        && (if builtins.isBool (value.package.build or false) then true else localFile value.package.build)
      )
    then
      false
    else if value ? workspace || nested then
      fail "unsupported nested workspace at ${path}"
    else if (value.package or { }) ? workspace then
      fail "unsupported package.workspace at ${path}"
    else
      regularTree path && builtins.deepSeq (localDependencies path) true;
  validWorkspaceDependency =
    dependency:
    assert validateDependency dependency;
    if builtins.isAttrs dependency && dependency ? workspace then
      fail "unsupported workspace dependency inheritance"
    else if !builtins.isAttrs dependency || !(dependency ? path) then
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
    if manifest ? patch || manifest ? replace then
      fail "unsupported root patch/replace tables"
    else if manifest ? package then
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
  inherit
    manifest
    members
    packages
    closure
    localDependencies
    ;
  projectBin =
    name: bin:
    let
      path = packages.${name} or (fail "unknown package: ${name}");
      value = packageManifest path;
      entries = builtins.readDir (root + "/${path}");
      src = builtins.readDir (root + "/${path}/src");
      bins = value.bin or [ ];
      selected = builtins.filter (target: target.name == bin) bins;
      target =
        if selected != [ ] then
          (builtins.head selected).path or "src/bin/${bin}.rs"
        else
          "src/bin/${bin}.rs";
    in
    if
      value ? lib
      || value.package ? build
      || entries ? "build.rs"
      ||
        builtins.attrNames entries != [
          "Cargo.toml"
          "src"
        ]
      || builtins.attrNames src != [ "bin" ]
      || target != "src/bin/${bin}.rs"
      || !(builtins.pathExists (root + "/${path}/${target}"))
      ||
        builtins.match ".*(^|[^A-Za-z_0-9])(mod|include|include_str|include_bytes)([^A-Za-z_0-9]|$).*" (
          builtins.readFile (root + "/${path}/${target}")
        ) != null
      || builtins.any (file: !(lib.hasSuffix ".rs" file)) (
        builtins.attrNames (builtins.readDir (root + "/${path}/src/bin"))
      )
    then
      fail "unsupported bin projection layout: ${name}"
    else
      [
        "${path}/Cargo.toml"
        "${path}/${target}"
      ];
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
