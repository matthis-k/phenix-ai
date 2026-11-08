# Storage And Kernel Review

## Baseline

Inspected branch `rewrite/clean-foundation`, initially clean, ahead 1 and behind
1963 relative to its tracking branch. No merge or legacy import was performed.
The initial inspection found a canonical graph compiler/executor and a Rust-only
plugin catalog with leased contributions. There was no storage implementation,
kernel, SQLx infrastructure, native shared-library loader, or devenv configuration.
`flake.nix` supplies the repository workflow. This written baseline records the
initial inspection; the score table was written after implementation, not before it.

Scores assess this task's deliverables, not the quality of the existing graph.
An absent feature receives zero rather than credit for a prospective design.

| # | Category | Baseline | First Pass | Repair Pass | Final |
|---|---|---:|---:|---:|---:|
| 1 | Rust API clarity/inference | 2 | 8 | 9 | 9 |
| 2 | Macro-free completeness | 3 | 9 | 9 | 9 |
| 3 | Ownership separation | 7 | 8 | 9 | 9 |
| 4 | Plugin/schema interoperability | 4 | 7 | 8 | 8 |
| 5 | Honest backend neutrality | 0 | 8 | 9 | 9 |
| 6 | Native query expressiveness/reuse | 0 | 8 | 8 | 8 |
| 7 | Observation correctness | 0 | 6 | 8 | 9 |
| 8 | Deterministic graph composition | 7 | 8 | 9 | 9 |
| 9 | Kernel minimality | 0 | 9 | 9 | 9 |
| 10 | Lifecycle/failure recovery | 4 | 6 | 8 | 9 |
| 11 | Non-speculative extensibility | 5 | 8 | 8 | 8 |
| 12 | Tests/examples/verification | 5 | 7 | 8 | 8 |

## Implementation Passes

### Storage And Composition

Added `phenix-storage` with strict logical addresses, explicit grants, memory KV,
native rusqlite transactions and bounded invalidation. Added `phenix-kernel`
around existing catalog snapshots, contribution leases, GraphBuilder, Deployment,
and Engine. No foundation depends on the kernel. Kernel production dependencies
do not require storage. The storage binding is an example plugin, not an engine
hook or a second graph runtime.

First-pass defects: build activated producers; activation received the engine;
ordinary field ordering dropped engine nodes before cancelling producers;
SQLite callback panic poisoned the registry; subscription closure returned idle;
unsupported capability errors lacked capability identity. These reduced categories
1, 3, 5, 7, and 10. Fixed each rather than weakening tests.

### Lifecycle And Authority Repair

Separated `Kernel::build` from explicit `start`. Activation has no engine argument,
uses a temporary vector, and rolls back all acquired producers on error. Drop
cancels producers before engine teardown, with contribution leases retained last.
Tests in `kernel/tests/kernel.rs` check dependency rejection before composition,
compile failure before activation, retry, producer/node/plugin destruction order,
admission errors and wave-local invocation failures.

SQLite authorizer denies transaction/savepoint escapes, attachments, PRAGMAs and
pragma functions. Mandatory transactions reject VACUUM INTO. Callback panics roll
back and release the lock before resuming unwind. Tests assert rollback effects,
deferred commit failures, implicit rollback, absence of phantom notices, native
parameter binding, typed rows, and continued unrelated-mount use after panic.

### Adversarial Observation Repair

Independent read-only review found subscription destruction could reacquire the
registry mutex during transaction callback unwinding. The registry now keeps weak
watch registrations; a subscription owns its queue independently. Dropping it
never acquires the transaction lock. Dead registrations are pruned on subscribe
and emit. Timeout-bounded regression tests cover callback drain/drop and panic.
Queue drain does not wait for an event, but may briefly contend on a queue mutex;
neither drain nor producer polling is a wait-free API.

Added positive interoperability of independently defined Rust values/codecs in
separate modules, alongside all four authoritative schema-field mismatch tests.
Added actual storage producer teardown and partial-activation rollback/retry for
both backends. Independent waves, buffered notices, overflow, typed ingress
rejection and a sink that cannot create feedback are asserted in
`kernel/tests/storage.rs`.

## Evidence And Remaining Defects

1. **9, API:** synchronous typed `Session::get/put`, transaction closure row
   inference, and explicit `Kernel::build/start/fire/drain` are exercised by runnable
   examples. No async facade disguises blocking SQLite I/O.
2. **9, no macros:** every added API and both examples work without generated code.
   Existing graph macro-free tests and isolated no-default library tests are reused.
3. **9, ownership:** adapters own connections/queues; host issues sessions; plugins
   compose graph declarations; compiler and executor remain unchanged. See the
   public abstraction inventories in storage.md and kernel.md.
4. **8, boundaries:** storage checks nominal identity/revision/structure/codec
   metadata and independent codec interoperability is tested. However descriptors
   are author-attested strings, not a machine-checked structural schema shared with
   Graph. Graph's wire bounds still prove nominal schema only. No ABI is claimed:
   existing catalog is Rust-only and native ABI/version fixtures are inapplicable.
5. **9, backend honesty:** capability discovery distinguishes typed KV from SQL;
   unsupported operations identify their capability. Only invalidation is offered,
   with adapter-local source, scope, delivery metadata and explicit overflow.
6. **8, query:** native parameterized SQLite and typed row decoding reuse rusqlite.
   No SQL parser, dialect translator or universal AST was invented. But queries
   eagerly collect rows, DML RETURNING is not exposed, and PRAGMAs are unavailable
   through scoped SQL. Host-specific administration has no public native hook.
7. **9, observation:** ordering is registry-local; notification follows successful
   commit only. Rollback, overflow, closure, cancellation and external-connection
   noncoverage are tested. No live query, replay, reconnect or snapshot handoff is
   advertised. Separate snapshot/read plus invalidation is the supported model.
8. **9, graph:** deterministic topological contribution order, strict module
   exports and one compiled graph reuse the canonical compiler. Storage tests
   assert multiple pending notices each become distinct new Engine waves. Existing
   graph session tests cover concurrent/pending waves and selective routing.
9. **9, kernel:** no SQL, domain nodes, graph policy or storage registry is in core.
   Bounded explicit producer polling only admits events to Engine; no background
   orchestration, graph recursion control, hot reload or hidden execution budget.
10. **9, lifecycle:** inactive build, transactional producer activation and tested
    producer-before-node-before-lease teardown. Ordinary errors/panics roll back
    SQLite. Trusted constructors/compose callbacks must avoid setup side effects;
    arbitrary plugin side effects cannot be undone by this host.
11. **8, extensibility:** intentionally closed backend enum and small contribution
    contract avoid speculative driver frameworks. A third-party backend cannot be
    installed without editing storage. The example binding is not a reusable
    published Storage-to-Graph adapter. These are real limitations, not TODO stubs.
12. **8, verification:** runnable memory/SQLite slices and negative tests exist;
    existing graph tests are unchanged. New plugin fixtures use the available
    in-process loader boundary, not independent shared libraries. Markdown snippets
    are not automatically extracted as doctests. See verification record below.

## Stop Condition

The implementation is working but **does not meet all twelve >=9 thresholds**.
The concrete remaining blockers are a shared checked structural schema contract
across graph/plugin/storage, broader native SQL access without weakening scoped
authority, and a replaceable backend/binding contract with independently packaged
fixtures. Pretending descriptor strings prove codec structure, exposing raw SQLite
connections, or adding a speculative ABI/driver framework would not repair these
defects. Those changes require additional design and integration beyond this
minimal implementation. The review does not award unsupported passing scores.

## Verification Record

Initial bare-shell fmt/clippy were unavailable; the repository's Nix shell supplies
both. Initial fmt check failed on new code formatting; `nix develop --command cargo
fmt --all` repaired it. Exact final commands and outcomes are recorded below after
the final verification run.

| Exact command | Outcome |
|---|---|
| `cargo test --workspace` | Passed initially; final repeat with `--quiet` passed, including doctests |
| `cargo test --workspace --no-default-features --quiet` | Passed; dependency feature unification can still enable graph derives |
| `cargo test -p phenix-graph --no-default-features --lib --quiet` | Passed all 175 graph library tests in isolation |
| `nix develop --command cargo test --locked --offline --workspace --all-targets --quiet` | Passed, including benchmark/example targets |
| `nix develop --command cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | Passed |
| `nix develop --command cargo fmt --all --check` | Passed after formatting repair |
| `cargo run --locked -p phenix-kernel --example storage` | Passed both memory and SQLite slices |
| `cargo run -p phenix-kernel --example basic` | Passed |
| `git diff --cached --check` | Passed |
| `nix flake check --no-update-lock-file --print-build-logs` | Incomplete: 200-second attempt and 600-second retry timed out compiling release test targets; release libraries, sandbox formatting and Clippy completed successfully |

The runner prints `interrupted by the user` when its timeout terminates Nix;
this was a tool timeout, not a reported test assertion failure. The full flake
check is **not** declared passing. No devenv configuration exists on this branch.
New foundation tests total 28: 19 storage and 9 kernel integration tests.
