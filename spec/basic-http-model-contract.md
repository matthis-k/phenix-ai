# Pending HTTP model provider

> **Historical B4 implementation slice, not the desired provider onboarding experience.** This document records an environment-configured transport implementation. Future provider selection, authentication, and model discovery should use the accepted current contract design and owning migration PRs; a plugin implementation ID must not appear as a selectable model. Verify what still runs in source/tests. See [spec index](README.md) and [Graph-v2.4 target](graph-architecture-v2.4.md) for the distinction between current and target architecture.

B4 owns `phenix-plugin-model-http`. It implements the existing typed Model
contract and imports the selected EventJournal. The provider uses libcurl's
Rust wrapper for HTTP/HTTPS, certificate verification and caller-selected
timeouts. Endpoint, model, optional bearer credential and optional CA file come
from explicit `PHENIX_MODEL_HTTP_*` provider settings. Missing endpoint/model
fails closed. Requests follow the Chat Completions streaming protocol. The
provider follows no redirects and supports only HTTP/HTTPS endpoints.

Selected-provider installs are session-scoped and bind to the first nonempty
Basic run identity. All Model calls for that run, including tool continuations,
share the checked clearing configuration. Another run requires a fresh checked
install; a missing session, empty run or different run fails closed without
consulting preset environment credentials. The controller revalidates provider
auth and resolves the exact credential before installing each new turn. This
does not promise mid-run external credential revocation detection.

The authenticated begin callback checks the session resource and validates the
complete transcript, tool schemas, skills, context and caller limits before
starting network work. It rejects empty run identities and zero token/timeout
limits before effects. No execution timeout or generic caller token limit is added
when the caller omits those options. `model_tokens` bounds generated completion
tokens. The optional legacy HTTP preset `PHENIX_MODEL_HTTP_MAX_TOKENS` specifies
a provider-configured wire maximum; it must be a positive integer. Anthropic
Messages requires `max_tokens`, so an omitted caller limit uses this explicitly
configured provider maximum, while an explicit caller limit is constrained by
the configured maximum if present. If neither limit is present for Anthropic,
request encoding returns a typed InvalidInput *before network I/O*. OpenAI
Chat and Responses retain their existing absent-caller-limit behavior; this
does not add a kernel default or a parallel provider registry.
The API's function parameters use one object envelope, `{"value": <tool JSON>}`,
for every tool schema. Replies unwrap this envelope into the existing ToolCall
arguments. History uses the same mapping, so scalar and object tools preserve
their internal arguments without changing the ToolExecute contract.

One worker owns the libcurl transfer. It carries owned request data, a wake
permit and a cancellation flag, never a host scope. The incremental SSE decoder
assembles text and indexed function calls, preserves call IDs and JSON arguments,
and requires a terminal marker, finish reason and typed usage. Malformed replies,
unsupported content, incomplete streams and invalid calls return typed errors.
Portable payload bounds limit buffered data; they are not execution budgets.

Validated text deltas reach the journal through authenticated poll callbacks.
The worker cannot write plugin state or journal records. A bounded progress queue
coalesces text without losing final response data. Poll errors abort transport
and retain the worker until quiescent. Partial output can precede a provider
error; the error cannot become a successful final reply.

Cancellation sets the worker's flag and asks libcurl to abort. The existing
pending ticket retains the native image until the transfer closes and the worker
joins. Cleanup discards queued progress and performs no revoked journal writes.
It reuses the existing pending ABI, wakes, cancellation tree and physical leases.

An independent local HTTP fixture must verify request contents, split SSE/UTF-8
chunks, live journal progress before completion, multiple function calls, usage,
provider errors, malformed/truncated streams, caller limits, cross-session zero
requests and cancellation/draining. Local HTTPS tests prove trusted CA success
and untrusted certificate rejection. Full B4/P1 acceptance also requires pending
Basic-agent integration and frontend streaming/cancellation parity.

Protocol references: [Chat Completions](https://developers.openai.com/api/reference/resources/chat/completions),
[curl Rust callbacks](https://docs.rs/curl/0.4.51/curl/easy/trait.Handler.html).

## PR #1227 destination-policy correction

Implementation audit against base `88371239c3ad825df63c153c1d5e5bb91217f182`:
the three outbound HTTP constructors are Model `transport::run_with_resolver`,
discovery `http::fetch_bearer_catalog`, and SIWC `request_with_mode`. Each
authorizes the complete DNS answer before constructing credential headers or
copying a secret POST into curl, pins the authorized addresses on a fresh curl
handle, disables proxies and redirects, and retains the original URL hostname
for Host/SNI and mandatory TLS peer/hostname verification. SIWC token exchange,
refresh, JWKS, issuer discovery, revocation and catalog requests all use this
same transport. PKCE/OIDC helpers do not introduce another HTTP client; the
OAuth callback listener is inbound literal loopback, not an outbound transport.
Provider credential-store access is separately scoped to the authorized account
before plaintext access. Catalog/OAuth tokens can already exist in provider
memory before destination resolution; this is not a claim that no plaintext
exists before DNS, only that rejected destinations receive no credential bytes.

The classification follows the IANA IPv4/IPv6 Special-Purpose Address Registries
(both updated 2025-10-09), with a deliberately stricter IPv6 native-unicast rule
rejecting translation, mapped/compatible, Teredo, 6to4 and ORCHID/DETs prefixes.
Globally reachable native exceptions are not classified as private merely
because they are special-purpose. In particular, `192.0.0.1` was already
rejected at the base: its `192.0.0.0/16` exclusion was too broad, not a bypass.
The correction narrows that exclusion to the IANA blocks, honors the `.9`/`.10`
exceptions and restores globally reachable AS112/AMT entries. Boundary tests
cover denied ranges, adjacent addresses, all of `192.0.0.0/24`, native IPv6
exceptions, mapped/transition targets and mixed-family DNS answers.

Cancellation is checked immediately after resolution, before model credential
decoding. System `ToSocketAddrs`/NSS resolution itself remains synchronous,
outside curl deadlines and cancellation: a stalled resolver can delay worker
drain and retain its native-image lease indefinitely. There is no existing
bounded resolver abstraction in this tree. A detached thread with a receive
timeout would not cancel or bound the underlying resolver work; replacing NSS
with direct DNS changes semantics. The lifecycle-owned resolver follow-up and
deterministic stalled-resolution acceptance are tracked in
[#1230](https://github.com/matthis-k/phenix-ai/issues/1230). No bounded-DNS or
bounded-cancellation guarantee is made by this correction.
