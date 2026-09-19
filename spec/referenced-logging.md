# Referenced logging

Phenix logging separates a compact chronological index from optional deep diagnostic content.

## Modes

`PHENIX_LOG_DEPTH` selects one of three explicit projections:

- `summary`: write only compact summary fields to the main log.
- `reference`: write the summary plus a typed `ContentReference`; full detail is stored in a content-addressed backend.
- `inline`: write summary and full detail directly in the main log.

The default remains `inline` for compatibility. Product frontends may choose a different default.

## Content references

Core owns the generic reference contract:

- canonical SHA-256 digest;
- media type;
- exact byte length;
- backend locator.

The digest verifies the exact referenced bytes independently of the backend locator. References may be nested inside referenced objects, so a detailed diagnostic can form a tree or DAG of progressively deeper evidence without expanding the main log.

Backends implement `ContentReferenceStore`. Core provides a filesystem CAS. `phenix.artifacts` projects its durable artifacts into the same `ContentReference` contract, so logs, artifacts, and memory/provenance systems can exchange stable references without sharing domain policy.

Backend selection must not create recursive tracing. In particular, a trace listener must not synchronously call a traced artifact service merely to persist the trace that caused the call. Physical backend reuse therefore happens below service-dispatch boundaries or through explicitly non-recursive adapters.

## File layout

For a file sink such as:

```text
append:/state/phenix/phenix.jsonl
```

reference mode defaults to:

```text
/state/phenix/phenix.jsonl
/state/phenix/phenix.jsonl.d/objects/sha256/ab/abcdef...
```

`PHENIX_LOG_STORE` overrides the CAS root. Console sinks require an explicit store root when `reference` mode is selected.

## Invariants

1. The main log remains useful without loading referenced content.
2. Referenced bytes are immutable and verified by SHA-256 on read.
3. Equal canonical JSON detail deduplicates to the same reference.
4. Locators are backend-specific; verification identity is backend-independent.
5. References are restart-stable and may be nested.
6. Sensitive values must be redacted before entering either the main log or the referenced store.
7. Logging/storage failures remain observational and must not alter the operation being traced.
8. Retention policy may delete unreachable detail objects later, but must never silently rewrite a reference to different bytes.

## Relationship to memory

Memory and logs have different policy but can share reference infrastructure. Memory owns semantic extraction, consolidation, freshness, associations, and recall. Logging owns chronological diagnostics and retention. Both may point at the same immutable referenced evidence instead of copying large source payloads.
