# Scoped Storage: identity, placement, backends and plugin extension

This design and additive implementation build on the clean foundation at
`b53f5be1`. The existing `Registry`, `Session`, SQL transactions and
storage-to-graph binding remain the low-level API and are not replaced.

## Resource identity

A `ResourceId` consists of:

- **ScopeId**: host-issued user, project or session identity.
- **StorageClass**: config, data, state, cache or runtime. This is a placement
  and retention classification, *not* a query engine capability.
- **Name**: a validated `namespace/resource` name, e.g.
  `phenix.agent/settings` or `phenix.code/graph`.

Backend type, file path, current working directory, Git remote, Rust type and
requesting plugin identity are **not** part of the resource identity.

Sharing is explicit: two plugins opening the same resource ID through the same
`Bindings` snapshot address the same physical dataset, subject to their
separate `Session` grants. A different scope or class is a different resource.
The trusted host cannot alias two logical resources to one physical dataset by
accident: `ResourceCatalog::bind` rejects conflicting targets.

`Identity::project` requires a **stable host-issued ProjectId**. Source paths,
Git remotes and worktrees are *discovery hints*, not stable identities. Two
clones can intentionally be different projects. An alias registry or project
identity service belongs in the trusted host or a policy plugin, not in
`phenix-storage`.

## Author-facing API (implemented)

These are the actual APIs of `phenix_storage::scoped`; the host must provision
mounts, datasets and resource bindings first. The example codec below is an
ordinary `Codec` implementation and its type opts into `Stored`.

```rust
use phenix_storage::scoped::{Identity, ResourceCatalog, ResourceId,
    ScopeId, StorageClass, Stored};
use phenix_storage::{Address, Grant, Operation, Registry, Scope};

// Host provisioning:
let registry = Registry::new();
registry.mount_sqlite("workspace", path)?;
let address = Address::parse("store://workspace/settings")?;
registry.register(&address, AgentConfigCodec::default().schema())?;

let resource = ResourceId::new(
    ScopeId::Project("project-uuid".into()),
    StorageClass::Config,
    "phenix.agent/settings",
)?;
let mut catalog = ResourceCatalog::new();
catalog.bind(resource, address.clone())?;
let bindings = catalog.freeze();

// Plugin A: write-capable view.
let writer = bindings.access(
    registry.session([Grant::new(
        Scope::Dataset(address.clone()), [Operation::Write]
    )]),
    Identity::user("local-user")?.project("project-uuid")?,
);
writer.project()?.config::<AgentConfig>("phenix.agent/settings")?
    .put("default", &config)?;

// Plugin B: independently restricted read-capable view.
let reader = bindings.access(
    registry.session([Grant::new(
        Scope::Dataset(address), [Operation::Read]
    )]),
    Identity::user("local-user")?.project("project-uuid")?,
);
let selected = reader.project()?.config::<AgentConfig>("phenix.agent/settings")?
    .get("default")?;
```

`Stored` has one associated type:
```rust
trait Stored: Sized {
    type Format: Codec<Value = Self> + Default;
}
```
It is deliberately not a new universal serialization scheme. Types still
carry an explicit codec, whose `Schema` is checked by the low-level session.
Independent plugins can implement compatible codecs without sharing Rust
`TypeId`, as long as both structure and wire format really agree.

`View::sql(class, name)` resolves a declared logical resource, but it **does
not** narrow native SQL authority to a dataset. SQL remains mount-scoped in
`Session::transaction`, as required by arbitrary SQL joins/DDL.

## Placement: XDG, not hard-coded paths

`XdgPaths::from_env()` uses absolute `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
`XDG_STATE_HOME`, `XDG_CACHE_HOME` overrides or the respective HOME defaults.
Invalid relative overrides are ignored. `XDG_RUNTIME_DIR` is optional.
`XdgPaths::directory(ResourceId)` provides the base directory for the host,
e.g. a project data resource resolves under
`$XDG_DATA_HOME/phenix/projects/<stable-id>`.

This is **host-only path policy**: it does not create directories, open a DB,
grant filesystem access, scan workspace roots or store secrets. The trusted host
must validate XDG runtime-directory ownership/permissions before using it for
sensitive IPC. Never derive physical paths directly from an untrusted resource
name. Directory creation, migrations, backup and encryption belong to
provisioning/backend policy.

Storage classes are independent of query capabilities. For example,
`Project/Config` may be backed by SQLite today and another typed-KV adapter
later without changing its resource identity. User/project/session scope
inheritance is *not automatic*; config fallback belongs in the config plugin.

## Persistent typed KV with SQLite (implemented)

`mount_sqlite` provisions an internal `__phenix_kv` table using native
SQLite `BLOB` values and a composite `(dataset,key)` primary key. A SQLite
mount now offers both typed KV and native SQL. Single-statement KV mutations
complete before the existing bounded invalidation is published.

The host must register datasets at startup, but SQLite also persists the
authoritative schema descriptors in `__phenix_schema`; incompatible restarts
are rejected before stored bytes can be decoded. Resource bindings remain host
configuration, not implicitly durable. SQLite records persist in the selected
database file; memory records do not. Explicit schema migration is still a
separate host operation.

The reserved table is part of the adapter's storage schema. A trusted SQL
administrator on the same mount can change it; this is one reason untrusted
plugins must not receive SQL administrator authority. Preserve explicit SQL
read/write/admin grants and validate migrations before deploying.

## Backend and plugin architecture

**Decision: Storage core is a foundation; backend implementations are plugins
or host-installed adapters.**

The existing in-process plugin registry already permits non-graph contribution
types and pins contributions through `PluginSnapshot` leases. Reuse that API.

Implemented optional bridge in `phenix-storage-plugin`:

```rust
// Provider plugin:
registrar.add(BackendOffer::new("memory", || {
    Ok(Box::new(MemoryBackend::default()))
}))?;

// Trusted host, before composing the graph:
mount_offer(&registry, &provider_snapshot, "memory", "workspace")?;
```

The offer is **not authority**. Only the trusted host may select backend
offers, supply paths or credentials, configure mounts, register resources, and
issue restricted sessions. A `LeasedBackend` pins the exact provider snapshot
until the mounted backend has been dropped; logical plugin unload does not
invalidate a live mount. Graph contributions capture restricted sessions. No
Storage -> Kernel or Graph -> Storage dependency is required.

The current `Backend::typed_kv` extension interface is object-safe, so an
external backend can implement typed KV without editing Storage. The current
`SqlBackend` returns a `rusqlite::Connection`, and is therefore genuinely
**SQLite-specific**, not an arbitrary PostgreSQL SQL interface. Do not disguise
it as backend-neutral SQL; add a provider-specific native capability or a
carefully scoped query abstraction once a second SQL backend is implemented.

A runtime `StorageBroker` with plugin declarations, binding selection and
resource provisioning is a *future host composition layer*, not a second
scheduler or database engine. Do not modify the kernel merely to house it.



### Resource declarations (implemented)

Two independently authored plugins can declare compatible resource requirements:

```rust
let resource = ResourceId::new(
    ScopeId::Project("project-uuid".into()),
    StorageClass::Config,
    "phenix.agent/settings",
)?;
registrar.add(ResourceOffer::new(
    "agent-settings",
    resource,
    AgentConfigCodec::default().schema(),
))?;
```

The trusted host calls `ResourceRequirements::collect(selected_snapshots)`
to obtain one canonical schema per logical resource. A conflict fails before
provisioning. Duplicate compatible declarations do not themselves share grants:
each plugin still receives an independently restricted `Session`.

This pure preflight does **not** choose a filesystem path, create a resource,
issue grants or retain plugin code after the resource declaration is copied.
Selected backend instances do retain a contribution lease for their lifetime.

## Domain workload coverage

| Domain | Resource class | Backing capability | Additional behavior owned by plugin |
| --- | --- | --- | --- |
| Agent config | user/project config | Typed KV | CAS, precedence, validation |
| Authorization policies | user/project data | SQL + scoped grants | Audit, revocation, secret references |
| Session/model events | session state/data | SQL + blobs | Event sequencing, lifecycle |
| Evidence-based code graph | project data/cache | SQL, indexes | Symbols, source revision, provenance |
| Referenced memory | user/project data | SQL, blobs, search | Source-to-derivation links and deletion |

Do not conflate the runtime **execution graph** with an evidence/code graph
stored as domain data. Storage does not understand either domain.

## Next passes and evidence gates

1. **CAS/revisions**: add an explicitly advertised atomic compare-exchange
   capability for KV (memory and SQLite). Do not emulate CAS using get+put.
2. **Hosted resource provisioning**: `BackendOffer` and `ResourceOffer`
   now reuse the existing registrar; `ResourceRequirements::collect` accepts
   repeated declarations only when their schemas match. Add trusted-host
   resource placement, grant preflight, transactional provisioning and rollback.
   Backend factory configuration remains host responsibility.
3. **Blob and index adapters**: provide optional capabilities, not a universal
   database API; document blob durability and index invalidation guarantees.
4. **Observation ordering**: preserve commit-aware invalidation on KV/SQL,
   add snapshot/subscribe handshake only where continuity can be proven.
5. **Secret handling**: route actual credentials through an OS secrets provider.
6. **Nix/CI**: keep the branch's independent flake checks and matrix inventory;
   measure cold/warm runtime and validate a persistent-SQLite example in the
   package smoke suite.

Regression requirements: two plugins sharing one logical resource with
different grants; same type/name under user vs project scope; no physical
aliasing; stable project-ID placement after path moves; SQLite KV persist/reopen;
denied writes; same-mount reentrancy; no phantom notice on failed writes;
correct external backend capability rejection.

The present additive slice implements resource identity, typed KV views, XDG
path policy, SQLite KV persistence, backend offers with pinned plugin leases,
and declarative resource-offer compatibility checks. It does **not** implement
CAS, memory derivation, automatic resource provisioning, blob storage or
XDG auto-provisioning.

References:
- https://specifications.freedesktop.org/basedir/0.8/
- https://docs.rs/rusqlite/latest/rusqlite/
- https://github.com/matthis-k/phenix-ai/blob/b53f5be1/packages/phenix-plugins/src/registry.rs
- https://github.com/matthis-k/phenix-ai/blob/b53f5be1/packages/phenix-storage/src/lib.rs
