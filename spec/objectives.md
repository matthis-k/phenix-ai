# Objectives

status: implemented

## Ownership

Objectives are durable workspace-owned desired states. The runtime owns their identity, lifecycle, criteria, and links to executions.

A root objective comes only from explicit user intent. Agents cannot create or rewrite a root objective. Later explicit user intent may create a new root that supersedes an earlier root.

Agents may create derived objectives inside the scope of an existing root.

Non-root model-agent executions receive one runtime-owned objective semantic tool when they have an objective assignment. The tool may inspect the assignment and create or evolve derived objectives only within that execution's root lineage. It never exposes root creation or root mutation. Agent-driven lifecycle transitions record the calling execution as their cause. Root executions keep user intent as the root ownership boundary and do not receive this mutation tool.

## Shape

Objective ownership is a tree. Every derived objective has exactly one canonical parent. Cross-cutting relationships use references rather than additional parents.

An objective contains:

- stable identity;
- workspace identity;
- root or derived origin;
- a desired-state statement;
- structured success criteria;
- lifecycle state;
- optional supersession identity.

Each success criterion is either required or optional. Required criteria gate completion.

## Drafts and enactment

A derived objective may exist as a draft. Agents may revise a draft before activation.

Activation enacts the objective. After enactment, its desired-state meaning and criteria are immutable. A semantic change creates a successor objective rather than editing the enacted record.

Root objectives are enacted when accepted from explicit user intent. They are never agent-editable drafts.

## Lifecycle

Objective states are:

```text
draft
active
completed
failed
invalidated
abandoned
superseded
```

`failed` means the desired state was attempted and could not be achieved under the recorded work. `invalidated` means new evidence made the objective or its assumptions no longer valid. `abandoned` is an explicit choice to stop pursuing it. `superseded` points to a successor objective with changed intent or meaning.

Completion is a runtime transition. Every required criterion must have durable evidence before the transition succeeds. Optional criteria do not block completion.

Transition causes are durable provenance. Agent-action and execution-outcome causes must name recorded executions. Evidence-assessment references and policy descriptions must be non-empty. Replay validates the same cause contract as live mutation.

## Execution links

Every execution has one primary objective. It may support additional objectives.

Child creation does not implicitly change objective ownership or rewrite the objective tree. The runtime records the objective identities on the execution so replay preserves the same semantic work assignment.

## Context contract

This slice records the exact objective assignment needed by context projection. The later context-catalog and execution-projection slices must include the active primary objective as mandatory content.

Completed objectives leave mandatory context once they are no longer required by active work. They remain durable and addressable as compact referenced history after the references and history slices add that representation.

## Persistence

Objectives and criteria live in the canonical workspace SQLite database. Enacted objective meaning is append-only. Lifecycle transitions, completion evidence, supersession, and execution links are durable relational facts.

Objective semantics have one explicit durable activation boundary. When opening a journal that predates objective semantics, the runtime records that boundary before new objective-aware work and derives primary assignments for existing executions from their immutable root user input and parent lineage. Replay requires one primary objective for every execution covered by the activation event.

Deletion must preserve referential integrity. A referenced objective cannot silently disappear.

## Scope

This slice owns objective identity, root and derived ownership, draft activation, immutable enacted meaning, lifecycle transitions, success criteria, evidence-gated completion, supersession, execution links, relational persistence, replay, and focused regressions.

Exact general-purpose durable references, common context discovery, model-context projection, and compact historical representation belong to later slices.
