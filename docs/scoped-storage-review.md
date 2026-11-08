# Scoped Storage Review

## Scope And Baseline

Comparison baseline: the exact fetched HEAD `eeb39b64` (locally resolved as
`eeb39b64b`, `docs(storage): document durable SQLite schema validation and migration boundary`).
The earlier `b53f5be1` design foundation is not this review's baseline.
Current means the working-tree implementation, not a claimed release or green CI.
Sources inspected: storage core and scoped module, storage-plugin implementation
and README, scoped kernel example and integration tests, and revision, reserved-SQL
and SQLite-preflight regression tests. This documentation update changes no code.

## Public API Delta

These are source-checked public Rust call examples, with application-owned codecs,
values and enclosing error propagation omitted where indicated. They are not
claims that Markdown snippets were compiled. Both versions retain `Stored` with
`type Format: Codec<Value = Self> + Default` and low-level unversioned `Session::get`.

### Resource Declarations

Before, at `eeb39b64`, plugins supplied a concrete identity and only a schema:

```rust
use phenix_storage::{Codec, scoped::{ResourceId, ScopeId, StorageClass}};
use phenix_storage_plugin::{ResourceOffer, ResourceRequirements};

let resource = ResourceId::new(
    ScopeId::Project("project-1".into()), StorageClass::Config,
    "phenix.agent/settings",
)?;
registrar.add(ResourceOffer::new("settings", resource, codec.schema()))?;
let requirements = ResourceRequirements::collect(plugins.snapshots())?;
// requirements.iter(): (&ResourceId, &Schema)
```

After, scope resolution uses host identity and operations stay per requester:

```rust
use phenix_storage::{Codec, Operation, scoped::{Identity, ScopeKind, StorageClass}};
use phenix_storage_plugin::{ResourceOffer, ResourceRequirements};

registrar.add(ResourceOffer::kv_schema(
    "settings", ScopeKind::Project, StorageClass::Config,
    "phenix.agent/settings", codec.schema(), [Operation::Read],
))?;
let identity = Identity::user("alice")?.project("project-1")?;
let requirements = ResourceRequirements::collect(plugins.snapshots(), &identity)?;
// requirements.iter(): (&ResourceId, &ResourceKind)
// requirements.requests(): per-plugin ResourceRequest data, not approvals
```

`ResourceOffer::new` now takes `(id, ResourceRequest)`, not `(id, ResourceId, Schema)`.
`kv::<T>` is the `Stored` convenience; `sql` declares a mount without KV metadata.
The complete current host approval/prepare/activate example is in
[Scoped Storage](scoped-storage.md#declare-prepare-activate). Baseline had no
`StoragePolicy`, `PreparedStorage` or `ActivatedStorage`; hosts manually mounted,
registered, bound and issued sessions through APIs that remain available.

### Scoped Reads And Writes

Before, with a provisioned `Storage` and `AgentConfig: Stored`:

```rust
let kv = storage.project()?.config::<AgentConfig>("phenix.agent/settings")?;
let current: Option<AgentConfig> = kv.get("default")?;
kv.put("default", &replacement)?; // unconditional; no public CAS API
```

After:

```rust
use phenix_storage::{Revision, Revisioned};

let kv = storage.project()?.config::<AgentConfig>("phenix.agent/settings")?;
let current: Option<Revisioned<AgentConfig>> = kv.get("default")?;
let expected: Option<&Revision> = current.as_ref().map(|v| &v.revision);
let next: Revision = kv.compare_exchange("default", expected, &replacement)?;
// current.as_ref().map(|v| &v.value) accesses the decoded value.
// None means insert only if absent, not unconditional overwrite.
```

`put` remains unconditional. Read authority does not imply write authority; CAS
requires `Write`. Scoped read planning now requires revision support even for
read-only requests. Tokens are opaque and key-local, not integers, grants or
snapshot IDs. SQLite clock persistence and immediate transactions prevent normal
delete/recreate ABA within a backend/mount; old-file restore and absence are explicitly
outside a historical-token guarantee.

### SQL Binding

Before, even a SQL view used a registered dataset address:

```rust
catalog.bind(resource, address.clone())?;
// registry.register(&address, application_schema)? was required.
let storage = catalog.freeze().access(session, identity);
storage.project()?.sql(StorageClass::Data, "phenix.code/index")?
    .transaction(|sql| sql.execute("CREATE TABLE symbols(name TEXT)", &[]))?;
```

After, native SQL has an explicit whole-mount target and subscription:

```rust
use phenix_storage::scoped::BindingTarget;

catalog.bind_target(resource, BindingTarget::SqlMount("project".into()))?;
// No synthetic dataset registration. Session needs mount-scoped SQL authority.
let storage = catalog.freeze().access(session, identity);
let sql = storage.project()?.sql(StorageClass::Data, "phenix.code/index")?;
let mut changes = sql.changed(16)?; // independent mount Subscribe grant required
sql.transaction(|tx| tx.execute("CREATE TABLE symbols(name TEXT)", &[]))?;
let notices = changes.drain()?;
```

`Session::transaction(address, callback)` remains; `transaction_mount(name, callback)`
and `capabilities_mount(name, operation)` remove the fake-dataset requirement.
Sharing SQL across resources or plugins requires host opt-in. SQL authority remains
mount-wide for application tables, while reserved names, schema SQL-text access and
all callback `ALTER TABLE` are denied at every SQL authority level.

## Architecture

Each row records ownership, invariant, reuse/rationale, phase and replacement
boundary. Keep declaration, planning, authority and execution separate; no new
broker, scheduler, universal serializer or backend-neutral SQL language is needed.

| Type | Owner And Invariant | Reuse And Rationale | Phase And Replacement Boundary |
| --- | --- | --- | --- |
| `ResourceId` | Host-issued scope/class/name; independent of type, path and engine | Validated strings and ordered value keys give stable sharing identity | Resolution/lookup; internals replaceable, identity semantics must remain stable |
| `ResourceSpec` | Plugin declaration with unresolved `ScopeKind` and KV schema or SQL kind | Reuses `Identity::resolve` instead of plugins choosing host IDs | Collection; value description, no provisioning authority |
| `ResourceOffer` | Plugin contribution; request is never approval | Existing `Registrar`/`Contribution` and `ResourceRequest`, including operations/CAS | Discovery; bridge can be bypassed by manual host provisioning |
| `ResourceRequirements` | Host-owned copied compatible declarations and separate requester operations | `BTreeMap` deterministic merge; no requester code lease | Pure collection; replaceable input aggregation, not a policy engine |
| `ResourceCatalog` | Trusted host builds unique logical targets; dataset aliases forbidden | Logical/physical maps and existing `Address`; explicit SQL-alias exception | Binding validation; manual alternative to planned orchestration |
| `PreparedStorage` | Host-approved immutable plan; no operational sessions | Existing registry actions, provider handles and policy maps; semantic `StoragePlan` equivalent | Prepare/inspection then consuming activate; no redundant public `StoragePlan` wrapper |
| `Bindings` | Immutable shared lookup; conveys no authority | `Arc<BTreeMap<...>>` freezes catalog without another runtime | Scoped resolution; representation replaceable independently of grants |
| `Storage` | Plugin-specific identity, bindings and restricted session | Thin composition with `View`, `Kv` and `SqlResource`; no hidden cache | Author-facing access; optional facade over low-level API |
| `Session` | Host issues immutable grants; authorizes before lookup | Existing registry and per-mount locking | Execution; internals replaceable, no automatic revocation or scope fallback |
| `BackendOffer` | Provider factory; exact revision leased, not permission | Existing plugin contribution handles; features are declared contracts | Planning/opening; host can install a backend directly |
| `Backend` | Trusted host/provider extension; durable schema admission is provider responsibility | Object-safe typed-KV/SQL facets and observation metadata | Mount/register; external implementations supported, not sandboxed |
| `TypedKv` | Provider byte access; opt-in revision methods must be truly atomic and non-reusing | Existing get/put/delete plus unsupported-by-default CAS methods | Data access; no get/put CAS emulation; behavior cannot be proven by probing |
| `Stored` | Application's codec association, not universal serialization | Existing `Codec` associated value and `Default` format | Typed binding; optional convenience, independent codecs may interoperate |
| `Revisioned<T>` | Caller-owned decoded value and opaque revision together | Generic pair avoids a separate revision lookup/race | Read/CAS handoff; no snapshot or authority semantics |
| `XdgPaths` | Host path policy; absolute bases and stable identity placement | XDG environment/defaults, explicit runtime validation; bridge adds private provisioning | Placement; manual policy remains available with host-owned filesystem security |

`Revision` is the backend extension token (`from_bytes`/`as_bytes`), not an
application sequence. `BackendFeatures` is a planning declaration, distinct from
authorized runtime `Capabilities`. `SqlBackend` explicitly lends rusqlite and is
SQLite-specific. No PostgreSQL neutrality is claimed.

## Review Passes

These are the actual implementation review/fix passes recorded for this change,
not three new test executions performed by this documentation task. Named tests
below exist in the inspected tree; existence is separate from final run status.

### Pass 1: Authority And Leases

Rejected XDG auto-authority: `StoragePolicy::xdg` now selects placement with empty
allowlists. `prepare` requires both resource-level and per-plugin host approvals;
requests are never sufficient. Removed redundant requester leases after copying
declarations; only selected provider code needs lifetime pinning.

Evidence in `phenix-storage-plugin/src/lib.rs`:
`xdg_and_requests_are_placement_only_both_host_allowlists_are_required`,
`copied_request_declarations_do_not_pin_requester_code`, and
`prepared_and_active_backends_pin_exact_provider_revision`.

### Pass 2: SQL And ABA

Fixed persisted-trigger bypass of adapter authority: internal SQLite KV,
registration and migration deny derived SQL execution through `adapter_authorizer`.
Restored the external backend's no-authorizer connection contract on every SQL
exit, including unwind and failed transaction boundaries. Missing/regressed clocks
in versioned files now fail closed rather than reconstructing a clock from live
rows and risking reuse after deletion.

Evidence in `phenix-storage/tests/reserved_sql.rs`:
`persisted_trigger_cannot_gain_adapter_authority_during_kv_writes`,
`external_backend_recovers_host_authority_after_every_sql_exit`, and
`metadata_and_reserved_reads_are_denied_through_nested_and_temp_triggers`.
Evidence in `tests/revisions.rs`:
`missing_or_regressed_clock_fails_closed_without_reusing_tokens` and
`migration_blocks_derived_sql_and_rolls_back_all_schema_changes`.

### Pass 3: Independent Review

Independent review found and fixed private XDG storage/permissions across all
classes, not just runtime; revision-capability planning for reads without explicit
CAS requests; missing scoped SQL `changed` subscription; and persisted-schema
preflight before activation. The example now uses fixed explicit host approvals
and one PluginSet with OnceLock injection into the same deferred contributions,
not a second load/reload that could change declarations after approval.

Evidence in `phenix-storage-plugin/src/lib.rs`:
`xdg_creates_private_subtree_database_and_sqlite_journal`,
`xdg_rejects_unsafe_existing_components_and_sidecars_without_repairing`,
`xdg_revalidates_non_runtime_paths_and_rejects_out_of_subtree_overrides`,
`read_only_kv_requests_require_revision_support_in_prepare_and_activate`, and
`prepare_checks_persisted_schemas_per_mount_after_authorization`.
Additional evidence: scoped core test
`scoped_sql_changes_require_mount_subscription_and_report_mutations`, SQLite
preflight test `registration_rechecks_schema_after_successful_preflight`, and kernel
test `fixed_host_policy_rejects_extra_requests_before_storage_injection`.

## Final Scorecard

This is a code-and-regression-evidence assessment, not a numeric security rating or
a final CI verdict. Paths under tests below are relative to the named package.

| Area | Assessment | Evidence |
| --- | --- | --- |
| Authority | Explicit/default-deny implemented | `PreparedStorage::prepare`; plugin tests `prepare_rejects_permissions_kinds_and_capabilities_without_opening`, XDG allowlist test above |
| Sharing/isolation | Dataset aliases denied; SQL mount sharing explicitly approved | Plugin test `sql_is_mount_only_and_sharing_requires_explicit_policy`; scoped tests `two_plugins_share_one_resource_but_not_permissions`, `same_type_and_name_are_distinct_across_scopes` |
| CAS | Built-in atomic revision support implemented | Storage `tests/revisions.rs`: `revisions_conflicts_aba_and_notices`, `concurrent_cas_has_one_winner_and_one_notice`, `sqlite_migration_persistence_and_cross_connection_cas` |
| SQL boundary | Reserved namespace, schema SQL text and ALTER denial implemented | `reserved_sql`, `adapter_authorizer`; `tests/reserved_sql.rs::reserved_objects_are_protected_at_every_sql_authority_level` and pass-2 tests |
| XDG privacy | Unix private subtree and database/sidecar checks implemented | `private_xdg_file`, `XdgPaths::validate_runtime`; pass-3 permission tests and `xdg_runtime_is_revalidated_before_activation_side_effects` |
| Persistent schema | Advisory read-only built-in preflight plus activation recheck | Storage `tests/sqlite_preflight.rs::persisted_schema_checks_all_fields_without_mutating_database`, `metadata_view_is_rejected_without_derived_execution`; plugin per-mount preflight test |
| Provider lifetime | Exact provider revision pinned; requester leases removed | `LeasedBackend` field order; pass-1 lease tests |
| Failure lifecycle | No partial view publication; no filesystem rollback promised | Plugin `failed_activation_drops_earlier_backends_and_can_retry`; `PreparedStorage::activate` private registry |
| Graph integration | Same snapshots, independent types/codecs, restricted injected views | Kernel `tests/scoped_storage.rs::independent_plugins_drive_real_storage_waves_and_enforce_grants`, `restart_preserves_data_and_revisions_while_project_switch_isolates_both_resources`, `public_entry_point_runs` |
| Final checks | Local Cargo and Nix checks passed; hosted CI not yet rerun | Verification record below |

## Limits And Verification

Provider factories cannot validate persisted schemas before opening through this
interface. Planning trusts declared features; activation probes interfaces/CAS
support, not correctness, durability or honest invalidation. Durable providers
must enforce compatibility in `register_dataset`. Built-in schema preflight does
not validate payloads or revision-clock health and cannot eliminate planning races.
Activation rechecks but is not a multi-database/filesystem transaction. Private
XDG checks require host-protected bases; manual paths and provider-internal paths
have their own host/provider security policy. Observation stays bounded, lossy,
non-durable and adapter-local. No revocation or snapshot/subscribe handoff exists.

Verification record:

- Baseline was the exact fetched HEAD `eeb39b64`; cargo tests passed in the earlier
  verification work. This is historical evidence, not a rerun of the final tree.
- System `cargo fmt`/`cargo clippy` were unavailable then. Subsequent
  `nix develop` cargo fmt/check/clippy checks passed at that stage.
- CI run [42450640340](https://github.com/matthis-k/phenix-ai/actions/runs/42450640340)
  failed on rustfmt formatting. That failure is not superseded by a green result here.
- The first local Nix formatting check also found `flake.nix` formatting; both
  Rust and Nix formatting are now corrected.
- Final local commands passed through the repository's Nix development shell:
  `cargo fmt --all --check`, `cargo test --workspace --locked`,
  `cargo test --workspace --no-default-features --locked`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`, and
  `cargo run --locked -p phenix-kernel --example storage`. The storage example
  also executes the three-plugin scoped example.
- Each requested direct Nix build passed: `format`, `clippy`, `storage`,
  `kernel`, and `integration`. `nix flake check --no-update-lock-file
  --print-build-logs` passed, including workflow synchronization. An initial
  120-second attempt timed out; only the successful longer retry counts as passed.
- The Nix kernel inventory now explicitly includes `--test scoped_storage`;
  the existing integration inventory already includes the storage-plugin and
  storage-graph crates and all kernel integration tests. No CI backend or generated
  workflow change was necessary.

The failure-lifecycle tests also prove old bindings remain usable after candidate
activation failure, and an extracted scoped KV keeps the provider alive past
activation teardown and logical plugin unload. See
`failed_candidate_activation_preserves_old_bindings_and_data` and
`extracted_scoped_kv_outlives_activation_and_unload_with_provider_alive_at_backend_drop`.
