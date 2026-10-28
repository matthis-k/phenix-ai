# Contributing to Phenix

Phenix uses a small kernel and replaceable plugins. The kernel owns generic execution mechanics: verified loading, typed invocation, composition, authority, graph scheduling, cancellation, and generations. Plugins own application behavior and state.

This guide describes **how to contribute**, not a frozen description of the current implementation. **Source code and verified tests at an exact commit tell us what is implemented. Specifications describe intended behavior, often not yet implemented, and may be older than the code.** The latest graph design proposal is [v2.4](spec/graph-architecture-v2.4.md); it is a target, not the current graph ABI. Check [the specification index](spec/README.md) and the owning PR before using any older design text. Neither an open PR nor an unverified specification closes an implementation gap.

## Find the owner before changing code

| Question | Source |
| --- | --- |
| Which implementation milestone owns this work? | [Specification index](spec/README.md), [M1 roadmap](spec/m1-roadmap.md), and [P2 roadmap](spec/p2-advanced-agent-roadmap.md) |
| What does the current kernel accept? | Current code in `crates/phenix-contract/` and `crates/phenix-kernel/`, plus tested product compositions; the [M1-only runtime contract](spec/m1-only-runtime-contract.md) is supporting design context |
| Who owns live state and persistence? | [Plugin state ownership](spec/plugin-state-ownership.md) |
| Which plugin responsibilities overlap? | [Plugin responsibility audit](spec/plugin-responsibility-audit.md) |
| What are the proposed author-facing plugin APIs? | [Usage-first architecture](spec/usage-first-plugin-architecture.md) |
| How do I build and test? | [Development guide](DEVELOPMENT.md) |

Read the linked spec and implementation PRs when changing an area. Specifications are design snapshots, not automatically accurate descriptions of `main`. Older graph design proposals are superseded for future graph-v2 work by [v2.4](spec/graph-architecture-v2.4.md), but existing M1 graph-v1 code remains valid until a tested migration replaces it. Inspect the relevant source, accepted tests, and current PR merge state. Update the existing owner rather than creating a second roadmap.

## Specification change policy

A spec records the desired design, even when the code does not implement it. New or substantially changed specs must state their **status**, **date and inspected commit**, **scope**, **superseded predecessors**, **owning roadmap/PR**, and **acceptance tests needed to close the gap**. Use these status meanings:

| Status | Meaning |
| --- | --- |
| **Proposed** | Design under review; implementation may be absent |
| **Accepted design** | Approved target semantics; not evidence of implementation |
| **Partially implemented** | Some requirements have code; remaining requirements must stay explicit |
| **Verified / integrated** | A specific scope passed acceptance at a named commit; not blanket acceptance of the entire spec |
| **Superseded / historical** | Retained for context; no longer determines future changes |

Check the [spec index](spec/README.md) before writing new guidance. Prefer amending the latest relevant specification and referring to the implementing PR. When a new spec supersedes an old prescription, replace the obsolete active text with a short link to the successor and preserve historical source in Git history. **A design requirement remains open until implementation and its acceptance gate are verified.** Do not close a gap by editing the spec to match incomplete code without an explicit design decision.

## Roadmap and PR workflow

Follow the dependency order agreed in the owning roadmap:

1. Stabilize the existing P1 product and its tested user workflows.
2. Refactor to a **behaviorally equivalent P1** with shared mechanisms and simpler plugin interfaces. Replace old paths only after parity tests pass.
3. Add P2 behavior through explicit extensions to the accepted architecture.

Independent tasks may proceed in parallel. Do not treat a planned refactor as a prerequisite for an unrelated bug fix.

For each PR:

1. Link its roadmap issue or existing milestone and identify the affected behavior.
2. Show the intended usage before introducing new interfaces, registries, or macros.
3. Assign responsibilities to the kernel, domain owner, concrete implementation, adapter, and composition.
4. Write tests that fail for the defect or missing behavior, including denied or invalid cases.
5. Implement the smallest coherent change using existing typed contracts and Rust libraries.
6. Verify the **exact commit** with focused tests, product tests, and the required CI checks.
7. Report **implemented**, **verified**, and **merged/integrated** as separate states. Record the remaining user test or dependency.
8. Update the owning roadmap and submit the PR under the repository's current review policy.

Use this PR summary structure:

```md
## Roadmap
Owning issue, dependencies, and acceptance condition.

## Before
Observed behavior or design limitation.

## Change
New behavior, responsibility changes, and removed paths.

## Verification
Exact SHA, commands, failures tested, product evidence.

## Result
Implemented / verified / integrated, outstanding work.
```

Do not present an unmerged PR, a compiled library, or a successful unit test as evidence that the distributed application behaves correctly. Verify the selected composition and frontend.

## Design principles

### Start with usage, then choose the pattern

The standard plugin should express only its identity, configuration or registrations, domain-specific code if needed, and private state. Shared code handles common lifecycle and integration work. Prefer established Rust libraries and the existing M1 contract and composition mechanisms.

These are **review guidelines**, not claims that every subsystem currently follows them.

| Pattern | Use it for | Avoid |
| --- | --- | --- |
| **Ports and Adapters (Hexagonal Architecture)** | Separate typed interfaces from transport or storage implementations | Exposing an implementation ID as a domain entity |
| **Strategy** | Select interchangeable algorithms such as authentication, discovery, or preparation | Copying the same orchestration flow into every implementation |
| **Composition Root / Dependency Injection** | Select and authorize concrete implementations explicitly | Treating a user-supplied ID as permission to load or invoke a plugin |
| **Registry and Catalog** | Validate registered contributions and publish current domain entries | Equating registration with authentication or execution authority |
| **Separation of Mechanism and Policy** | Keep generic scheduling, authority, and cancellation in the kernel; put domain decisions in plugins | Adding provider-, tool-, or frontend-specific branches to the kernel |
| **Single Responsibility Principle** | Give each authoritative lifecycle one owner | Distributing one state machine across several unrelated implementations |
| **State Machine** | Define states, allowed operations, transitions, and typed failures | Implicit transitions and success-shaped fallback behavior |
| **Event-Driven Architecture** | Connect events and state changes to graph reactions | Inventing another scheduler or interpreting commands as completed events |
| **Strangler Fig** | Replace working behavior in independently tested slices | Permanent parallel paths or removing the old path before parity |

A **Strategy** varies behavior; an **Adapter** translates between interfaces or protocols; a **Handler** implements an operation; a **Manager** coordinates a domain lifecycle. Use these familiar names only when their responsibilities match. A registry is a domain-owned lookup, not a global service locator.

### Prefer one shared mechanism and small concrete contributors

For repeated integration work, design the public extension first. A common target is a manager that accepts immutable typed registrations, owns the domain lifecycle, and invokes selected implementations or strategies. Individual contributors should add data and their genuinely distinct behavior.

This is a **design preference**, not the present state of all Phenix providers, tools, and frontends. Do not introduce a universal manager or an extra kernel registry. Reuse existing ports and selected imports when possible.

The provider and tool migrations have their own implementation status in the [plugin audit](spec/plugin-responsibility-audit.md), [usage-first proposal](spec/usage-first-plugin-architecture.md), and linked PRs. Do not copy an unfinished proposed API into new plugins.

### Separate types, signatures, interfaces, and behavior

Use the terminology already represented in M1:

| Concept | Existing M1 representation | Scope |
| --- | --- | --- |
| **Type declaration / data shape** | Rust types and canonical `ValueSchema` | Required fields, option types, closed variants, tagged alternatives, and references |
| **Operation signature** | `OperationSchema` | Operation name, request, response, and error schemas |
| **Interface definition** | `ContractDefinition` | Contract ID, revision, and grouped operations |
| **Contract metadata** | `ContractMetadata` | Plugin identity, definitions, imports, exports, and properties |
| **Runtime semantics** | Owning kernel or plugin implementation | Authority, effects, cancellation, persistence, and recovery |

These structures **already exist** in [`crates/phenix-contract`](crates/phenix-contract/src/descriptor/metadata.rs) and [`ValueSchema`](crates/phenix-contract/src/reference.rs). SDK contract macros are described in the [authoring contract](spec/m1-contract-authoring-contract.md). Do not introduce a parallel contract representation without an accepted specification and migration plan.

Prefer types that make invalid **structural states** impossible. Use closed enums rather than unrelated boolean flags, optional values rather than sentinels, and distinct identifier types when identities must not be confused. Current implementations do not uniformly use those representations; a recommendation is not proof of a type-level guarantee.

The current `ValueSchema` does **not** express general refinement predicates such as `x < 40`. Enforce such domain conditions in the responsible operation and return a typed error. Do not describe them as compiler- or schema-enforced guarantees. A future refinement-type extension requires its own specification, ABI and decoding semantics, and conformance evidence.

Type checking establishes structural properties of typed Rust code. Untrusted ABI or protocol bytes must still pass canonical decoding and schema validation. A valid type cannot by itself prove that a credential remains valid, an operation is authorized, or a write is crash-safe. The relevant runtime or plugin owner enforces those **behavioral semantics**; the owning specification and tests document them.

### Keep interfaces separate from domain entities

`Model` is an interface with an operation like the **existing** `complete(ModelRequest) -> Result<ModelReply, BasicError>`. `phenix.model.http` is an implementation plugin. A provider's model ID is a domain entity.

Do not infer that a plugin ID, contract ID, protocol adapter, or import slot is a selectable model. Model choices require a current validated catalog and admitted routing. The current ACP implementation has a known exception: `phenix.model.http` is still offered as a model choice. Treat this as a bug, not established design. Track the runtime model-selection work through [PR #1161](https://github.com/matthis-k/phenix-ai/pull/1161) and the owning roadmap. An open PR is not a shipped fix.

### Own state in the responsible plugin

Follow [plugin state ownership](spec/plugin-state-ownership.md). Plain Rust fields are the default for live state. Durable storage holds records, settings, credentials, and recovery data according to the domain's persistence semantics. A stored value is not automatically current authority. Restoration revalidates identity, scope, and revisions.

A contributor must not gain execution rights merely by registering metadata. The trusted composition selects imports and supplies grants. A frontend renders authorized state and forwards commands; it should not duplicate the domain lifecycle.

### Keep commands, events, and graph wiring distinct

A **command** requests an action and may fail. An **event** identifies an occurrence. A **query** reads authorized state. A **trigger** schedules graph work. A **graph edge** expresses the relation specified by its versioned graph representation. Graph-v1 `after` means ordering, while the proposed graph-v2 distinguishes `Flow`, `Order`, and `Deferred`.

Use these meanings in designs and tests. Do not claim that proposed shared default frontend operations, model-before-session selection, or custom graph-event emission already ship. Their changes are tracked in [PR #1161](https://github.com/matthis-k/phenix-ai/pull/1161) and [PR #1148](https://github.com/matthis-k/phenix-ai/pull/1148), respectively. Kernel graph semantics have their own specification in [M1 reactive DAG RFC](spec/m1-values-reactive-dag-rfc.md).

## Implementation status and spec drift

Do not maintain an as-built status table in this guide. Check the source tree, exact-head tests, and the merged state of the owning PR. The [specification index](spec/README.md) classifies target design, active implementation constraints, milestone evidence, and superseded proposals. Update a target specification when the intended design changes; update tested implementation evidence when behavior ships.

A gap between a specification and `main` is **open work**, not an inconsistency to resolve by rewriting either side as though it were already done. For graph work, [v2.4](spec/graph-architecture-v2.4.md) is the latest target, while the existing M1 graph-v1 implementation is the current behavior. Follow the staged P1 stabilization → graph-v2 P1 parity → P2 transition. Preserve tests and service until replacement parity is proven.

## Verification

Run focused Rust and product tests for the affected interfaces and full acceptance gates when required by the owning roadmap.

```sh
nix develop
nix flake check -L
nix run .#phenix-m1-demo
nix build .#phenix-m1-release
```

Use [DEVELOPMENT.md](DEVELOPMENT.md) for focused checks. For structural API changes, test compile-time rejection where possible and malformed schema or ABI inputs at the boundary. For behavioral changes, test invalid identities, missing authority, state conflicts, cancellation, restart, and actual selected compositions. Keep deterministic fixture plugins separate from public production packages.

Report both executed tests and tests you could not run. A claimed result needs the tested SHA and environment.

## Glossary

| Term | Meaning |
| --- | --- |
| **Type declaration** | A named type and its permitted structure. |
| **Schema** | A machine-checkable description of a value's shape. M1 uses `ValueSchema`. |
| **Operation signature** | The typed inputs, outputs, and errors of one named operation. |
| **Interface / Port** | A named set of operations required or provided by a component. |
| **Contract** | The API agreement. In M1, distinguish its machine-readable definitions from behavioral requirements documented elsewhere. |
| **Implementation** | Code that fulfills an interface. |
| **Domain entity** | An identified thing in the application, such as a model or tool. Not an implementation plugin. |
| **Adapter** | A translation between interfaces or external technologies. |
| **Strategy** | An interchangeable algorithm or policy. |
| **Handler** | The function that processes an operation. |
| **Manager / Orchestrator** | A component coordinating a domain lifecycle and state. Not a kernel primitive. |
| **Registration** | A typed extension contribution. Registration grants no authority. |
| **Registry** | Validated identity-to-contribution lookup. |
| **Catalog** | Published domain entries derived from registrations and current state. |
| **Plugin** | A packaged contribution or implementation selected through the composition. |
| **Composition Root** | The trusted location that selects implementations and grants authority. |
| **Binding** | The selected connection between an import slot and an export. |
| **Authority grant** | Permission to invoke an operation on a resource under an authenticated scope. |
| **State** | Mutable data with a defined authoritative owner. |
| **Snapshot** | Recorded state used for restoration and validated on load. |
| **Journal** | An ordered record of domain events, distinct from diagnostic logs. |
| **Command** | An attempt to change state or cause an effect. |
| **Event** | A fact about an occurrence. |
| **Query** | An authorized read operation. |
| **Trigger** | A declared condition for scheduling graph work. |
| **Revision / CAS** | A version and compare-and-swap guard against stale updates. |
| **Conformance test** | A test of behavior expected from any implementation of an interface. |
| **Acceptance gate** | The tests and evidence required to verify a roadmap item. |

## Review checklist

- [ ] The change has an existing roadmap owner, or explains why a new specification is needed.
- [ ] The proposed usage is simpler and does not duplicate shared mechanics.
- [ ] Existing interfaces, implementation code, and planned APIs are clearly distinguished.
- [ ] Type-level claims are enforceable by Rust or canonical schema decoding.
- [ ] Behavioral requirements have a named owner and appropriate tests.
- [ ] Registration, catalog visibility, authorization, and executable readiness are not conflated.
- [ ] The PR cites an exact revision and tests the selected product, including failures.
- [ ] Old paths are removed only after verified parity.
- [ ] Documentation distinguishes target specifications, the currently implemented code, and exact-head verified acceptance evidence.
- [ ] Older design snapshots are marked superseded when their prescriptions conflict with graph-v2.4; historical tests and current M1 behavior remain discoverable.