# P1 verification boundaries

This document describes test composition and acceptance, not a new runtime
abstraction. The kernel and production plugins must not import test fixtures.

## Rule

Test the smallest contract boundary that can prove the behavior. Each check
uses the real consumer and a narrowly substituted provider of the existing
interface. Do not bundle login, external HTTP, model invocation, tool selection,
session restoration, and frontend rendering into a single acceptance gate.

A **contract** describes an operation; a **test implementation** supplies its
behavior; an **acceptance scenario** invokes the production consumer against
that implementation and checks observable outcomes. An interface ID is not a
concrete, selectable model.

## Acceptance matrix

| Boundary | Fixture or real implementation | CI acceptance | Needs external credentials? |
| --- | --- | --- | --- |
| ABI, graph, authority, cancellation | deterministic conformance plugins | `m1-*`, `b2-*` | No |
| Tool registration and executable routing | real Basic Tools plus tool-contributor fixture | `tool-contributors` | No |
| Model request projection and capabilities | real Basic agent + `phenix.model.history-test` | `basic-model-capabilities` | No |
| Model replies, errors, limits and continuation | `phenix.model.scripted` | `b4-scripted` and agent checks | No |
| Multi-turn history, storage and recovery | `phenix.model.history-test` + real Basic Session | `basic-history-verification` | No |
| Frontend ACP contract | verification composition + ACP frontend | `basic-history-verification` ACP scenario | No |
| Provider registration and model discovery | provider builtins and loopback fixtures | provider/ACP product checks | No |
| Credential storage, auth policy and lifecycle | credential-test backend or synthetic transport | authentication product checks | No real provider secret |
| Production provider interoperability | actual network service and credentials | opt-in live smoke check (not a required offline CI gate) | Yes |

The `basic-model-capabilities` check is intentionally distinct from both
history verification and auth verification. It drives the real Basic graph
over JSONL; the selected test-only Model returns the exact `ModelRequest`
snapshot available at the Model contract: tool IDs, descriptions, JSON argument
schemas and idempotency; skill IDs; context IDs. It never claims that a tool
is executable merely because it was advertised. The check separately observes
a real tool call and result before accepting the wiring.

Descriptions, instructions, full context bodies and secrets are **not** part
of the capability observation. Skills and context are identified, not dumped.
The observed catalog is compared against the expected installed Basic tools.
Capabilities are scoped to the individual request and are not global model
features or an invented remote entitlement.

## Failure attribution

- Catalog absent or wrong: investigate Basic tool registration, catalog
  discovery, and ModelRequest projection. Authentication is out of scope.
- Advertised tool cannot execute: investigate authority, tool dispatch or
  result continuation. Provider login is out of scope.
- Auth lifecycle fails: investigate auth adapter, transport, credentials or
  persistence. Model/tool execution is out of scope.
- Model catalog contains a contract ID (for example `phenix.model.http`)
  instead of an entitled concrete model: investigate provider discovery and
  selection, not the Basic Model interface.
- History check fails but capability check passes: investigate session journal,
  replay, transcript ordering or persistence.
- ACP fails but JSONL passes: investigate frontend protocol/operations rather
  than the model fixture.

## Isolation invariants

1. Offline checks must never call a real provider endpoint or require user
   credentials. Sanitize relevant API key environment variables in product
   tests, and use isolated test state directories.
2. Keep mocks in test-specific plugin implementations and compositions. The
   history verification plugin is excluded from the default Basic closure.
3. Reuse the real typed `Model`, `Tool`, and provider contracts. Do not
   introduce test-only variants of production interfaces.
4. Test fixtures produce deterministic responses from their input, never from
   hidden shared state. Statefulness must be proven through the real session
   owner and storage.
5. Production tests assert user-observable outputs, real authority decisions,
   and expected negative cases. A successful fake response by itself is not
   acceptance evidence.
6. Distinct Nix checks should cover separate failure domains. A change to
   OAuth transport must not cause offline graph/agent tests to require OAuth.
7. Live provider checks are separate, opt-in and report configuration/network
   failures explicitly; they never gate deterministic P1 execution acceptance.

## Follow-on isolation

The remaining cleanup is to split existing broad ACP and combined auth tests
into finer fixtures: provider catalog, auth challenge lifecycle, credential
persistence, entitlement/discovery, selection, frontend default actions, and
real external transport. Implement one fixture and CI gate per boundary while
preserving existing coverage. Do not fork provider runtime logic to achieve
testability; swap concrete implementations at their existing contract slots.
