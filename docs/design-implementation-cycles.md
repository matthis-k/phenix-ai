# Top-down, bottom-up, top-down implementation loop

All changes happen on the isolated rewrite. The old P1 source remains at \`reference/pre-clean-plugins-20261026\`. Each step records the intended API, actual compiled contract, discovered friction, fixes, and final author-facing API. The v2.4 spec is a **target**, not evidence of shipped behavior.

## Phase A: top down, choose the call sites

Start from what a plugin author or a graph consumer types. Define its public ports, optional extension points, accepted outcomes, failures and scope. Count user-written operations and boilerplate. Refuse author-facing features that exist only to work around the current framework.

| Cycle | Target call site | Observable acceptance | Implementation substrate |
| --- | --- | --- | --- |
| C0 | Pure \`#[provide]\` stateless handler | Native fixture exports one operation, no State | Existing SDK ABI, typed contract |
| C1 | \`graph.module(...); m.invoke; m.entry; m.exit; graph.flow\` | Type-check, compile offline, inspect and lower stable boundaries | Portable graph-v2 IR, SDK handles, compiler |
| C2 | Basic prompt to model to answer | One native selected request/response cycle | Flow scheduling and host dispatcher |
| C3 | Model request to registered tool batch back to model | Typed branches and deferred bounded waves | Switch, Merge/Join closure, tools policy |
| C4 | Provider data declaration and shared login | Restored verified auth/model without selecting each time | Shared auth strategies and storage, provider manager |
| C5 | Memory and compaction module attachment | Verified retrieval; changed candidate admitted after CAS+remeasure | Evidence and checkpoints, meter |
| C6 | Advanced internal graph patch without public API break | Exact precondition and authority checks, traceable patch | Offline patch compilation and provenance |

### C0 extension: source-owned contracts and State

Start from one plugin-owned contract and its request/response/error structs. Derive Portable and operation signatures, then inspect the actual ABI metadata. Test State mutations, isolation and error atomicity. Only after this compiles should the graph authoring cycle wire the same typed contract operations into module interfaces. Do not add duplicate registry entries or author-written ABI metadata to satisfy the compiler.

## Phase B: bottom up, build only what's needed

For each call site:
1. Write the smallest executable fixture and a failing acceptance test at the public interface.
2. Identify the missing **generic guarantee** and its owner (kernel, contract, SDK, or shared domain helper).
3. Implement that guarantee **once**, without shipping another plugin or old-format adapter.
4. Confirm schema encoding, selected imports/grants, pending/cancel behavior, exact root/wave and generation boundaries as applicable.
5. Record surprises as an explicit friction item; do not add a user-visible knob unless there is a genuine domain decision.

## Phase C: top down again, simplify

After the tests run, rewrite the same author's call site against the implementation. Count required declarations and duplicated wiring again. Remove any unnecessary glue, keep only stable interface points, and test the final source. Do not mark a cycle complete merely because its internal unit tests pass.

## Acceptance and trace ledger format

| Field | Record |
| --- | --- |
| Baseline | Exact parent SHA and retained reference behavior |
| Intended UX | Minimal end-user/plugin-author call site |
| Actual | Complete compiling usage example, or explicitly not yet implemented |
| Code | Added/removed files, exact commit |
| Verification | Exact-head CI, native fixture, E2E, fault/restart, or not run |
| Friction | Mechanism and ownership, not only an error string |
| Simplification | Boilerplate/adapter removal performed after feedback |
| Result | Implemented, tested, integrated or blocked, each independently |

## Rules for deciding to create a plugin

A new plugin is justified only for independently selected domain behavior, authoritative private domain state, or concrete external effects. A pure data transform, graph boundary, graph patch, generic registry or ABI glue is normally a contract/SDK/internal helper, not an additional native plugin. Shared OAuth/transport logic can be reusable domain libraries; tiny provider definitions are declaration-only contributions.

Avoid a fixed plugin count. Optimize **small author surface and clear responsibility**, not artificially one native crate per graph node or one monolith for the whole product.

## Current checkpoint

- **C0:** two minimal native fixtures and Cargo tests exist; native composition through the kernel is still unverified.
- **C1:** module boundaries and graph-v2 proposals exist in the spec, but do not yet compile through the public SDK.
- **C2-C6:** planned, not implemented on the clean rewrite.
