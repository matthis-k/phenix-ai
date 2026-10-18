# Native plugin source preparation

The one supported runtime composition format is **phenix.composition.v3**.
Phenix's kernel accepts no legacy v2 compositions. Source resolution and
compilation are owned by the product host, before kernel admission. The
kernel continues to load verified native package directories via its existing
platform ABI and does not depend on Cargo, Git, Python, Nix or C++.

## Declare sources

An existing full composition document (entry, slots, grants, authority,
definitions, artifacts) can include a `plugin_sources` array:

```json
{
  "format": "phenix.composition.v3",
  "artifacts": ["/nix/store/...-existing-plugin"],
  "plugin_sources": [
    {"source": "./plugins/custom.so"},
    {"source": "./plugins/rust-crate", "builder": "rust"},
    {"source": "git+https://example.org/custom.git",
     "revision": "0123456789012345678901234567890123456789",
     "builder": "rust", "package": "custom_plugin"},
    {"source": "./plugins/nix-crate", "builder": "rust",
     "executor": "nix", "installable": ".#plugin"}
  ]
}
```

This abbreviated example omits standard required composition fields. Binary
paths may reference the Nix store, local filesystem or resolved downloaded
library. A prebuilt verified native package directory may be referenced
directly without re-packaging. Rust is the only built-in source builder;
additional strategies such as C++ are registered via the trusted top-level
`builders` argv map, using `{source}` and `{output}` placeholders. There
is no shell interpolation.

The Basic host invokes `phenix-plugin-sources` automatically when
`plugin_sources` or `builders` is present. The tool outputs a composition
of the *same v3 format* with source declarations replaced by verified native
package paths. Original configuration is never modified. Launchers can use
`PHENIX_PLUGIN_PREPARER` to provide a pinned tool path; otherwise it is
looked up on PATH. Manual preparation is also supported:

```sh
nix run .#phenix-plugin-sources -- config.json prepared.json
```

## Build and security properties

- Rust compilation is driven by Cargo metadata and emits exactly one
  `cdylib`. Cargo uses `--locked` and the selected package/features.
- Git sources require a pinned 40-character commit; HTTPS binaries require
  SHA-256. Remote source files are never trusted configuration.
- `executor = "nix"` builds an explicit Nix installable; the existing
  distributed Nix scheduler, remote builders and binary caches own reuse.
  Build and retrieval do not alter native plugin loading.
- New binaries are snapshotted under a content-digest package directory,
  described using the existing `phenix-describe` executable and validated
  again by the kernel. Optional `--store nix` copies packages to Nix store.
- Local mutable files are trusted deployment inputs, not an isolation
  boundary against a malicious process running with the same user rights.
- The current native artifact validator supports Linux x86-64 ELF `.so`
  only. Recognizing `.dll` or `.dylib` as a source extension is not a
  claim of Windows or macOS native loading support.

No new plugin ABI, legacy composition translation, duplicate source registry,
or language-specific kernel plugin loading is introduced.
