# P1 independent provider and authentication acceptance

This change splits three *control-plane* cases out of combined end-to-end
smoke tests. It adds separate acceptance outputs without removing existing
release-preflight evidence prematurely.

| Nix check | Concrete implementation | Sole responsibility | Forbidden dependency |
| --- | --- | --- | --- |
| `p1-api-key-control` | native OpenAI API provider, ACP | API-key challenge, cancellation, identity, no fake profile | model selection, Model invocation, user credentials |
| `p1-oauth-control` | native ChatGPT provider, ACP | PKCE browser challenge, pending, cancellation, replay denial | browser sign-in, model selection, Model invocation |
| `p1-model-discovery` | native loopback provider, ACP | two selectable concrete models, schema, invalid/foreign profile and revoked catalog | auth, session creation, HTTP Model invocation |

Every scenario receives only the relevant provider management composition and
the ordinary scripted Basic composition. That scripted model is **not called**:
the product harness allows only provider-management RPC calls. The discovery
fixture's endpoint points to an unreachable local sentinel; it can return
provider descriptors and models without a running HTTP server.

These are regression gates for *real* provider interfaces using controlled
concrete implementations. An advertised `phenix.provider.*` or
`phenix.model.*` plugin ID is never accepted as a concrete model ID.
Discovered model IDs and their protocol bindings are separate from the
`phenix.basic.model@1` interface and native plugin identity.

Preserved legacy acceptance:
- `p1-combined-auth-product` still tests independent identities and
  cross-connection grants in a single process.
- `basic-host-smoke` still performs selected-model execution and crash/restart
  routing tests because `p1-release-preflight` currently expects their outputs
  under the host directory.
- `provider-runtime-product` still tests the native provider runtime interface,
  including an explicitly unavailable Model execution adapter.

Follow-up: move the three bundled frontend/provider checks from
`basic-host-smoke` to their own Nix outputs, update
`tests/p1-release-evidence.py` to consume those outputs explicitly, and then
remove duplicate execution from host smoke. Keep release assertions unchanged.
In the same manner, split the existing combined-auth release evidence into
cross-connection policy vs independent API/OAuth evidence. Neither change
should create a production/test-only provider interface.
