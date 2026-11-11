# FBP agent-composition acceptance slice

Status: Phase 1 design note, after whole-drain lifecycle work. No second
runtime and no special-cased agent/kernel types.

## Phase 0 — requirement versus suggested implementation

Requirement: a host can start from an empty FBP kernel, install an ordinary
agent-preset plugin, and let independent plugins replace high-level semantics
(model, tools, memory and compaction) by exact authorized graph edits. The
preset must not be privileged. The test must demonstrate replacements and
revocations actually change execution, not merely metadata.

No new abstraction is required: `ComponentDef::map`, `NetworkPatch`,
`Registration`, `Composer`, `Edit::Splice`, and `NetworkSession`
already support the intended boundary when the preset declares each semantic
stage as a **root-public component**.

## Phase 1 — alternatives

| Approach | Strength | Weakness | Decision |
|---|---|---|---|
| Leave example-only coverage | No new code | No test proving ordinary-plugin agent editing | Reject |
| A privileged `AgentLoop`/compiler mode | Polished surface | Violates FBP kernel/plugin separation | Reject |
| Public root stages with exact grants and compiled runtime test | Uses existing abstractions; verifies replacement/splice/revoke | Does not permit editing private internals of opaque composites | Selected |

Test composition:

`prompt -> memory -> model -> tools -> reply`

All are ordinary `ComponentDef` primitives. A plugin `preset` adds them,
connects them and exports public `prompt`/`reply` from an initially empty
`Composer`. A separate `provider` plugin replaces `model` with the same
interface. A separate `compaction` plugin adds a process and splices it between
`memory` and `model`. Run actual sessions before/after, then revoke and
confirm transitive dependencies and behavior. All registration origins are
ordinary plugin origins and grants are exact footprints; no plugin gains
implied namespace or descendant authority.

**Limitations:** This is an architectural seam proof with deterministic string
processes, **not** an actual LLM, persistent memory store, tool protocol,
multi-step reasoning loop, tool sandbox, concurrency validation, or a solution
to crossed-request FIFO head blocking. Nested private edits remain forbidden;
the preset explicitly exposes the intended edit points as public root nodes.
No claim of complete P1/P2 agent functionality is authorized.

## Phase 4 acceptance

The integration test must exercise compile+execute and revoke from the same
`Composer`. Do not add a new public agent API or bypass host grants; preserve
existing graph/compiler semantics and full exact-head CI. Functional test
assertions must change when independent plugin registration changes.
