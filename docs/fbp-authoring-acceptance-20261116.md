# FBP Authoring Acceptance

Baseline: `0c0bbb1a57c9f8c0a98de2a0b8a1a0a26d70ef6e`, reviewed on
2026-11-16. This is bounded evidence for the original A4 authoring/consolidation
finding, not a score update or a declaration that independent review and CI are
complete. No production graph API was changed or removed. The acceptance work
owns only the new graph integration test, this note, and commands in the existing
Nix `graph` and `no-default` checks. `fbp_port_keys.rs` needed no edits.

## Authority Inventory

The source audit found **no independently editable duplicate authority** in these
overlapping authoring surfaces. Similar names or repeated cached views alone do
not demonstrate redundant authoring models.

| Surface | Authority, derivation, and invalidation |
|---|---|
| `Interface` / `PortDef` (`src/interface.rs`) | Canonical named input/output sets and local `TypeId` or complete wire-schema identities. Construction validates names and duplicates; detached cloned port definitions cannot mutate an existing interface. |
| `ComponentDef::primitive` (`src/component.rs`) | Receives the canonical immutable interface and factory. It does not derive a second boundary from process code. Claims are separate, explicit semantic promises. |
| `ComponentDef::composite` | Gets its interface, claims, and origin from the frozen `NetworkDef`. There is no independently supplied composite boundary to drift from its exports. A new definition identity is not behavioral equivalence evidence. |
| `NetworkBuilder::node`, `module`, `add` (`src/network.rs`) | `node` and `module` construct a definition and delegate to `add`. They are convenient body-specific constructors, not separate registries or graph builders. Keeping them avoids an unsupported API break. |
| `NetworkBuilder` exports and `NetworkDef` | Public ports are derived from checked endpoint projections. Frozen definitions preserve symbolic incident edges, public projections, capacities, explicit starts, claims, and requirements. Compilation resolves these to physical processes without executing factories. |
| `InputKey`, `OutputKey`, `InputGroup` (`src/port_keys.rs`) | Typed names/modes and dotted namespaces. Key lookups delegate to existing local/wire component lookup. Keys carry neither builder scope nor instance revision and cannot authorize another owner or infer a lease. |
| `#[derive(Ports)]` (`port-args-derive/src/lib.rs`) | Generates those existing keys/groups. `interface()` and `InputRecord::input_ports()` use their definitions, with canonical `Interface` validation. Metadata is recomputed rather than maintained as a separately editable interface cache. Flattened groups are namespaces, not transport slots. |
| `InputRecord` / `ProcessContext::input_record` | Reconstruct reserved named leaves using scoped continuations. It does not select queue heads, schedule readiness, or transport a hidden record. Unique local leaves move; shared move-only leaves reject; encoded/explicit-Arc leaves retain envelopes. Failure may consume preceding leaves. |
| `ComponentRef`, typed refs, erased endpoints | Validated views of a declared interface, guarded by builder scope and revision. Successful replacement revokes old capabilities. Failed replacement leaves them valid. `from_def` creates a new scope, not authority transferable from the original builder. |
| Compiled contract bindings | Restricted validated boundary views tied to one compiled generation. A separately compiled identical definition does not accept the old bound input/output. Old compiled handles remain valid against their own immutable compiled network. |
| Contracts and frozen obligations (`src/contract.rs`, `src/network.rs`) | Exact boundary equality is not semantic membership. Replacement must preserve existing claims and private descendant obligations, even when a public shape and public claim match. |

Erased/dynamic APIs remain necessary. Runtime-selected declarations and distinct
Rust types with the same complete wire schema cannot in general be expressed as
one static `connect<T>` call. `connect_dynamic` still validates both endpoint
capabilities and canonical identities. Removing it, or forcing all authors through
a derive macro, would remove supported functionality rather than consolidate a
demonstrated duplicate authority.

## Execution Evidence

The new test is
[`fbp_authoring_equivalence.rs`](../packages/phenix-graphs/tests/fbp_authoring_equivalence.rs).
It deliberately retains a manual path with explicit `PortDef`s and literal leaf
lookups. Manual expected declarations are not obtained from a derived interface.
The key path uses existing typed keys, and the feature-gated derived path uses
generated keys/groups and actual `InputRecord` reconstruction. Both manual and
key executions remain active without default features; the whole file is not
derive-gated.

| Test | Acceptance boundary |
|---|---|
| `same_topology_authors_have_identical_canonical_compilation_and_fifo_execution` | Paired local and encoded pipelines, each with primitive and nested-composite target bodies. Compares canonical public interface, resolved input/output projections, root ingress capacity 3, exact physical incident edges and capacities 2/4 for the same topology. Child export capacity 1 does not override the root ingress. Executes FIFO values `[7, -2, 19]`; checks local payloads, independent literal canonical wire-byte vectors, output identity, capture name, output name, producer path, absent request/origin, consistent session/generation, and increasing activation metadata. Wrong local/wire admission rejects before the successful run. |
| `body_replacement_preserves_symbolic_incident_edges_and_public_execution_not_physical_identity` | Executes both primitive-to-composite and composite-to-primitive replacements, for all authors and both transport modes. Compares public interfaces, symbolic incident edges and export projections, ingress capacities, and observed captures. Rejects old endpoint/component capabilities. Does not equate the replaced body's physical path, index, or definition identity. The unchanged downstream observer provides the common capture metadata boundary. |
| `nested_renamed_flattened_records_execute_unique_move_only_local_and_wire_leaves` | Independent manual leaf list for generic/raw/renamed records nested through two flattened groups. Compares canonical and resolved public views/capacities, runs unique non-`Clone` local leaves plus encoded leaves and labels, and captures exactly `11:7:a`, `12:-2:b`, `13:19:c`. Uses explicit `Readiness::All`: reconstruction is not an implicit join policy. No synthetic group port exists. |
| `all_record_authors_report_later_leaf_decode_failure_without_publishing_a_capture` | Same canonical schema but malformed later-leaf bytes pass transport admission and fail application decoding for manual, key, and generated execution. Session failure reports `bad i64 length`; no summary capture is published. This is not a promise to roll back earlier consumption or process effects. |
| `empty_source_and_sink_execute_without_synthetic_unit_ports` | Real source publication and sink consumption, plus explicit empty-process execution. Expected value is 23, empty execution occurs once, input-bearing sink cannot be started as a source, and the public interface is genuinely empty. |
| `keys_do_not_grant_scope_revision_claim_or_compiled_generation_authority` | Matching shape does not infer a claim. Foreign typed/bound/erased endpoints reject. Failed shape or claim replacement preserves the old scope and usable endpoints. Successful replacement revokes typed, erased, and bound views. Missing names and wrong Rust/local-wire identities reject. Compiled bound input and output reject another compilation generation. |
| `equal_public_shape_and_claim_do_not_erase_private_obligations` | Public-shape/public-claim-compatible replacement cannot erase a required private child. Rejected replacement leaves the original module executable and its private implementation inspectable. |
| `generated_nested_later_leaf_error_and_unwind_drop_acquired_payload_before_lease` | Generated nested continuation acquires an earlier leased move-only local leaf before a later nested decoder returns an error or unwinds. The payload destructor observes its code lease alive via `Weak::upgrade`; event order is exactly payload then lease. Consumed earlier/later leaves remain consumed and an unrelated context leaf is untouched. This extends the existing direct single-leaf callback test rather than relabeling it as generated-record coverage. |

The existing 13 `fbp_port_keys` tests remain useful for codec framing, collisions,
missing leaves, shared move-only rejection, explicit Arc sharing, lease-preserving
successful reconstruction, independent leaf selection, and type/mode checks.
Some use `Noop` factories and prove only compilation. They were not sufficient on
their own to establish paired execution equivalence; the new suite fills that
specific acceptance gap without replacing their narrower checks.

## Check Wiring

Before editing, inspected the existing `flake.nix`, evaluated both original
derivations and their `buildPhase`, and inspected their narrowed source snapshots:
graph source `/nix/store/0sb892qnwhv69hr4zna3plxmwbjpa2bi-source` and workspace
no-default source `/nix/store/35xpky0i8qb20irgjx9km9l0nhqc712v-source`. Those source
snapshots were locally present; their evaluated check outputs and vendor output
were not present locally. This is not evidence of completed cached builds.

The existing `graph` check retains its graph/plugin all-target command and adds
only `cargo test --locked --offline -p phenix-graph --doc`. The existing
`no-default` check retains its workspace test command and then explicitly runs:

```sh
cargo test --locked --offline -p phenix-graph --no-default-features --lib --tests
cargo test --locked --offline -p phenix-graph --no-default-features --doc
```

The graph-only invocation is necessary, not a second equivalent workspace build:
workspace consumers can enable graph's `derive` default dependency feature even
when their own defaults are disabled. Selecting graph alone avoids that feature
unification. `cargo tree -p phenix-graph --no-default-features -e features` confirms
that this graph selection depends only on `phenix-schema`, not the derive crate.
There is no new check, workflow, standalone build command, external CI pin change,
or extra default graph unit/integration build.

## Verification

Executed with `--locked --offline` and package selection `-p phenix-graph`:

```sh
cargo test --locked --offline -p phenix-graph --test fbp_authoring_equivalence --test fbp_port_keys --test fbp_components --test fbp_contracts
cargo test --locked --offline -p phenix-graph --no-default-features --test fbp_authoring_equivalence --test fbp_port_keys --test fbp_components --test fbp_contracts
cargo test --locked --offline -p phenix-graph --doc
cargo test --locked --offline -p phenix-graph --no-default-features --doc
```

- Default focused tests: acceptance 8, port keys 13, components 21, contracts 13, all passing.
- No-default focused tests: acceptance 7, port keys 0 (existing file-wide derive gate), components 21, contracts 13, all passing. Seven real acceptance tests remain, including manual and key execution; the zero-test legacy target is not counted as coverage.
- Doctests: 9 runnable and 17 compile-fail cases pass in each configuration. Four derive-dependent compile-fail examples (`port_keys.rs` directions, constant collision, codec bound, and `InputRecord` sendability) can reject merely because `Ports` is unavailable without defaults. Their no-default results are **not** counted as evidence of the intended macro/type rejection. Derive-gated runnable example bodies likewise are not credited as no-default reconstruction coverage.
- Scoped rustfmt check and clippy with `-D warnings` pass for the acceptance/port-key/component/contract targets in both configurations. Rust formatting and clippy were obtained through `nix develop --offline`; host Cargo lacks those subcommands. Nix formatting passes for `flake.nix`.
- Offline Nix evaluation succeeds for the modified existing checks, including their commands and source closures. Direct import evaluation also includes untracked acceptance source, avoiding the Git-flake omission of new unstaged files. Full sandboxed Nix builds and remote CI were not run.

## Judgment And Limits

Within this audited authoring boundary, retaining the current canonical model and
its convenience APIs is supported by paired compiled execution, not just source
similarity or shape comparison. No independently editable duplicate authority or
production bug was demonstrated, so an API-breaking consolidation is not
justified. Independent final source/evidence review found no actionable defects
and supports the whole original A4 acceptance candidate. The integrating owner
also passed the full assembled workspace/default/no-default, isolated graph
no-default, formatting and strict Clippy gates. Exact committed-head hosted CI
remains required; this note does not assign completion percentages.

Finite fixtures do not prove all user-written factories implement their claims,
all hand-written `InputRecord` implementations use honest metadata, arbitrary
codec correctness, transactional reconstruction, safe payload detachment through
callback side effects, or runtime/closure/lifecycle correctness outside the
authoring boundary. They do not establish performance gains or equivalence of
physical identities after body replacement. Other agents' runtime and kernel
work is separate and is not claimed as evidence here.
