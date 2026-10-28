# Plugin responsibility audit and provider registry target

**Usage-first follow-up:** [Plugin author APIs and derived architecture](usage-first-plugin-architecture.md) — user workflows and usage sketches precede proposed contracts, ownership, routing and migration gates.

Audit baseline: `main` tree `00598cedf0a345934098d950881a702110001f83` (2026-10-24). **Historical snapshot, not the current plugin inventory.** Since then provider/tool registration contracts have landed (merged #1128/#1129), while shared manager/event population follow-up PRs remain open. For intended graph-v2 behavior use [v2.4](graph-architecture-v2.4.md); verify actual code on `main`. See [spec index](README.md).

**Scope:** 52 workspace crates, 28 `phenix-plugin-*` crates. The extra libraries, contracts, tools, and test/product crates are not automatically runtime plugins. Classification is by implemented contract and shipped/test composition, not by crate name alone.

## Responsibility inventory

| Boundary | Plugin crate | Actual responsibility | Assessment |
|---|---|---|---|
| Agent / execution | `basic-agent` | Graph-triggered agent-turn orchestration | Keep |
| Agent / execution | `basic-tools` | Tool catalog and tool execution | Keep |
| Agent / execution | `basic-session` | Session aggregate over Storage | Keep |
| Agent / execution | `basic-memory` | Session-scoped Skills and Context | Keep; not P2 cross-session memory |
| Presentation | `basic-frontends` | Terminal and JSONL adapters via separate feature builds | Keep; one crate, distinct artifacts |
| Model transport | `model-http` | HTTP Model, pending I/O, selected model configuration | Keep transport separate from provider registration |
| Model test | `model-scripted` | Deterministic request-driven Model | Keep as test/dev model |
| Model test | `model-history-test` | History-only Model verification | Keep test-only |
| Model test | `model-meter-fixture` | Deterministic token-meter/Model fixture | Keep test-only |
| Model / P2 | `advanced-model-adapter` | Selected Model interception/delegation | Keep independent from auth provider |
| Model / P2 | `advanced-preparer-graph` | Graph wiring for selected preparation | Keep |
| Model / P2 | `advanced-preparer-noop` | No-op preparation implementation | Keep opt-in verification implementation |
| Model / P2 | `advanced-summarizer` | Opt-in model-backed summary proposals | Keep |
| Provider catalog | `provider-builtins` | Metadata-only ProviderCatalog and failing/not-ready ProviderAuth operations | Transitional registry stub; not a real login |
| Provider concrete | `provider-openai-api` | API-key attempts, credential verification, model discovery, ProviderCatalog/Auth | Refactor into declaration + generic API-key strategy |
| Provider concrete | `provider-chatgpt` | OAuth attempt state, OIDC/PKCE flow, token persistence/refresh/revoke, model discovery, ProviderCatalog/Auth | Refactor into declaration + generic OAuth strategy + issuer-specific settings |
| Credential backend | `provider-credential-files` | Opt-in restricted file credential store | Keep; **not** a provider registration |
| Credential backend | `provider-secret-service` | OS Secret Service credential store | Keep; **not** a provider registration |
| Provider test | `provider-loopback-fixture` | Test-only local provider selection | Keep outside shipped production composition |
| Storage | `storage-sqlite` | Revisioned Basic Storage | Keep |
| Storage test | `storage-fixture` | In-memory Storage conformance fixture | Keep test-only |
| Storage | `project-storage-sqlite` | Project-scoped SQLite records | Keep separate from Basic session storage |
| Journal | `event-journal` | Authoritative Basic semantic event journal over Storage | Keep |
| Evidence / P2 | `evidence-journal` | Verify journal-backed source references | Keep |
| Evidence / P2 | `evidence-store` | Validated durable evidence over Storage | Keep |
| Compaction / P2 | `compaction-checkpoints` | Journal-anchored checkpoint metadata | Keep |
| Environment | `environment` | Environment/location resolution | Keep |
| Workspace | `workspace-selection` | Selected durable workspace | Keep |

## Stale versus overlapping

1. **Apparently unconsumed abstraction:** `phenix-provider-auth-strategies::OAuthAttempts` is defined and unit tested in `src/oauth.rs` but GitHub default-branch code search found no caller outside its own crate. Meanwhile `provider-chatgpt` holds its own `BTreeMap<String, Pending>`. Verify with `rg 'OAuthAttempts' crates` before deciding whether to integrate or remove it. Do **not** delete prior to migration; it may be the intended reusable core.
2. **Transitional implementation:** `provider-builtins` implements ProviderCatalog and ProviderAuth yet intentionally cannot authenticate or discover entitled models. It is used in `flake.nix` and `tests/provider-builtins-product.py`; it is **not dead**. Migrate to a real manager-owned registry before retiring its failure stubs.
3. **Responsibility duplication:** `provider-openai-api` and `provider-chatgpt` both implement full `ProviderCatalog`/`ProviderAuth` surfaces, inspect authorized credential roots, and manage authentication lifecycle. `phenix-basic-runtime/src/acp_provider_management.rs` also multiplexes multiple provider roots and catalogs. This is the priority consolidation.
4. **Naming mismatch, not stale:** `provider-secret-service` and `provider-credential-files` implement ProviderCredentials storage, not provider registrations. Preserve the backend boundary; a naming cleanup can follow consumers and package migration.
5. **Intentional verification variants:** `provider-loopback-fixture`, `model-history-test`, `model-meter-fixture`, `storage-fixture`, `advanced-preparer-noop`, and the scripted model have explicit tests/flake uses. No evidence of dead production code solely from their names.
6. **Absent from this branch:** `phenix-plugin-basic-skills` and `rust/crates/.../skills/write/SKILL.md` do not exist in the audited `main` tree; do not count them as current plugins.

## Target ownership and minimal registration

Keep the M1 kernel limited to graph scheduling, invocation, authority and cancellation. Keep the Basic agent turn loop in Basic Agent, and model HTTP wire codecs in Model/Protocol strategies. A **single selected provider-manager plugin** owns authoritative per-provider/per-connection/per-profile state and the orchestration of registration, auth, model discovery, and selection. The secure credential backend stores secrets durably; it must not become the authority on whether an account is authenticated.

Provider contributor plugins only declare immutable metadata at trusted startup and **do not implement** `ProviderAuth`, `ProviderCatalog`, credential persistence, HTTP transport, callback listeners, or model sessions:

```rust
// Illustrative author-facing registration shape, not an existing M1 ABI.
providers.register(ProviderRegistration {
    id: "openai",
    connections: [
        connection("api", "https://api.openai.com/v1",
            protocol("openai-responses"), auth("api-key")),
        connection("chatgpt", "https://api.openai.com/v1",
            protocol("openai-responses"), auth("chatgpt-oauth")),
    ],
});
```

`protocol` describes the inference protocol; `auth` describes the authentication strategy (API-key or OAuth), not the same dimension. The **already existing** `phenix-provider-builtins::ProviderDeclaration { endpoint, protocol, auth }` is the correct minimum schema and should be reused rather than replaced for its own sake. Optional typed strategy settings carry explicit model-catalog endpoint, wire header, OAuth issuer/authorization/token endpoints, scopes, client registration, and provider-specific verification rules.

### Contract routing

| Contract / behavior | New owner | Registrant responsibility |
|---|---|---|
| Startup registration | Provider manager plugin (state-backed) | Supply immutable, schema-checked declaration once |
| ProviderCatalog list/describe/models | Provider manager plugin | No implementation |
| ProviderAuth status/begin/poll/cancel | Provider manager plugin | Choose installed authentication strategy ID/settings |
| API-key input, env binding and verification | Reusable manager-owned API-key strategy | Describe key presentation and optional verification endpoint |
| OAuth PKCE/state/callback/OIDC/refresh/revoke | Reusable manager-owned OAuth strategy, library-backed | Describe issuer-specific endpoints, scopes and claims; no flow implementation |
| Credential persistence | Selected ProviderCredentials backend | No secrets or persistence |
| Response streaming / protocol codecs | Selected Model/Protocol implementation | Describe protocol and endpoint |
| Effective session model choice | Provider manager validation + existing session/model control | No session writes |
| Resource grants, admission, cancellation | Kernel | No grants implied by registration |

Authoritative mutable state belongs to the manager plugin; persistence is a durable checkpoint of that state and secret material, subject to revalidation on restart. Profiles and attempts are scoped by provider, connection, named account and authorized root. Registration grants no account capability or inference entitlement.

## Migration sequence (do not break live P1 auth)

1. **Consolidate catalog ownership:** add a state-backed provider manager/registry, ingest declarations at startup (bounded, deterministic, conflict-detecting), and expose one ProviderCatalog. Keep old implementations until products migrate.
2. **Move generic API key:** migrate `provider-openai-api` behavior into a reusable API-key adapter with environment-name and direct-key paths, credential-store interface, authorization and live model verification. Make OpenAI API a tiny declarative contributor.
3. **Move generic OAuth:** reuse or retire the currently unconsumed `OAuthAttempts` abstraction; centralize state/PKCE/loopback/poll/cancel/refresh/revoke with library-backed flows and typed issuer-specific policy. Make ChatGPT a tiny declarative contributor. Do not weaken issuer, audience, nonce, code/verifier, token, logout or account-bound CAS validation.
4. **Centralize model discovery and transport selection:** registry resolves declared catalog adapter, selected account, protocol and endpoint. Existing `model-http` owns request/stream transport; no provider plugin duplicates it.
5. **Collapse host multiplexer:** after all accounts use one registry, simplify the ACP provider-management root wiring and `provider_deployment` manifest, preserving existing named-account resource isolation.
6. **Delete only proved-obsolete code:** remove transitional ProviderAuth stubs and duplicated concrete implementations after closure/product tests confirm the new route. Rename credential-storage plugins only as a separate mechanical change.

## Acceptance gates

- Registration-only contributor has no auth/model/credential contract implementation, no token storage, and no network I/O.
- Multiple providers/connections use one manager implementation; duplicate provider+connection or conflicting strategy IDs fail closed.
- Registration may advertise methods but cannot claim authenticated status or model entitlement.
- API-key environment and securely entered API-key flows both survive restart; no credentials in journals, ACP, logs, Nix store, or declarative registrations.
- OAuth browser flow completes only after exact-account PKCE/OIDC/token validation and protected persistence; cancellation, expiry, refresh and revocation are account-scoped.
- No credentials or sessions cross account roots; model selection changes only subsequent turns; revoked/stale selections fail closed with no account fallback.
- Existing product tests continue: `tests/provider-builtins-product.py`, `tests/provider-credential-store-product.py`, `tests/provider-secret-service-product.py`, `tests/acp-provider-management-product.py`, plus API-key and browser OAuth product checks.
- Test-only fixture libraries are not linked into the default package closure; verify the actual `phenix-basic-p1-auth-acp` entrypoint and its managed providers.

## Verification of this audit

Inspected the 2026-10-24 `main` GitHub tree, source headers and `#[phenix_sdk::plugin]` surfaces for all 28 plugin crates, registration builder and strategies, the `flake.nix` package/check wiring, runtime provider deployment/ACP management, and GitHub code search for `OAuthAttempts`. This is a **static ownership audit**; no Nix build, dynamic loader test, or live OAuth test was run for this document. Treat apparent unused code as a candidate until local reachability and feature-gated consumers are checked.
