# Declarative plugin cleanup

Baseline: `00598cedf0a345934098d950881a702110001f83`, 24 October 2026. **Dated cleanup inventory; not current as-built documentation or the graph-v2 design.** See [spec index](README.md) and [v2.4 graph target](graph-architecture-v2.4.md).
The workspace contains 52 crates, including 28 native plugin crates.
This inventory records ownership and removal gates. It does not treat an
opt-in or dynamically loaded artifact as dead because the default excludes it.

## Minimal usage retained by this change

A frontend author implements the existing typed operations. The SDK already
generates empty state, ABI lifecycle, schema checking and handler dispatch:

```rust
#[provide(Frontend::decode)]
fn decode(host: &Host<'_>, request: ClientInput) -> CallResult<Submission, BasicError> {
    check_session(host, &request.session)?;
    selected_decode(request).map_err(CallError::Domain)
}
```

The three shipped frontend artifacts now share these handlers. Each feature
selects only its codec functions and plugin identity. A codec retains its native
input and output formats. The existing `Frontend` contract, selected import,
session check and host-owned graph cancellation remain necessary. No frontend
registration contract or new authoring macro is needed.

Evidence providers use the same existing `#[provide]` syntax. Their empty
`State` declarations and unused state parameters are removed. Their durable
ledger, membership checks and source validation stay in their owning operations.

The runtime's selected HTTP path is:

```rust
let install = HttpModelInstall::from_checked(&binding, session, credential, budget)?;
// The selected authorized HttpModelSession::install receives this value.
let config = Config::from_install(install)?;
```

`Config` is private transport implementation. The checked constructor proves
exact account and method matching. The receiving plugin validates decoded input
again because native Portable input can bypass that Rust constructor. This is
validation at two trust boundaries. The removed `Config::from_checked_binding`
was a third route with no production caller. Tests now exercise the actual
constructor and receiver, including live SSE for API keys, both OAuth grant
kinds and credential-free connections.

Adding a frontend codec does not change the agent or model. Adding a provider
uses the selected provider manager workstream. These changes add no contracts,
imports, grants or kernel behavior. They cannot create invocation authority.

## Native plugin inventory

Names below omit the `phenix-plugin-` prefix. Each crate has an explicit source
entry and native build in [flake.nix](../flake.nix). Test references name the
behavior that protects an opt-in artifact or constrains removal.

| Plugin | Classification | Retained owner or migration | Reachability and removal gate |
| --- | --- | --- | --- |
| advanced-model-adapter | Refactor | Model delegation; move preparation and journal orchestration to their existing owners | `p2-model-adapter-product.py`, `p2-metered-model-adapter-product.py` |
| advanced-preparer-graph | Keep | Selected preparation graph handoff | `p2-basic-staged-prepare-product.py`; selected graph composition |
| advanced-preparer-noop | Test-only | Unchanged candidate fixture | `basicNoopPreparerPlugin`, selected graph product |
| advanced-summarizer | Keep | Independent summary proposal algorithm | `p2SelectedModelSummarizer` check; explicit P2 opt-in |
| basic-agent | Refactor | One graph-driven turn and durable tool receipt owner | `phenix-b5-agent`, staged preparation, pending HTTP and restart products |
| basic-frontends | Consolidate | One stateless handler module; feature-selected Terminal, JSONL and ACP codecs | `frontendTerminal`, `frontendJsonl`, `frontendAcp`; `f1-frontends`, `f2-acp`, ACP v2 workflow |
| basic-memory | Refactor | Rust-owned session Skills and Context with typed CAS persistence | `phenix-b3-memory`; persistence helper migration |
| basic-session | Refactor | Rust-owned live session aggregate with durable recovery | `phenix-b2-sqlite`; restart and concurrent CAS checks |
| basic-tools | Refactor | Registration catalog and authorized selected dispatch | `phenix-b3-tools`; migrate implementations after typed registration parity |
| compaction-checkpoints | Keep | Durable checkpoint lineage and source ancestry | `p2-compaction-checkpoints-product.py` |
| environment | Keep | Private captured directory inputs and typed location resolution | `environment-product.py`; no filesystem effects |
| event-journal | Keep | Authoritative semantic events and observer checkpoints | `phenix-b2-journal`, observer and SQLite fault products |
| evidence-journal | Refactor | Stateless journal source verification using existing `provide` | `p2-source-evidence-product.py`; generated empty state replaces authored empty state |
| evidence-store | Refactor | Stateless operations over durable evidence ledger | `p2-evidence-store-product.py`, `p2-project-evidence-product.py`; preserve scope and source checks |
| model-history-test | Test-only | History-only model verifies memory without tool shortcuts | `basic-history-verification-product.py`; dedicated CI workflow |
| model-http | Refactor | Shared HTTP/SSE transport and validated selected configuration | `http-model.py`, native selected, pending and ACP products; preset migration gate below |
| model-meter-fixture | Test-only | Deterministic exact-token and Model fixture | `p2-model-meter-product.py`, metered adapter and graph products |
| model-scripted | Test-only | Deterministic reference Model used by demo and regression compositions | `b4-scripted`, default scripted Basic and history substitution products |
| project-storage-sqlite | Keep | Project records, membership and source observation in one SQLite snapshot | `p2-project-storage-product.py`, membership/source products; splitting checks would lose atomic admission |
| provider-builtins | Consolidate | Replace metadata-only failing auth stub with selected manager | `provider-builtins-product.py`; keep artifact until real catalog/auth migration passes |
| provider-chatgpt | Refactor | Declaration plus issuer-specific configuration; manager owns OAuth lifecycle | ChatGPT OAuth product, combined-auth product and secure restart tests |
| provider-credential-files | Keep | Explicit restricted durable secret backend | `provider-credential-store-product.py`; independent from account authentication |
| provider-loopback-fixture | Test-only | Explicit local account/model fixture | ACP native selection/restart products; exclude from default production closure |
| provider-openai-api | Refactor | Declaration plus API-key strategy settings | `acp-openai-api-key-product.py`; keep exact named-account isolation |
| provider-secret-service | Keep | OS Secret Service credential backend | `provider-secret-service-product.py`; independent optional backend |
| storage-fixture | Test-only | Deterministic in-memory Storage fixture | `phenix-b2-journal` and conformance compositions |
| storage-sqlite | Keep | Durable revisioned Basic records and transactions | SQLite recovery, schema upgrade and fault products |
| workspace-selection | Refactor | Rust-owned session selection with typed CAS persistence | `workspace-product.py`; helper migration must retain conflict invalidation |

## Supporting crate inventory

Names below omit the `phenix-` prefix. All 24 supporting crates have explicit
source references in `flake.nix`. Contract crates with a `main.rs` emit definition
packages; those binaries are intentional composition inputs.

| Crate | Classification | Responsibility and evidence |
| --- | --- | --- |
| advanced-contracts | Keep | Canonical P2 evidence, preparation, checkpoint and meter contracts; `advancedDefinitions` |
| artifact | Keep | Verified native descriptors and package identity; `phenix-describe`, kernel dependency |
| basic-contracts | Keep | Existing typed Basic operations and scopes; `basicDefinitions`; remove unused empty `src-placeholder.tmp` |
| basic-runtime | Refactor | Trusted graph/controller and ACP transport; remove provider multiplexing after manager migration |
| conformance | Test-only | Native contract/product runners used throughout Nix checks |
| contract | Keep | Canonical metadata, schema and wire types; public SDK and kernel dependency |
| environment-contracts | Keep | Typed location interface and pure platform resolution; `environmentDefinitions` |
| http-selected-product | Test-only | Native selected installation and real transport proof; `native-http-selected-product.py` |
| kernel | Keep | Verified generations, authority, graph scheduling, invocation and cancellation |
| macros | Refactor | Generate usage-driven startup and typed handler glue; retain existing M1 declaration semantics |
| p2-compaction-product | Test-only | Durable checkpoint product runner |
| p2-compaction-replay | Keep | Shared compaction replay and lineage validation used by real plugins |
| p2-evidence-store | Test-only | Session/project evidence product runner |
| p2-model-meter-product | Test-only | Exact token measurement product runner |
| p2-project-storage | Test-only | Project storage and membership product runner |
| p2-source-evidence | Test-only | Source verification and changed/withdrawn evidence product runner |
| provider-auth-strategies | Consolidate | Keep shared API-key mechanics; retire unused OAuthAttempts only with manager migration |
| provider-builtins | Refactor | Declarative provider values and real provider-specific settings; manager consumes registrations |
| provider-contracts | Keep | Existing catalog/auth/credential/model-selection operations; only narrow registration additions if required |
| provider-credential-files | Keep | Restricted atomic file operations shared by credential plugin |
| provider-discovery | Keep | Bounded reusable catalog decoding; API-key provider dependency |
| provider-protocols | Keep | Endpoint validation, request/SSE codecs and reusable verified OAuth implementation |
| sdk | Refactor | Generated lifecycle, selected invocation, owned publication and typed authoring helpers |
| workspace-contracts | Keep | Typed workspace selection and source identity; `workspaceDefinitions` |

## Removal decisions and remaining proof

| Item | Classification | Evidence | Required coordinated cleanup |
| --- | --- | --- | --- |
| Three frontend native wrapper modules | Consolidate | Same operation signatures and session admission; only codec and identity differ | Replace with one stateless module; keep artifact IDs, feature flags, Cargo and Nix outputs |
| Evidence plugins' empty State and unused handler parameters | Delete | No plugin-private fields or state-dependent operation behavior | SDK generates lifecycle state; no persistent records or schema change |
| `Config::from_checked_binding` | Delete | `rg` finds only its definition and unit tests; `#[allow(dead_code)]` masked absence of runtime callers | Remove method; move all its negative and live HTTP tests to checked native installation |
| Basic contracts `src-placeholder.tmp` | Delete | Empty, outside `src`, no references in code, Cargo, Nix, CI or docs | Remove tracked placeholder |
| ChatGPT direct `curl` dependency | Delete after provider migration | No Rust reference in concrete provider; verified flow transport lives in provider-protocols | Provider lane updates Cargo and lock dependency entry with its implementation move |
| `OAuthAttempts` | Investigate | References only in `provider-auth-strategies/src/oauth.rs` and its export; ChatGPT uses private `Pending` state | Provider lane must either use it or remove module, export and tests with replacement lifecycle proof |
| Metadata-only provider stub | Consolidate | Active builtins native package and product check; advertises no authenticated entitlement | Manager replaces handlers, then same package/checks exercise real registered ownership |
| Legacy HTTP environment preset | Refactor | `ModelConfiguration::Preset`, `Config::load`, `basicHttpComposition` and launcher are active | Port fixture configuration and all scenarios to selected installs, then remove preset branch, variables, Nix outputs, tests/docs references together |
| Basic default and staged loop branches | Consolidate | Both artifacts and products exist; separate `staged-graph` feature selects behavior | Agent lane proves journal/tool/cancel/restart parity, then removes feature/build variant and obsolete wiring |
| SQLite schema v0 migration | Keep | `phenix-b2-sqlite` proves existing durable records survive upgrade | This is supported data recovery, separate from retired M0 runtime formats |
| Frontend features | Keep | Native plugin IDs require separate Terminal/JSONL/ACP artifacts; ACP v2 workflow checks independent selection | Share implementation without merging incompatible descriptors |
| P2 and local fixture plugins | Test-only | Explicit Nix builds/products and opt-in compositions | Verify default closure excludes fixtures; retain their independent package outputs |

Legacy HTTP fixture migration must cover `tests/http-model.py`,
`tests/basic-http-preset-product.py`, `tests/basic-pending-http-product.py` and
`tests/acp-pending-http-product.py`. Preserve OpenAI Chat, OpenAI Responses and
Anthropic wire scenarios, max-output settings, CA fixtures, malformed-stream
failure, journal deltas and physical cancellation drain. The native selected
and ACP selected/restart tests already guard against environment fallback, but
they do not replace every old fixture scenario by themselves.

No unreferenced native Nix artifact was found at this baseline. Every plugin
crate appears in a source subset and build/check wiring. A source-subset match
alone is insufficient to prove production reachability; the tables distinguish
independent tests and opt-in products.

## Verification for this cleanup slice

- `cargo test --locked -p phenix-plugin-model-http`: 19 pass. The live SSE test
  exercises four selected auth modes through the real native install path.
- Frontend tests pass for Terminal and JSONL, 3 each; ACP v1, 5; ACP v2, 7.
  These are separate feature-build executions of overlapping codec assertions.
- Evidence journal and store unit tests retain scope, revision and provenance
  assertions after removing unused state parameters.
- Formatting and `git diff --check` must pass before this slice is committed.
- Nix acceptance and packaged native product checks remain integration gates.
  The local environment has no Nix executable.

This slice retains all 28 plugin crates. It consolidates three frontend handler
implementations into one, deletes one unused transport method and one empty
tracked placeholder, and removes two authored empty state declarations. It adds
and removes zero contracts. Provider and agent removals are recorded only when
the owning implementation and parity checks are complete.
