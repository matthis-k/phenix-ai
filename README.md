# Phenix AI

Phenix is a fully customizable coding agent built from plugins. Plugins define what the agent does; the M1 kernel defines how contributions execute. The generic runtime follows from that separation rather than being a separate product goal. [The product decision](spec/agent-product-decision.md) records the rationale, while the [M1 roadmap](spec/m1-roadmap.md) and [Advanced roadmap](spec/p2-advanced-agent-roadmap.md) own implementation requirements.

The native kernel has typed contracts, explicit provider selection and inherited authority. M1 is its only supported contract, value wire format and runtime profile. Native plugins use ABI 3. Definition-only packages carry the same canonical contract metadata without executable code.

The kernel verifies packages offline, prepares one immutable composition and dispatches selected native or cell operations through one authority boundary. Pending imports use the existing parent/root ticket tree. Cancellation revokes effects before cleanup, and physical leases retain executable images until work drains. Roots remain pinned to their admitted generation during promotion, rollback and retirement.

Rust plugins declare contracts and implement handlers. The SDK generates metadata, serialization, ABI and lifecycle glue. Stateless handlers need no authored State:

```rust
#[phenix_sdk::contract(id = "example.text@1")]
trait Text {
    fn uppercase(&self, text: String) -> Result<String, String>;
}

#[phenix_sdk::plugin(id = "example.uppercase", define(Text), provide(Text, slot = "text"))]
mod plugin {
    use super::*;
    #[provide(Text::uppercase)]
    fn uppercase(text: String) -> String {
        text.to_uppercase()
    }
}
```

Use `nix flake check -L` for the complete M1 conformance suite, `nix run .#phenix-m1-demo` for the packaged provider/consumer example and `nix build .#phenix-m1-release` for the evidence product. SDK, fixtures and inspection tools remain separate from the production kernel closure.

[The transition contract](spec/m1-only-runtime-contract.md),
[verified audit](spec/m1-consolidation-audit.md) and [roadmap](spec/m1-roadmap.md)
record scope and acceptance. M0 acceptance documents retain historical evidence.
Their authority, cancellation, ordering, generation and retirement guarantees now
have M1 tests. The transition passed exact-head CI at `367420e33b`.
[PR #781](https://github.com/matthis-k/phenix-ai/pull/781) completes S3b at
`75a255b9`, with full CI and verified release evidence.

## Opt-in Graph V2 packaged host

`nix run .#phenix-basic-v2-terminal` runs the existing Basic terminal host
with a canonically packaged Graph V2 `Source -> prepare -> model -> tool -> continuation -> finish` contribution. The
opt-in host prepares that verified contribution with an explicit trusted
source/operation security policy, then uses ordinary scoped action admission,
ABI-3 dispatch and journal rendering. The agent plugin opens or creates the
session on its first prompt; no preliminary session or authentication call is
required for this scripted-model preset. Use `--session NAME` to resume a
different session. The ordinary P1 launchers remain Graph V1 and unchanged.

`nix build .#phenix-basic-v2-product` runs the packaged executable against a
fresh SQLite database without a supplied session identity, reports the actual
host-established session, and checks first-prompt rendering, restart/observer
continuity, real packaged tool execution and tool failure, model failure/recovery, and denial
before native loading when an operation grant is absent. It does not rewrite
artifact metadata or use a test-only dispatcher or grants. No-tool turns pass
through the fixed stages without an extra Model invocation. The same gate is
registered as `checks.basic-v2-packaged-host`. This bounded increment is not
Graph V2 P1 parity: provider authentication, HTTP transport and in-flight
cancellation remain separately covered by the existing native gates.

## Durable session state

Basic, verification and Advanced presets persist their SQLite session state in
`$XDG_STATE_HOME/phenix/basic.sqlite`, falling back to
`$HOME/.local/state/phenix/basic.sqlite`. The runtime creates the parent
directory and exports the resolved path to its in-process storage plugins, so
packaged launchers and direct invocations work without extra configuration.
Set `PHENIX_BASIC_SQLITE_PATH` to override it; component-specific launchers such
as `phenix-basic-http-acp` select their own database file.

## Opt-in verified OpenAI API onboarding (experimental)

`phenix-basic-openai-api-acp` is a separately installed ACP launcher for
provider-owned authentication, current model discovery and selected-model HTTP
execution. It does **not** change the ordinary Basic or metadata-only provider
presets. This first opt-in composition authorizes only the named credential
`openai/api/personal`; other profiles require their own trusted composition.

The launcher uses the explicit-consent credential-file backend. Create an
existing private directory before starting it (the credential plugin rejects
unsafe ownership or permissions); do not put an API key in Nix expressions or
the environment:

```sh
mkdir -p "$HOME/.local/state/phenix/openai-api-credentials"
chmod 700 "$HOME/.local/state/phenix/openai-api-credentials"
export PHENIX_CREDENTIAL_FILE_DIRECTORY="$HOME/.local/state/phenix/openai-api-credentials"
export PHENIX_CREDENTIAL_FILE_CONSENT=1
nix run .#phenix-basic-openai-api-acp
```

In a Neovim frontend configured to launch that binary, run `:Phenix auth`
and enter the OpenAI API key for the **personal** profile. A saved key remains
unverified until the provider checks it against OpenAI's models endpoint.
Then run `:Phenix model` to select an offered, authorized Responses model
before prompting. Model selection is stored in the dedicated
`basic-openai-api.sqlite` session database; credentials are resolved at
execution instead of being persisted in the journal. Revocation invalidates
the credential-bound selected model.

This experimental preset explicitly disables legacy environment-key
fallback. The local file fallback requires operator consent and stores
credential material on disk with restrictive permissions; it is not encrypted
at rest. A properly configured OS Secret Service backend can be integrated
as an alternative when its provider composition is available. A successful
`/v1/models` response is authentication evidence, **not** proof of quota or
inference entitlement. ChatGPT subscription OAuth is not provided by this
OpenAI API-key connection.

## Combined P1 authentication (API key and ChatGPT OAuth)

`phenix-basic-p1-auth-acp` is the explicit combined ACP authentication
launcher used by the `phenix-nvim#nvim-nix-p1-auth` distribution. Unlike the
older API-only preset above, it **requires no shell credential setup**:

```sh
nix run .#phenix-basic-p1-auth-acp
```

The launcher creates `$XDG_STATE_HOME/phenix/p1-auth-credentials` (default
`~/.local/state/phenix/p1-auth-credentials`) with private 0700 permissions.
The plugin verifies owner, mode, and credential-file permissions before use.
Credentials are scoped by the kernel to the selected provider, connection,
and profile; OAuth token refresh still uses atomic compare-and-replace.
The file fallback is **not encrypted at rest**. Launching this opt-in
authentication product authorizes its private file store; an explicit
`PHENIX_CREDENTIAL_FILE_CONSENT=0` veto is always respected. No secrets
or storage paths are written to a Nix derivation or session journal.

## Opt-in P1 HTTP model

`phenix-basic-http-acp` runs the accepted Basic agent with the existing
`phenix.model.http` provider instead of the scripted model. The ordinary
`phenix-basic` package and its deterministic checks do not change. The HTTP
provider accepts OpenAI-compatible streaming chat-completions responses and
uses the same Basic tools and durable SQLite journal.

Configure the executable with environment variables in the shell that starts
the frontend. Keep secrets out of flake configuration and the Nix store:

```sh
export OPENAI_API_KEY="..."  # or PHENIX_MODEL_HTTP_API_KEY
export PHENIX_MODEL_HTTP_MODEL="gpt-4.1-mini"
# Optional, for another compatible provider:
# export PHENIX_MODEL_HTTP_ENDPOINT="https://provider.example/v1/chat/completions"
nix run .#phenix-basic-http-acp
```

`phenix-basic-http-acp` is the named ACP launcher for a Neovim client.
It defaults to the OpenAI chat-completions endpoint, chooses the model from
`PHENIX_MODEL_HTTP_MODEL`, and stores its P1 SQLite state under
`$XDG_STATE_HOME/phenix/basic-http.sqlite` unless
`PHENIX_BASIC_SQLITE_PATH` overrides it. The transport reads a credential
only at execution. `OPENAI_API_KEY` is an alias supported by the launcher.

The installed `phenix-basic-acp`, `phenix-basic-terminal` and
`phenix-basic-jsonl` launchers use
`$XDG_STATE_HOME/phenix/basic.sqlite` (or
`$HOME/.local/state/phenix/basic.sqlite`) by default. All installed
launchers create missing parent directories and honor an explicit
**absolute** `PHENIX_BASIC_SQLITE_PATH`. The Basic scripted-model and
HTTP-model defaults are intentionally separate databases. Neovim does not
choose a database path; the launched Phenix process does. A relative or
non-file SQLite path is rejected with a diagnostic instead of depending on
Neovim's current working directory.

For credential-bearing model connections the HTTP Model now requires
HTTPS for remote hosts. Explicit loopback HTTP with a literal
`127.0.0.1` or `[::1]` address remains available for local servers and
deterministic fixtures. Plaintext remote URLs and credentials embedded in
URLs are rejected before transport. This legacy preset still uses an
environment-supplied API key; interactive account selection and OAuth
remain tracked in [runtime management #891](https://github.com/matthis-k/phenix-ai/issues/891).

The `basic-http-product` check drives the installed Basic HTTP composition
against a local streaming HTTP fixture. That deterministic check has no
external network calls or paid model dependency. Live-provider behavior and
billing are external and must be tested explicitly.

P1 Basic is an accepted composed agent product. The Advanced agent remains under development through independently replaceable P2 plugins; neither agent is embedded in the kernel. The M1 and P2 roadmaps own current status and acceptance evidence.
