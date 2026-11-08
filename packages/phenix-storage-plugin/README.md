# Scoped Storage Integration

This crate bridges the existing plugin contribution snapshots to host-owned
storage. It does not load plugins or activate graphs.

## Declarations

Plugins contribute `ResourceOffer::kv::<T>(id, scope, class, name, operations)`
when `T: Stored`, or `ResourceOffer::kv_schema(...)` with a canonical `Schema`.
The latter requires no shared Rust value type. `ResourceOffer::sql(...)` declares
native SQL, without inventing a dataset or KV schema.

`ScopeKind::{User, Project, Session}` describes the requested owner. The host's
explicit `Identity` resolves it to a stable `ScopeId`. Missing projects/sessions
are errors, never fallbacks to user storage. Resource names use a namespace and
path, for example `phenix.agent/settings`.

`ResourceOffer::new(id, ResourceRequest::new(spec, operations).with_atomic_cas())`
can explicitly require atomic compare-and-exchange. SQL requests accept SQL
operations and subscription; KV requests accept read, write, delete and
subscription. CAS requires write authority. Since scoped `Kv::get` always returns
a revision, **every KV `Read` request implicitly requires the atomic-CAS/revision
capability**, even without `with_atomic_cas()`. Both declared capabilities during
prepare and actual backend capabilities during activation must satisfy this.
`ResourceRequest::requires_atomic_cas()` exposes that effective requirement.
Legacy unversioned-only backends remain usable through low-level `Session::get`,
not through scoped reads.

## Host Flow

```rust,no_run
use phenix_plugin::PluginSnapshot;
use phenix_storage::Operation;
use phenix_storage::scoped::{Identity, ResourceId, ScopeId, StorageClass, XdgPaths};
use phenix_storage_plugin::{ActivatedStorage, Error, PreparedStorage,
    ResourceRequirements, StoragePolicy};

fn activate(snapshots: Vec<PluginSnapshot>) -> Result<ActivatedStorage, Error> {
    let identity = Identity::user("alice")?.project("stable-project-id")?;
    let requirements = ResourceRequirements::collect(snapshots, &identity)?;
    let paths = XdgPaths::from_env()?;
    let mut policy = StoragePolicy::xdg(&requirements, &paths)?;
    // XDG selects placement only. These are trusted host decisions, not copies
    // of plugin requests. Every other request remains denied.
    let settings = ResourceId::new(
        ScopeId::Project("stable-project-id".into()),
        StorageClass::Config,
        "phenix.agent/settings",
    )?;
    policy.resources.get_mut(&settings).ok_or(Error::MissingPlacement)?
        .allowed.insert(Operation::Read);
    policy.plugin_access.insert("settings-reader".into(),
        [(settings, [Operation::Read].into())].into());
    // SQL shared by multiple resources OR plugins additionally requires an
    // explicit entry in policy.shared_sql_mounts.
    PreparedStorage::prepare(requirements, identity, policy)?.activate()
}
```

`ResourceRequirements::iter()` is the deterministic compatible merge of logical
resources. `requests()` preserves each plugin's requests, without merging its
authority with another plugin's. Schema and resource-kind conflicts fail collection.
Collection copies only owned declaration data; it does not retain requester code
or instance leases. A declaration, plugin name, or XDG placement is never approval.

For manual placement, start with `StoragePolicy::default()`, fill `mounts` with
`BackendPlacement::{Memory, Sqlite, Offer}`, and fill `resources` with
`ResourceBinding { target, allowed }`. `BindingTarget` is either `Dataset(Address)`
or `SqlMount(String)`. Both `ResourceBinding::allowed` and
`plugin_access[plugin][resource]` must explicitly approve every requested operation.
Missing plugins, missing resources in a plugin allowlist, empty sets and insufficient
operation sets deny access. `StoragePolicy::xdg` leaves both sets empty: it selects
placement and grants nothing. Hosts must fill both from trusted configuration or
an explicit user decision, not by copying declared requests. Successful activation
issues only requested operations, never the full host allowance. Never give plugins
the registry or the host policy.

`prepare` performs declaration, placement, access and capability checks without
opening provider backends, creating directories, registering datasets or issuing
sessions. After authorization, it calls `Registry::validate_sqlite_datasets` once
per existing SQLite mount with that mount's planned dataset schemas. This opens
existing SQLite databases read-only, without creation or migration, and rejects
persisted descriptor conflicts. Missing databases remain uncreated. Registration
checks compatibility again during activation, including changes after prepare.
Backend offers used here must declare `with_features(BackendFeatures { ... })`.
The exact offer revision is leased during prepare and retained by opened backends.
The low-level `mount_offer` remains available without this orchestration.
Provider factories cannot prove persisted compatibility before opening through
this contract: their prepare-time capabilities are declarations, not evidence of
on-disk schema compatibility. Providers must truthfully declare capabilities and
enforce persisted schema compatibility in `Backend::register_dataset`; failures
may therefore still occur during opening/registration at activation.

Prepared plans expose read-only views over their existing data: `requirements()`
provides canonical IDs, kinds and approved requests; `bindings()` provides targets
and resource allowances; `plugin_access()` and `shared_sql_mounts()` expose the
host decisions. `mounts()` exposes planned `PreparedBackend` open actions, SQLite
paths and provider handles; each backend's `capabilities()` reports its declared
features (`Some` for every successful plan). `registrations()` derives the planned
KV schema registrations; SQL has none. `xdg_paths()` exposes retained placement
provenance and private-path/runtime validation inputs for all storage classes.
The approved requests describe subsequent per-plugin grant/view
issuance. Inspection itself neither executes these actions nor grants authority.

`activate` constructs a private registry, opens mounts, checks provider capabilities
before issuing sessions, registers KV schemas, then returns `ActivatedStorage.views`, keyed by plugin
name. Each view has only that plugin's bindings and grants. SQL is always
mount-wide: explicit sharing exposes the mount's application SQL tables across
those logical resources. Reserved typed-KV, schema and revision tables remain
protected by the SQL authorizer, including on mounts colocating KV resources.
Provider replacement/unload does not replace existing backend instances.

Activation failure publishes no views and releases opened backends and leases.
It is not a multi-database/filesystem transaction: directories, files and durable
registration metadata created during activation are not deleted on failure.
Existing data is never deleted as cleanup. Hosts publish a successful replacement
activation themselves; old views remain usable until dropped.

## Scoped Access

Use `view.user().config::<T>(name)`, or `project()?` / `session()?` followed by
`config`, `data`, `state`, `cache` or `runtime`. `Kv::get` returns
`Result<Option<Revisioned<T>>>`; read `.value` and retain `.revision` for
`compare_exchange(key, Some(&revision), &value)`. Use `None` to insert only when
absent. CAS delegates to the session/backend and is never emulated with get/put.
`put`, `delete` and `changed` retain their explicit operation requirements.

`view.user().sql(class, name)?.transaction(...)` uses the real SQL mount without
registering a placeholder dataset. Resource identity does not sandbox SQL tables.
`SqlResource::changed(capacity)` subscribes to mount-wide invalidation through the
session and requires an explicit `Subscribe` grant, independently of SQL authority.

## Placement

XDG placement uses stable scope IDs, storage class and an injective ASCII resource
name encoding, not workspace paths, remote URLs or unstable Rust hashes. Each
logical resource initially gets an isolated SQLite file/mount. Canonical path and
existing Unix inode aliases between separately named mounts are rejected.

On Unix, XDG activation creates each app-owned directory below and including
`phenix` with mode `0700`, and precreates each database with mode `0600`, independent
of a permissive ambient umask. Existing app-owned directories/files must already
have these exact modes and belong to the effective OS uid. Symlinks, non-regular
database/sidecar files and hard-linked database/sidecar files are rejected, not
silently repaired. Private-path file checks and database precreation use
`O_NOFOLLOW`; SQLite subsequently opens paths under the validated private
directories. Newly created directories use explicit modes from their first creation. Existing XDG
base directories may be permissive, for example `0755`; their mode is not changed.
Existing journal/WAL/SHM files are checked as `0600` regular, owned, unaliased files.
New SQLite sidecars inherit the private database mode (the rollback journal mode
is exercised by tests). A restrictive umask that prevents the required modes may
cause activation to fail closed; the process-wide umask is never changed.

All XDG SQLite paths are checked during prepare and again before activation opens
any backend. App-private components are rechecked while provisioning and after
opening SQLite. XDG provenance is retained for every storage class, not just runtime.
SQLite path overrides must stay inside the appropriate class's `phenix` subtree;
cross-class sharing must use the same private root or is rejected. For arbitrary
manual paths, construct `StoragePolicy::default()` instead: directory/file modes,
symlink policy and filesystem security then remain explicitly the host's
responsibility. Replacing an XDG mount with memory does not provision filesystem
storage; replacing it with a provider uses that provider's own opening contract,
not a claim that XDG can control the provider's internal paths.

Runtime placement requires an existing non-symlink runtime directory owned by the
effective OS uid with mode `0700`. Unix uid lookup uses `libc::geteuid`, never
environment variables. Runtime checks run during policy construction and again
at activation. Non-Unix private XDG SQLite/runtime placement is rejected. The host must protect its
selected base directories against concurrent hostile path replacement; these
checks are not a filesystem sandbox. Manual low-level host placement remains
possible without using XDG policy.
