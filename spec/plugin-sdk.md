# Plugin SDK contributions

status: implemented

## Purpose

Define two separate roles:

- `phenix-sdk` is the passive Rust authoring library. Importing it activates no runtime behavior.
- `phenix-plugin-api` is an optional runtime plugin. It provides convenience services and contributes the default `phenix` SDK namespace.

Core resolves SDK contribution metadata. Runtime behavior still uses ordinary typed component interfaces and the normal component graph.

## SDK library

`phenix-sdk` owns Rust authoring support:

- `PhenixSdk` and `phenix_context`;
- typed SDK clients and capability objects;
- plugin authoring helpers such as event/listener support;
- common Core and domain re-exports used by plugin authors;
- `phenix-sdk-macros` derives and attributes;
- provider authoring helpers where appropriate.

The crate has package role `passive-library`. It has no `PluginManifest`, component manifest, factory, activation hook, runtime plugin ID, or Nix `phenixPlugins` entry.

## API plugin

`phenix-plugin-api` is an ordinary runtime plugin with these canonical identities:

```text
package       phenix-plugin-api
plugin        phenix.api
component     phenix.api
sessions      phenix.api.sessions@1
tools         phenix.api.tools@1
skills        phenix.api.skills@1
config        phenix.api.config@1
SDK namespace phenix
```

The API plugin may be disabled or replaced. No compatibility package, plugin ID, component ID, service ID, or package-set alias exists for the former `phenix-plugin-sdk` or `phenix.sdk` identity.

The API plugin provides policy-bearing convenience operations. It does not own the underlying session, tool, skill, model, context, or option state.

Sessions resolve scoped options before calling the ordinary session contract. Tools map registration and invocation to execution callables. Skills map registration and lookup to context resources whose kind is `skill`. Models, context, and options use their normal typed interfaces.

## Contributions

An SDK contribution declares:

- the selected plugin that provides it;
- one stable SDK namespace;
- typed component interfaces exposed through that namespace;
- optional opaque SDK resource identifiers for client helpers or generated bindings.

`SdkNamespace` and `SdkResourceId` are parsed identifiers. Invalid identifiers do not enter resolved state. Resources are opaque to Core.

SDK contributions resolve against an already resolved generation. Resolution requires the provider plugin to be selected, every referenced interface to be available, and one provider to own each namespace.

Source order does not affect the result. SDK metadata grants no authority and does not alter provider selection. The resolved SDK set is derived from selected plugins, components, and contribution metadata rather than stored in a second runtime registry.

A testing plugin may independently contribute a `testing` namespace. Removing the API plugin removes only the `phenix` contribution and its convenience services.

## Configuration files

The API plugin exposes `phenix.api.config@1`. `Read` accepts a parsed relative path under `PHENIX_CONFIG_DIR`. It rejects absolute paths, `.` and `..` segments, and symlinks that resolve outside that directory.

The Nix wrapper owns `PHENIX_CONFIG_DIR`. It points at the selected user configuration directory or the packaged default directory. SDK code receives relative names such as `settings.json`; it does not discover host configuration directories.

## Invariants

- `phenix-sdk` is passive authoring code.
- `phenix-plugin-api` is the only default runtime owner of `phenix.api*` identities.
- SDK helpers invoke ordinary typed component interfaces.
- SDK contribution metadata carries no authority.
- First-party and third-party contributions use the same contract.
- Duplicate namespace ownership fails resolution.
- Unknown providers fail resolution.
- References to unavailable interfaces fail resolution.
- Resource-only plugins may provide client-only SDK helpers.
- Zero-plugin composition has no SDK namespaces.
- API convenience behavior reads userspace options instead of adding hidden Core policy.
- Convenience wrappers reuse product-owned state instead of storing duplicate state.

## Required regressions

- importing `phenix-sdk` does not register or activate a plugin;
- `phenix-sdk` is classified as `passive-library`;
- `phenix-plugin-api` is classified as `runtime-plugin`;
- the legacy `phenix-plugin-sdk` package and `phenix.sdk` runtime identity are absent;
- two selected plugins can contribute different namespaces;
- a resource-only plugin can contribute client helper resources;
- unavailable interfaces reject a contribution;
- duplicate namespace ownership fails;
- an unselected provider fails;
- empty composition resolves to no SDK namespaces;
- the API plugin advertises sessions, models, tools, skills, context, and options through the `phenix` namespace;
- the API session helper honors scoped option resolution;
- API tools register through execution callables;
- API skills register and list through context resources;
- config paths are parsed before dispatch;
- config reads cannot follow symlinks outside `PHENIX_CONFIG_DIR`.
