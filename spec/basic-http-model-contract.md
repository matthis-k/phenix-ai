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

### Selected installation lifecycle (1 November 2026)

Dependent implementation on #1231, based on `298108cf1c1ff0d494d81e68a5e9cb7869420c4c`;
not merged or default-product migration. Selection persists nonsecret intent
only. Each prompt revalidates authority/auth and resolves credentials immediately
before install. The typed install returns a monotonically increasing instance-local
token under contract `phenix.provider.http-model-session@2` (not compatible with
revision 1's older install signature). The token is an
`u64` token; session-scoped release removes only that exact installation, so stale
cleanup cannot remove a replacement. Both install and release grants are required
before credential resolution. The controller releases after success, failure and
cancellation, including pre-Model admission failures. Model cancellation and worker
errors revoke same-run reuse; selected mode never falls back to preset settings.
If explicit release fails, the controller removes the session kernel from future
admission and attempts shutdown. A primary execution error takes precedence over
the cleanup error; cleanup failure after success is returned as an error.

Release revokes reuse, not physical ownership: an active worker's `Arc<Config>`
retains clearing credential storage until the worker drops it. A channel-controlled
unit test proves revocation before worker drain and final configuration drop after
drain. `selected_provider_cancellation.py` exercises actual V2 selected-provider
ACP cancellation before headers, after observed text, during partial tool assembly,
on tool continuation and with controlled completion-winning and cancellation-winning
ordering. Duplicate cancel, exactly one durable
terminal, peer socket closure, no late output and a fresh same-session prompt are
checked using server/observer handshakes. Completion-winning requires one Answer;
cancellation-winning withholds completion bytes until the Cancelled response.
Numeric loopback socket closure
does not measure internal native-image lease counts or stalled NSS resolution.

### Transitional lifecycle audit (1 November 2026)

The lifecycle stack #1232, #1233 and #1235 is now merged into the selected-provider
integration branch at `f5c6532f15322db443e7523bb76758b75b9b1ab7`, not the default
product. Exact-head generated CI verifies 89 derivations and two runnable packages.
The selected A/B fixture now directly observes final kernel allocation destruction
with weak-reference probes, separately from native state destruction and joining.
Existing-session installation replacement fails with a typed Conflict even when
the caller bypasses controller ordering.

The first #1234 increment shares `session_lifecycle::SessionAdmission` across ACP,
terminal and JSONL: typed validation, ownership, recovery, graph admission and
durable markers. `SessionOwnership` fences observer replay without authorizing
recovery or execution. Real ACP/JSONL contention and replay tests exercise this
boundary; existing cancellation and recovery products remain acceptance gates.
This is partial extraction: provider management, exact credential resolution,
installation, terminal release and quarantine remain ACP-owned. In particular,
non-ACP selected-provider release-fault parity is not established by shared session
admission. #1234 remains open until both callers use those same domain behaviors.

**Historical pre-merge audit.** At the time of inspection HEAD was
`361eef337b3de2cecd6c3e9bbb878ebf925e2e28`; [#1233](https://github.com/matthis-k/phenix-ai/pull/1233)
reported exact-head acceptance on top of parent
[#1232](https://github.com/matthis-k/phenix-ai/pull/1232). Both were then open.
The dependent acceptance increment moves typed prompt decoding before session
ownership/recovery and selected installation, retaining ownership through final
release. Local packaged `basic-host-smoke`, `b4-http` and `m1-generations` pass;
that evidence is separate from exact-head remote CI and merge status.

The remaining ACP orchestration is transitional, not the target ownership:
`Controller::{prompt,install_current_http_model,drive}` in
`crates/phenix-basic-runtime/src/acp_sdk.rs` still coordinate selection,
credential acquisition, installation, cancellation and release. The dependent
extraction is [#1234](https://github.com/matthis-k/phenix-ai/issues/1234),
after #1233 and parent #1232. It must reuse the existing typed domain contracts
and kernel lifecycle, leaving ACP as typed ingress and authorized observation;
it does not introduce another scheduler or change Graph-v2.4 decisions.

The exact existing boundary is not a single callable domain lifecycle today.
`model_selection_state::{read,compare_and_swap}`, provider management
`current_http_binding`, the exact resolver's `resolve_for`,
`HttpModelInstall::from_checked`, and `HttpModelRelease { session, installation }`
already supply the domain pieces. `main.rs` supplies `session_lock`,
`recover_owned_run`, `begin_submission`, `mark_admitted_run`,
`finish_submission` and `record_cancelled`. Extracting their coordination is
feasible without inventing execution machinery, but requires parity tests and
a frontend-neutral boundary rather than simply relocating an ACP closure.

Installation uses `Kernel::admit_root` and `Root::invoke` before graph admission;
final release uses another root after execution drops/drains. During cancellation,
`GraphExecution::invoke_selected` uses the existing authenticated graph root.
A physically pending Model can reject that control call with
`ReentrancyRejected`; cancellation revokes reuse and final release retries after
drain. The kernel's `active_root` excludes concurrent roots, so admitting a new
root inside a live graph callback is not a valid extraction. `begin_submission`
returns a borrowing `GraphExecution` and does not run its first node; ownership
marking waits for `pump` to commit Running. Preserve these lifetime and ordering
constraints, exact-token release, primary-error precedence and failed-cleanup
quarantine. Concurrent-root support, if needed, is separate kernel lifecycle/ABI
work, not a frontend scheduler.

Fallback routing also remains transitional. `current_model_id` and `model_options`
can expose the launcher-installed implementation ID as a legacy option when no
selection is recorded. `prompt` can use the preset only before the session has
entered selected mode. `selected_http_seen` prevents a withdrawn record or failed
selection CAS from silently restoring that preset in the controller's lifetime;
it is not durable evidence that a deleted selection existed across restarts.
Do not generalize this historical route into the desired catalog model identity
or a selected-provider failure fallback. Remove it only with its owning product
migration and parity evidence.

Graph-v1 paths likewise remain supported migration paths, not extraction targets
to delete opportunistically. `main.rs` retains feature-gated graph-v2 terminal
validation and tool-receipt recovery alongside legacy completion/recovery
behavior. Preserve both until the selected product proves replacement parity;
the v2.4 design is not evidence that these paths are already replaced.
Issue #1234 owns deterministic frontend-neutral acquisition/release parity,
including same-session fencing/recovery, independent-session progress,
pre-Model failures, tool-only cancellation and integrated release-fault handling.
Neither generic native overlap nor generic generation tests close selected HTTP
isolation/retirement gaps. Larger missing mechanisms require separate dependent
follow-ups; bounded resolver cancellation remains #1230.

`acp-native-model-select-product.py` uses a separate, default-off fault artifact
to prove release failure after successful execution, failed execution and
cancellation, primary-error precedence, quarantine and subsequent admission denial.
An unrelated unselected session continues; selected A/B isolation is established
separately by the native fixture, not by that unselected session.
`acp-selected-session-ownership.py` holds an actual HTTP request while conflicting
same-session ownership is rejected before selection read/install and an independent
session progresses. HTTP receipt and explicit barriers establish ordering.

The native HTTP fixture installs distinct selected A/B configurations under real
generations for all three codecs. It checks session/credential/token isolation,
overlapping worker leases, A cancellation or completion while B stays pending,
zero leases and released roots before retirement. A default-off instrumentation
artifact independently records successful thread joining, configuration credential
drop and native state destruction. These events are not socket/lease proxies.
The selected HTTP fixture also uses a test-feature-only weak kernel probe for each
actual A/B generation. Both kernels remain alive after roots drop; A's final
strong reference disappears after retirement while B remains alive as the selected
default, and B's disappears after the generation manager drops. Session and
generation identities correlate these observations with the independent worker,
configuration and native-state events; native-state destruction is not treated as
kernel destruction. Weak probes retain only allocation metadata, not live kernels.
The HTTP instance still owns one `Option<Worker>`: independent instances overlap,
not concurrent roots in one instance. Concurrent roots remain separate kernel
lifecycle/ABI work, not another scheduler. Resolver bounds remain solely #1230.

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
