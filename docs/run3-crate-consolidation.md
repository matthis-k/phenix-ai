# Run 3 Crate Consolidation Inventory

**Status:** the preceding documentation reconciliation is committed at `9690f0d1e843ba6a9a77fe9d56f3842a90e3ddc1`, parent `9fac7b992b338021bf2ecd8a5c60d7514a18f018`. This increment relocates HTTP Model to `plugins/shared`, following the committed P2 consolidation, runner isolation/regression, Environment and Basic SQLite relocations. HTTP is locally verified; remote exact-head CI must follow its commit, whose SHA is not yet assigned here. Prior CI does not cover this increment. The work belongs to draft PR #1221, not an integrated release.

**Baseline:** `d9c4c9f62dcb94cc2d83679475d08b34fc9d50a9`, inspected 2026-10-31.

**Scope:** all root workspace packages, nested workspaces/standalone Rust fixtures, Cargo dependency boundaries, plugin contracts and product selections.

**Owner:** Run3 in `/tmp/opencode/phenix-ai-run3`, dependent on open PR #1220 at the baseline SHA. Historical local verification below predates the committed runner head; subsequent evidence is distinguished below. Run2 and the divergent main checkout were not modified.

**Precedence:** [contribution guidance](../CONTRIBUTING.md), [layout target](plugin-layout.md), [plugin rules](plugin-dos-and-donts.md), [spec index](../spec/README.md) and the existing M1/P2 roadmaps. This is an inventory and organization proposal, not a replacement graph or runtime specification.

## Evidence And Measurement

The baseline inventory and original local test narrative below are historical. Prior exact-head CI and measured invalidation are recorded separately from those earlier local sessions and this HTTP relocation increment.

At baseline, `Cargo.toml:1-3` selected only `crates/*`. This increment selects `crates/*`, `testing/products/*` and the explicit relocated Environment/SQLite/HTTP members under `plugins/shared`, not the nested example/experimental workspaces. The exhaustive inventory below describes baseline packages unless explicitly marked implemented. Runtime ports are evidenced by each plugin's `src/lib.rs` declaration. Definitions are not executable exports; Cargo dependencies are not runtime import bindings; Nix package availability is not product selection.

`maintenance.nix` and `.stitch.json` are absent. No substitutes are introduced. Repository guidance names `nix flake check -L`, `nix run .#phenix-m1-demo` and `nix build .#phenix-m1-release` as acceptance commands. Local sessions ran focused Nix gates, not the full acceptance/release commands; `devenv test` could not start because `devenv` was absent. Remote full generated acceptance passed at the preceding committed heads below, not yet for HTTP. This does not turn the unavailable local gate into a pass. Cargo was not on the default shell PATH in those historical sessions. Counts describe this HTTP increment and are source/manifest measurements, not transitive resolved edges or performance measurements.

| Measurement | Baseline | Proposed target | Implemented increment |
| --- | ---: | ---: | --- |
| Root workspace package manifests | 55 | 51 | 51 (47 in crates, one in testing/products, three in plugins/shared) |
| Foundational packages | 5 | 5 | 5 |
| Shared domain contract-set packages | 5 | 5 | 5 |
| `phenix-plugin-*` package manifests | 31 | 31 | 31 |
| Provider implementation helper libraries | 5 | 5 | 5 |
| Compaction replay library | 1 | 1 | 1 |
| Basic runtime host | 1 | 1 | 1 |
| Acceptance runner packages (conformance + HTTP + five P2) | 7 | 3 | 3, preserving all five P2 binaries |
| Direct internal Cargo dependency declarations, including optional/build edges | 148 | 133 for this increment | 133 |
| Members explicitly listed in `examples/Cargo.toml` | 36 | 36 unchanged | Not part of proposed reduction |
| Experimental graph/Python workspace members | 3 | 3 unchanged | Not part of proposed reduction |
| Other standalone packages (two Python-trivial, ACP SDK client) | 3 | 3 unchanged | Not part of proposed reduction |
| Distinct package manifests across these scopes | 97 | 93 | 93 |
| Default Basic selected native artifacts / definition packages | 10 / 1 | Unchanged | 10 / 1, declarations unchanged |
| Default Advanced selected native artifacts / definition packages | 15 / 2 | Unchanged | 15 / 2, declarations unchanged |

The dependency counts are declarations, not unique transitive edges: count matches of `^phenix-.*(path|workspace)` in root member manifests, including `testing/products/*/Cargo.toml` after migration. Consolidation replaces 20 runner internal declarations with their five-dependency union, reducing 148 to 133. Package-name manifests measure packages; plugin-prefixed names measure the unchanged 31 plugin packages. The cross-scope total changes from 55 + 36 + 3 + 3 = 97 to 51 + 36 + 3 + 3 = 93. No build-time, binary-size, dependency-resolution, cache-hit or startup improvement has been measured. The implemented reduction is four packages (7.3% of the root workspace), not fewer runtime identities or acceptance scenarios.

### Actual Migration And Verification

The five baseline `crates/phenix-p2-{model-meter-product,compaction-product,source-evidence,evidence-store,project-storage}` packages are replaced by unpublished `testing/products/phenix-p2-products`. Each old `src/main.rs` was byte-identical to its new `src/bin/<old-binary-name>.rs`, compared against the baseline commit's old path. No common orchestrator, compatibility package, CLI rename or scenario change is introduced. The new manifest has the dependency union; `Cargo.lock` removes five local package entries and adds one, without registry version changes.

`flake.nix` shares `p2ProductsSource` and selects each binary with `-p phenix-p2-products --bin <existing-name>`. Source-isolated builds and the exported public SDK strip the unavailable testing member glob from their copied root manifest; they do not acquire private kernel or test-product sources. Existing product scripts, check names, selections, identities, grants, CI discovery, generic conformance and HTTP runner remain unchanged.

On 2026-10-31, Linux x86_64, the dirty Run3 worktree based on `d9c4c9f62dcb94cc2d83679475d08b34fc9d50a9` passed:

```sh
nix build --no-link -L \
  path:.#checks.x86_64-linux.p2-model-token-meter-product \
  path:.#checks.x86_64-linux.p2-compaction-checkpoints-product \
  path:.#checks.x86_64-linux.p2-source-evidence-product \
  path:.#checks.x86_64-linux.p2-evidence-store-product \
  path:.#checks.x86_64-linux.p2-project-storage-product \
  path:.#checks.x86_64-linux.p2-project-membership-product \
  path:.#checks.x86_64-linux.p2-project-source-product \
  path:.#checks.x86_64-linux.p2-project-evidence-product \
  path:.#checks.x86_64-linux.m1-public-sdk
git diff --check
```

The first Git-backed `.#` attempt failed because untracked new package files are excluded from Git flake sources. `path:.` included that uncommitted tree without staging. Two build attempts hit tool time limits; retries completed successfully using completed store outputs. All five release runner binaries built offline through Nix with lock consistency validation, and all eight P2 gates plus the isolated public SDK gate passed. These gates retain real kernel/native SQLite, restart, stale CAS, scope denial, source freshness and live membership revocation scenarios. At that local stage, full `nix flake check`, demo, release, broad ACP/observer/auth parity and external-plugin acceptance had not been run; those focused results alone did not establish full release acceptance. Subsequent remote generated acceptance is recorded below.

Historical local follow-up verification on 2026-10-31 confirmed that `contractSource` in `flake.nix` explicitly includes `./testing/products/phenix-p2-products`. This source-closure fix preserves the new workspace member for the workspace-wide contract check; unlike isolated SDK consumers, that check must not strip the testing member glob. The fix was already present when that follow-up began; no further code change was needed.

```sh
nix eval --impure --raw --expr builtins.currentSystem
# x86_64-linux
nix build path:.#checks.x86_64-linux.m1-contract --no-link -L
# Passed on retry after the first attempt exceeded the 200-second tool timeout.
devenv test
# Could not start: devenv: command not found.
```

The successful contract derivation checked the workspace including `phenix-p2-products` and completed workspace tests and doctests. This was additional focused verification of the then-dirty tree, not full flake/release acceptance. The workspace-required `devenv test` was attempted but did not run any tests because the executable was absent from PATH; it was not a passing gate. `maintenance.nix` and `.stitch.json` were absent. That local verification session created no commit; the consolidation was subsequently committed at `a014a07d1822b39cc7e35ed791c3a3e104663121`.

Rationale: these executables share the kernel/Advanced acceptance layer but own no production plugin state or loading identity. Grouping them removes duplicated package/dependency declarations without merging production lifecycles. The original cost was broader invalidation: all five derivations consumed the same source closure and dependency union, while still compiling separately per binary. The later isolation fix narrows each runner's source closure; the shared dependency union remains. Nix build logs confirm five distinct runner derivations, not one shared compilation. The consolidation's root-manifest/lock update also invalidated isolated consumer builds. No baseline-versus-target timing/cache benchmark was performed; do not market this as a performance optimization. Environment and SQLite physical moves subsequently committed; other moves and production merges remain deferred.

### Exact-Head Closure And Rebuild Measurement

On 2026-10-31, all reported checks for PR #1221 at `a014a07d1822b39cc7e35ed791c3a3e104663121` completed successfully, except the intentionally skipped legacy candidate `flake-checks`. No CI failure required a shipping-code fix. The [generated four-builder run](https://github.com/matthis-k/phenix-ai/actions/runs/41157912213) independently reports that exact `headSha`: evaluation, all four builders, DAG scheduler, both discovered runnable package tests and final acceptance passed. Final acceptance verified check-output commit identity and shipping evidence. Formatting, deterministic history, cache isolation, official ACP SDK, typed v1/v2 and pinned workflow generation also passed. This resolved the pending CI/full acceptance at that historical stage for that committed increment; later increments have separate evidence below. #1220 remains open; neither PR was merged.

Rebuild measurement used archived snapshots of the baseline and committed Run3 head in temporary directories outside the checkout. Each snapshot exposed the five existing runner derivations and kernel control through temporary flake outputs. `nix eval --impure --json` recorded their `drvPath` before and after adding one blank line to only the source-evidence runner. No runtime code or dependencies changed.

| Source-only perturbation | Baseline | Committed Run3 |
| --- | ---: | ---: |
| Runner derivations invalidated | 1/5 | 5/5 |
| Unrelated runner derivations invalidated | 0 | 4 |
| Kernel control invalidated | No | No |

Thus the original consolidation measurably increased unnecessary runner rebuild invalidation. The [runner isolation follow-up](run3-runner-isolation.md) fixes this: independent whitespace edits to every runner each invalidate only its own product, with all four unrelated products and kernel/Environment/SDK controls unchanged. It still builds five separate derivations; fewer Cargo packages do not imply shared compilation. No duration, substitution/cache-hit rate or binary-size benefit was measured. Counts from parsed manifests confirm 55/148 packages/internal dependency declarations before and 51/133 afterward. The runner fix's [exact-head generated CI](https://github.com/matthis-k/phenix-ai/actions/runs/41167127142) passed evaluation, all four builders, both runnable packages and final shipping-evidence verification.

### Environment Relocation Follow-Up

The committed Environment increment `d7412fba7e862a8a0f470a5c566aa3bede2d0d67` moved only `crates/phenix-plugin-environment` to `plugins/shared/phenix-plugin-environment`. Its implementation source was byte-identical to its parent. Its two Cargo path dependencies, explicit workspace membership, Nix plugin closure, workspace closure and isolated-manifest pruning changed together. `Cargo.lock` remained unchanged and Nix lock validation passed. Counts remain 51 packages, 133 internal dependency declarations and 31 plugin packages.

This low-coupling plugin has no runtime imports. Preserve its independent native artifact, `phenix.environment.default` identity, `phenix.environment@1` contract and Environment export; no state or authority is transferred. The selected Environment product continues using the same supplied artifact and definitions. Basic/Advanced selections and CI's flake-check discovery require no edits because neither refers to its source directory; Environment is not silently added to those products. No ACP observer integration changes.

On Linux x86_64, the Environment follow-up was reverified after the runner isolation commit and passed:

```sh
nix build --no-link -L \
  path:.#checks.x86_64-linux.environment-product \
  path:.#checks.x86_64-linux.m1-contract \
  path:.#checks.x86_64-linux.m1-public-sdk
git diff --check
```

The selected product retains wrong-resource, traversal and missing-grant rejection, XDG/workspace resolution and no-filesystem-effects assertions. Workspace tests/Clippy and public SDK isolation passed. Comparison with the preceding runner-isolation commit's old source confirmed byte identity, including plugin identity and contract declarations. `devenv test` could not start because `devenv` was absent. Remote exact-head verification was pending at that local stage and subsequently passed as recorded below. SQLite Storage was then the next candidate and has since been committed and verified separately.

Environment subsequently passed [generated exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41167975166)
at `d7412fba7e862a8a0f470a5c566aa3bede2d0d67`, including all builders,
both runnable packages and independent final shipping-evidence verification.

### SQLite Relocation Follow-Up

Commit `6a5d3424930e685a119693c377f5f8731a4b2b3f` moved only Basic Storage from `crates/phenix-plugin-storage-sqlite` to
`plugins/shared/phenix-plugin-storage-sqlite`. The implementation source was
byte-identical to the preceding Environment commit's old source. This preserves `phenix.basic.storage-sqlite`, its Storage
export, `phenix.basic.storage@1`, no runtime imports, schema versions, transactions,
CAS and recovery logic. The independent native artifact and `rusqlite = "0.37.0"`
plus Nix SQLite/pkg-config dependency boundary remain unchanged. No contract or
SDK gains an implementation/native dependency. Project SQLite is not moved.
Cargo membership, the two relative paths, Nix closure and isolated-manifest
pruning change together; the lockfile remains unchanged.

On Linux x86_64 the relocated worktree passed:

```sh
nix build --no-link --max-jobs 2 \
  path:.#checks.x86_64-linux.b2-sqlite \
  path:.#checks.x86_64-linux.basic-host-smoke \
  path:.#checks.x86_64-linux.b5-agent \
  path:.#checks.x86_64-linux.m1-public-sdk \
  path:.#checks.x86_64-linux.m1-contract
git diff --check
```

The first combined attempt timed out; its retry completed successfully. The
SQLite gate covers independent-process restart/recovery, schema migration and
rejection, failed transaction atomicity, durable persistence, cross-process CAS,
wrong-resource and missing grants. Basic selected behavior and public SDK
isolation passed without editing any runtime, ACP or graph execution code.

Derivation-hash measurements around both physical relocations found broad
one-time invalidation, not isolation: all five runner product derivations and
kernel, Environment product and public SDK controls changed. The copied root
manifest and pruning derivations change even where the final isolated manifest
normalizes away the relocated member. These moves are organizational, not cache
optimizations; runner-source perturbation isolation remains a separate result.
Counts remain 51 workspace packages, 133 internal dependency declarations and
31 plugin packages. The local `devenv test` attempt was blocked by the missing executable;
`maintenance.nix` and `.stitch.json` remain absent. Remote exact-head verification
subsequently passed in [run 41170082526](https://github.com/matthis-k/phenix-ai/actions/runs/41170082526)
at `6a5d3424930e685a119693c377f5f8731a4b2b3f`: evaluation, all four builders,
both runnable tests and independent final shipping-evidence acceptance succeeded.
HTTP Model follows as the separately audited increment below; no production crate merge is implied.

`nix flake check --no-build path:.` passed evaluation of every exported check and
package for this increment. This is evaluation only, not a local full build gate.

### Historical Automated Runner Regression

Commit `9fac7b992b338021bf2ecd8a5c60d7514a18f018` adds the automated regression described in [runner isolation](run3-runner-isolation.md), without another relocation. Its [generated exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41194107549) passed evaluation, all four builders, the DAG scheduler, both runnable test packages and independent final acceptance. On 2026-10-31 UTC, final acceptance completed at `10:50:36Z` after independently checking output commit identity, shipping evidence and executable-test results. The [existing cache-isolation workflow](https://github.com/matthis-k/phenix-ai/actions/runs/41194107132) also passed at this exact SHA; its mandatory five-runner regression step succeeded at `10:47:17Z` and its job completed at `10:50:41Z`. Mandatory here means an enforced workflow step, not a claim that GitHub branch-protection settings changed. Neither run covers HTTP relocation.

### Documentation Reconciliation

The docs-only reconciliation `9690f0d1e843ba6a9a77fe9d56f3842a90e3ddc1` passed [generated CI 41197840798](https://github.com/matthis-k/phenix-ai/actions/runs/41197840798): 89/89 checks, all four builders, both executable tests and final acceptance at that exact head. No sampled derivation changed versus `9fac7b9`. This is prior documentation-only evidence, not HTTP acceptance or new runner-regression evidence.

### HTTP Relocation Increment

This increment moves `crates/phenix-plugin-model-http` to `plugins/shared/phenix-plugin-model-http`. Cargo membership and four relative internal dependency paths, Nix source closures and isolated-manifest pruning change together. `Cargo.lock` is unchanged. All five Rust files (`control_tests.rs`, `lib.rs`, `request.rs`, `tests.rs`, `transport.rs`), totaling 56,763 bytes, are byte-identical to the preceding committed source.

The dependency/reverse-dependency audit found direct internal dependencies on SDK, Basic contracts, Provider contracts and provider-protocols with `model-codecs`, and no Cargo dependents on the HTTP implementation. External dependencies remain `zeroize`, `serde_json` and `curl` with defaults disabled; Nix preserves the native curl/zlib/pkg-config boundary. Protocols remain a separate lower-level helper, including shared request/response/stream codecs; neither contracts nor public SDK acquire the HTTP implementation or native transport closure.

Preserve identity `phenix.model.http`, version `1.0.0`, state profile, exports `Model:Model` and `ModelControl:HttpModelSession`, required `Journal:EventJournal` and optional `Storage:Storage`. Private checked installation remains scoped to the exact Basic session/resource, with storage CAS and denial checks, one-use checked configuration and no fallback to a legacy preset after selected mode is entered. The HTTP plugin owns its private configuration and worker lifecycle; worker I/O remains separate from authenticated journal effects, with cancellation/pending behavior unchanged. Its independently loadable native artifact, descriptor/digest packaging, product selections, wiring and grants are unchanged. This is relocation, not a provider/protocol merge or behavior refactor.

Existing review caveats remain open: transport lacks explicit resolved-address policy validation despite the shared protocol documentation assigning that responsibility to transport, and transport contains a literal `anthropic-version` header where codec responsibilities warrant review. Neither is a new violation caused by this byte-identical move. Defer behavior refactoring to the smallest separately reviewed fixes; do not describe relocation gates as closing these gaps.

On Linux x86_64, local verification passed all seven focused gates:

```sh
nix build --no-link -L \
  path:.#checks.x86_64-linux.b4-http \
  path:.#checks.x86_64-linux.basic-streaming \
  path:.#checks.x86_64-linux.basic-http-product \
  path:.#checks.x86_64-linux.p2-model-adapter-pending-http \
  path:.#checks.x86_64-linux.p2-preset-overrides \
  path:.#checks.x86_64-linux.m1-contract \
  path:.#checks.x86_64-linux.m1-public-sdk
nix flake check --no-build path:.
```

The contract gate includes formatting, Clippy and tests. Full flake `--no-build` passed evaluation only, not a full build. `devenv test` remains unavailable because the executable is absent. Remote exact-head generated CI and final acceptance must follow the HTTP commit; no HTTP commit SHA or remote pass is claimed here.

Relocation changed all 15 sampled derivations: five P2 runners, kernel/Environment-product/public-SDK controls, HTTP itself and six unrelated packages (`phenix-basic-runtime`, `phenix-describe`, `phenix-plugin-environment`, `phenix-plugin-model-scripted`, `phenix-plugin-provider-builtins`, `phenix-plugin-storage-sqlite`). HTTP's own path invalidation is expected. The broad unrelated invalidation is undesirable: copied root-manifest/pruning derivation inputs changed even for consumers whose final isolated manifest removes HTTP. Future isolated-source work should eliminate that unrelated manifest/pruning coupling, separately from this move.

A subsequent HTTP-only whitespace perturbation changed only HTTP; all 14 other sampled derivations remained unchanged (5 runners, 3 controls, 6 unrelated packages). HTTP's derivation moved from `/nix/store/7jjxxbvcqvr5f5ziwns19dm1n699ycq1-phenix-plugin-model-http-0.1.0.drv` before relocation to `/nix/store/v35fa7rimfzj9hz1plgs7x3bds9j4y1i-phenix-plugin-model-http-0.1.0.drv` after relocation, then `/nix/store/fi6xq8pim51fxakb1jnpf9vcpcav4r6p-phenix-plugin-model-http-0.1.0.drv` under perturbation. Evidence: `/tmp/opencode/http-relocation-before.json` and `/tmp/opencode/http-relocation-identity-isolation.log`; these are local diagnostics, not committed artifacts. No timing, cache-hit, binary-size or other performance improvement is claimed.

Final counts remain 51 packages (47 `crates`, one `testing/products`, three `plugins/shared`), 133 internal dependency declarations and 31 plugin packages. Next candidate: provider credential-files backend/plugin, after a restricted native-boundary audit; not a protocols merge.

## Before And Target Trees

Brace groups expand to individual directories. The exhaustive tables below resolve every member of each group; this tree is not a claim that destination crates exist.

```text
BEFORE
Cargo.toml                         members = ["crates/*"]
crates/                            55 packages, all flat
  phenix-{contract,kernel,sdk,macros,artifact}
  phenix-{basic,advanced,provider,environment,workspace}-contracts
  phenix-provider-{builtins,auth-strategies,discovery,protocols,credential-files}
  phenix-basic-runtime
  phenix-p2-compaction-replay
  phenix-plugin-*                   31 packages (enumerated below)
  phenix-conformance
  phenix-http-selected-product
  phenix-p2-{model-meter-product,compaction-product,source-evidence,
             evidence-store,project-storage}
mixes/{production,testing}/         actual selection helpers
plugins/ products/ testing/         documented destinations, not migrated crates
examples/                          separate 36-member workspace
examples/python-trivial/{runner,rust-equivalent}/  standalone packages
experiments/graph-v2-python/        separate 3-member workspace
tests/acp-sdk-client/               standalone ACP client
```

```text
PROPOSED TARGET (not implemented)
crates/                            10 foundational/shared-contract packages
  phenix-{contract,kernel,sdk,macros,artifact}
  phenix-{basic,advanced,provider,environment,workspace}-contracts
plugins/shared/                    13 retained packages
  phenix-provider-{builtins,auth-strategies,discovery,protocols,credential-files}
  phenix-plugin-{model-http,storage-sqlite,environment}
  phenix-plugin-provider-{builtins,openai-api,chatgpt,credential-files,secret-service}
plugins/basic/                     8 retained packages
  phenix-basic-runtime
  phenix-plugin-basic-{agent,tools,memory,session,frontends}
  phenix-plugin-{event-journal,tool-receipts}
plugins/advanced/                  10 retained packages
  phenix-p2-compaction-replay
  phenix-plugin-advanced-{model-adapter,preparer-graph,preparer-noop,summarizer}
  phenix-plugin-{compaction-checkpoints,evidence-journal,evidence-store,
                 project-storage-sqlite,workspace-selection}
testing/fixtures/                  7 retained packages
  phenix-plugin-model-{scripted,history-test,meter-fixture}
  phenix-plugin-{provider-loopback-fixture,tool-fixture,tool-pending,storage-fixture}
testing/testkit/phenix-conformance/  retained package, existing binaries
testing/products/
  phenix-http-selected-product/     retained isolated native HTTP gate
  phenix-p2-products/                implemented 5-to-1 multi-binary package
products/                           eventual thin declarations, no new runtime crate
mixes/                              preserve current declarations until migrated together
examples/ experiments/ tests/       unchanged nested/standalone package boundaries
```

The eventual workspace member patterns must explicitly include the destination directories. Merely moving folders while retaining `members = ["crates/*"]` loses members. The target tree describes physical organization independently of shipping selection. In particular, moving scripted under fixtures cannot silently remove it from the currently selected offline Basic/Advanced products.

## Complete Package Mapping

For this baseline mapping, package names are abbreviated by removing only the `phenix-` prefix. Baseline path is always `crates/phenix-<name>`. Target directory prefixes are `C = crates`, `S = plugins/shared`, `B = plugins/basic`, `A = plugins/advanced`, `F = testing/fixtures`, `T = testing/testkit`, `P = testing/products`. A target such as `B/same` means `plugins/basic/phenix-<name>` with the existing Cargo package name. The five-runner consolidation and Environment/Basic SQLite relocations are **committed**; HTTP is **implemented in this increment, locally verified, awaiting committed exact-head CI**. Other moves remain **proposed**. Unchanged foundational paths are retained, not pending moves.

Dependency shorthand: `c=contract`, `s=sdk`, `m=macros`, `k=kernel`, `a=artifact`, `b=basic-contracts`, `v=advanced-contracts`, `p=provider-contracts`, `e=environment-contracts`, `w=workspace-contracts`, `r=p2-compaction-replay`, `pb=provider-builtins`, `pa=provider-auth-strategies`, `pd=provider-discovery`, `pp=provider-protocols`, `pc=provider-credential-files`. Each abbreviation means the full `phenix-` package. `?` is an optional Cargo dependency, `build:` is a separate build dependency, `dev:` is a dev dependency. Internal dependency cells are exhaustive direct declarations; external dependencies follow separately.

### Foundations And Contracts

| Current package | Direct internal dependencies | Target | Reason for retaining this crate |
| --- | --- | --- | --- |
| contract | None | C/same | Canonical ABI/schema/metadata/authority value model, usable without loader or SDK macros. |
| kernel | c, a | C/same | Private execution, loading, authority, pending/generation and graph mechanisms must not enter public SDK/plugin closure. |
| sdk | c, m | C/same | Public plugin-author API; external-source fixtures build without private kernel. |
| macros | c | C/same | Rust proc-macro compilation boundary; cannot simply be a runtime SDK module. |
| artifact | c | C/same | Artifact inspection/verification and descriptor CLI independently consumed by kernel and packaging. |
| basic-contracts | s | C/same | One shared Basic interface set for production and fixture implementations; not implementation-owned types. |
| advanced-contracts | b, s | C/same | Additional evidence/project/preparation/compaction interfaces without forcing all Basic consumers to depend on Advanced. |
| provider-contracts | s, b, pp | C/same | Shared provider catalog/auth/credentials/runtime/model-control schemas; current protocol-type dependency is explicit, not hidden. |
| environment-contracts | s | C/same | Environment is independently packaged/tested and imported by the host; no Basic dependency in its manifest. |
| workspace-contracts | s | C/same | Standalone portable workspace API, independently packaged/tested; moving it into Basic would impose a new dependency on independent clients. |

No interface-per-crate split is proposed. Keep domain modules inside these sets. The current provider-contracts -> provider-protocols edge means protocols cannot be folded into a provider plugin or a library depending back on provider-contracts without breaking the layering/cycling the graph.

### Reusable Libraries And Host

| Current package | Direct internal dependencies | Target | Reason for retaining this crate |
| --- | --- | --- | --- |
| provider-builtins | p, pp | S/same | Registration/catalog data shared by three independently loaded provider plugins; source explicitly has no network I/O. Keep this restricted closure. |
| provider-auth-strategies | p, pp? | S/same | Shared attempt lifecycle; OAuth browser is feature-gated. Do not couple metadata-only registrations to network/auth implementation dependencies. |
| provider-discovery | pp | S/same | Bounded parsing/cache/HTTP discovery shared by real providers, independently tested; no catalog/auth runtime authority. |
| provider-protocols | s?, b? | S/same | Shared low-level endpoint/auth/model codecs; provider contracts also consume it, so it must remain below implementation helpers. |
| provider-credential-files | None | S/same | Small protected-files backend with only libc/zeroize dependencies, separately tested without SDK/contracts. Separate backend from plugin authorization wrapper. |
| p2-compaction-replay | v, b, s | A/same | Shared pure verification, history/budget/admission/view algorithms consumed by adapter, checkpoints, summarizer and acceptance runner. Not a second journal or executor. |
| basic-runtime | b, e, p, c, k, s | B/same | Product host and ACP connection/protocol boundary, not agent state owner. Explicitly untouched in this consolidation. |

### Basic And Shared Native Plugins

| Current package | Direct internal dependencies | Target | Reason for retaining this crate |
| --- | --- | --- | --- |
| plugin-basic-agent | s, b; build: b, c | B/same | Independent graph/loading identity and graph-v1/staged/v2 variants; merging requires public-port parity, not a folder edit. |
| plugin-basic-tools | s, b; build: c, b | B/same | Replaceable tool catalog/execution/contributor owner and graph variant; retains contributor/receipt authority boundaries. |
| plugin-basic-memory | s, b | B/same | Owns per-instance skills/context with durable restore; not agent fields or frontend settings. |
| plugin-basic-session | s, b | B/same | Sole live session/recovery owner; preserve independent lifecycle and snapshot reconciliation. |
| plugin-event-journal | s, b | B/same | Durable semantic journal/observer receipt behavior, not diagnostic logging; preserve required storage effects and recovery. |
| plugin-tool-receipts | s, b | B/same | Independent receipt/recovery capability and selected graph artifact; authority/session interactions need their own acceptance. |
| plugin-basic-frontends | s, b | B/same | One source crate intentionally builds mutually exclusive terminal/JSONL/ACP artifacts. ACP and observer semantics remain untouched. |
| plugin-model-http | s, b, p, pp | S/same (this increment) | Replaceable network Model and private model-control state with restricted credential/transport semantics; not an agent dependency. |
| plugin-storage-sqlite | s, b | S/same (committed) | Native SQLite dependency and independently replaceable durable Storage implementation, schema/CAS/restart boundary. |
| plugin-environment | s, e | S/same (committed) | Independently loaded Environment provider with no Basic/plugin implementation dependency. |
| plugin-provider-builtins | s, p, pb | S/same | Loaded catalog/registration aggregator identity distinct from reusable builtin data and real auth providers. |
| plugin-provider-openai-api | s, p, pb, pa, pd | S/same | Selected API-key provider/account authority and auth lifecycle; separately packaged production and loopback-test variants. |
| plugin-provider-chatgpt | s, p, pb, pa, pd, pp | S/same | OAuth/OIDC state, browser flow, bearer handoff and exact account grants are distinct from API-key auth. |
| plugin-provider-credential-files | s, p, pc | S/same | Authorized plugin wrapper around restricted file backend; stable exact-account Credentials export. |
| plugin-provider-secret-service | s, p | S/same | Alternative OS keyring/DBus credential implementation; must not force OS-service dependencies into file backend or SDK. |

### Advanced Plugins

| Current package | Direct internal dependencies | Target | Reason for retaining this crate |
| --- | --- | --- | --- |
| plugin-advanced-model-adapter | s, b, v, r | A/same | Independently selected Model decorator with optional meter/transcript/checkpoint/summarizer bindings; not a passive observer. |
| plugin-advanced-preparer-graph | s, b, v | A/same | Staged graph contribution binds agent-owned property and Advance port; graph/public-port blocker applies. |
| plugin-advanced-preparer-noop | s, b, v | A/same | Independently replaceable selected preparation strategy used by staged tests, not a branch inside graph owner. |
| plugin-advanced-summarizer | s, b, v, r | A/same | Opt-in selected upstream Model effect with intentionally no tools/checkpoint/journal authority; absent from public Advanced preset. |
| plugin-compaction-checkpoints | s, b, v, r | A/same | Durable checkpoint validation/store lifecycle through authorized journal and storage; not summarizer-owned state. |
| plugin-evidence-journal | s, v, b | A/same | Replaceable SourceEvidence adapter over authorized journal, no duplicated source-byte store. |
| plugin-evidence-store | s, v, b | A/same | Evidence ledger/version/freshness lifecycle with separate session/project bindings; cannot collapse into source adapter without changing ownership. |
| plugin-project-storage-sqlite | s, v | A/same | Native SQLite project storage, membership and project-source authority distinct from session Storage. |
| plugin-workspace-selection | s, b, w | A/same | Owns selected workspace live state and durable restore; standalone product gate, not implicitly selected by Advanced. |

### Fixtures And Acceptance Packages

| Current package | Direct internal dependencies | Target | Retention or consolidation reason |
| --- | --- | --- | --- |
| plugin-model-scripted | s, b | F/same | Retain independently selectable deterministic Model and controlled-pending variant. Currently ships in offline Basic/Advanced; relocation is not removal from products. |
| plugin-model-history-test | s, b | F/same | Retain isolated history-verification Model; production guard explicitly rejects this artifact name. |
| plugin-model-meter-fixture | v, b, s | F/same | Retain explicit synthetic token Model/Meter/Summarizer artifact; never describe byte-token fixture as production estimator. |
| plugin-provider-loopback-fixture | s, p | F/same | Retain isolated executable provider catalog/auth/runtime artifact; two credential-free models, prohibited in default shipping selections. |
| plugin-tool-fixture | s, b | F/same | Retain external ToolRegistrations contributor for native plugin replacement/authority tests. |
| plugin-tool-pending | s, b | F/same | Retain independently loaded pending-a/pending-b artifacts and physical worker cancellation/fanout tests; do not combine runtime instances. |
| plugin-storage-fixture | s, b | F/same | Retain replaceable in-memory Storage test artifact without SQLite or production test-mode branching. |
| conformance | c, a, k, b | T/same | Retain generic ABI/kernel/SDK acceptance executables isolated from provider/P2 changes; existing many-binary package demonstrates test grouping. |
| http-selected-product | c, k, b, p | P/same | Retain native private Model installation/HTTP gate. `flake.nix:1304-1313` explicitly isolates this from generic conformance to avoid rebuild coupling. |
| p2-model-meter-product | v, b, c, k | P/phenix-p2-products, `src/bin/phenix-p2-model-meter-product.rs` | Committed merge: package is an acceptance executable, not a plugin/state owner; binary name and scenarios retained. |
| p2-compaction-product | c, k, b, v, r | P/phenix-p2-products, `src/bin/phenix-p2-compaction-product.rs` | Committed merge: same kernel/Advanced acceptance layer; separate process/restart invocations preserved. |
| p2-source-evidence | c, k, b, v | P/phenix-p2-products, `src/bin/phenix-p2-source-evidence.rs` | Committed merge: native source-verifier test executable, not production source ownership. |
| p2-evidence-store | c, k, b, v | P/phenix-p2-products, `src/bin/phenix-p2-evidence-store.rs` | Committed merge: native ledger acceptance executable; journal seed kept in separate process. |
| p2-project-storage | c, k, v | P/phenix-p2-products, `src/bin/phenix-p2-project-storage.rs` | Committed merge: project isolation/revision test executable; not the SQLite plugin. |

The implemented `phenix-p2-products` package is a separate **test-only build boundary**, not merged into generic conformance or any production plugin. Its normal dependency union is c, k, b, v, r, serde_json, sha2. Each binary keeps its existing entry point/CLI/output evidence contract; Nix uses `-p phenix-p2-products --bin <existing-name>`. There is no common workflow interpreter, shared orchestrator or compatibility crate at the five old paths. The original shared-source invalidation regression was corrected by per-runner source filtering; shared dependency/manifest changes can still invalidate all five derivations. No performance gain is claimed. Environment and Basic SQLite relocations are committed; remaining target relocations are proposed.

### External Dependency Inventory

These are all direct non-workspace-package dependencies in the 55 manifests; versions/features remain owned by those manifests and `Cargo.lock`. Packages absent from a row below have no direct external dependency. `serde` and `serde_json` inherited from the root are version 1; this list does not enumerate transitive registry packages.

| Packages (same prefix convention) | Direct external dependencies |
| --- | --- |
| contract | serde, serde_json, sha2 |
| artifact | serde, serde_json, sha2, object (`read,elf,std`, defaults off) |
| kernel | serde, serde_json, libloading |
| macros | syn (`full`), quote, proc-macro2 |
| basic-contracts, advanced-contracts | serde; dev: serde_json; advanced also sha2 |
| environment-contracts, workspace-contracts | serde |
| provider-contracts | zeroize, serde; dev: serde_json |
| basic-runtime | zeroize, serde, serde_json, agent-client-protocol (=3.3.0, `stdio,unstable_protocol_v2`), futures (`executor`), blocking |
| conformance, p2-model-meter-product | serde_json |
| p2-compaction-product | serde_json |
| p2-source-evidence, p2-evidence-store, p2-project-storage | serde_json, sha2 |
| p2-compaction-replay | sha2 |
| plugin-basic-tools, plugin-model-scripted, plugin-model-history-test, plugin-tool-fixture, plugin-tool-pending, plugin-advanced-summarizer | serde_json |
| plugin-basic-frontends | serde, serde_json, optional agent-client-protocol-schema (=1.11.0, defaults off) |
| plugin-model-http | zeroize, serde_json, curl (defaults off) |
| plugin-tool-receipts, plugin-evidence-journal | sha2 |
| plugin-storage-sqlite | rusqlite (0.37.0) |
| plugin-project-storage-sqlite | rusqlite (0.37.0), sha2 |
| plugin-provider-openai-api | curl (defaults off), serde_json |
| plugin-provider-chatgpt | serde, serde_json, zeroize, uuid (`v4`), curl (defaults off) |
| plugin-provider-secret-service | keyring-core (defaults off), zbus-secret-service-keyring-store (`rt-async-io-crypto-rust`, defaults off) |
| provider-credential-files | zeroize, libc |
| provider-discovery | serde_json, curl (defaults off), url |
| provider-protocols | zeroize, url; optional serde_json, openidconnect (defaults off), curl (defaults off), sse-parse |

Important feature boundaries: protocols `oauth2-pkce -> openidconnect`, `oauth-loopback -> oauth2-pkce + serde_json`, `siwc-verified -> oauth-loopback + curl`, `model-codecs -> sdk + basic-contracts + serde_json + sse-parse`. Auth strategies `oauth-browser` activates protocols with OAuth features; ChatGPT selects that feature and protocols `siwc-verified,model-codecs`; HTTP Model selects `model-codecs`. Agent has `staged-graph` and `graph-v2`; tools and host have `graph-v2`; host also `acp_v2_preview`. Frontends default to terminal, JSONL/ACP are mutually exclusive builds, `acp_v2` adds unstable schema v2. Scripted has `controlled-pending`, pending-tool has `alternate`, OpenAI has `c11-loopback-test`. These are artifact variants, not extra Cargo packages.

## Runtime Identity And Port Inventory

All following identities, slots, optionality and multiplicities are **current**, and must remain unchanged by the proposed organization/runner consolidation. `slot:Type` names the Rust contract interface. Unmarked imports have cardinality one; `?` means optional; `*0` means many with minimum zero. A dash means no imports/exports, not unrestricted authority. All native plugin declarations use version `1.0.0`.

| Package suffix after `phenix-plugin-` | Plugin identity | Exports | Imports |
| --- | --- | --- | --- |
| basic-agent | `phenix.basic.agent` | Input:BasicInput; Prepare:BasicTurnPrepare; GraphTurn:BasicGraphTurn; GraphTools:BasicGraphTools; Advance:BasicTurnAdvance; Output:BasicOutput; generated pending_model_request property | Model:Model; Catalog:ToolCatalog; Tools:ToolExecute; GraphTools:ToolGraph?; GraphExecutorAuthority:ToolGraphExecutor?; Receipts:ToolReceipts?; Skills:Skills; Context:Context; Session:Session; Journal:EventJournal; Storage:Storage?; Prepared:AdvancedPreparedModelRequest? |
| basic-tools | `phenix.basic.tools` | Catalog:ToolCatalog; Tools:ToolExecute; GraphTools:ToolGraph; GraphBuiltin:ToolGraphExecutor | Receipts:ToolReceipts?; StorageAuthority:Storage?; GraphContributors:ToolGraphExecutor*0; Contributors:ToolRegistrations*0 |
| basic-memory | `phenix.basic.memory` | Skills:Skills; Context:Context | Storage:Storage |
| basic-session | `phenix.basic.session` | Session:Session; GraphRecovery:GraphToolSession | Storage:Storage |
| event-journal | `phenix.basic.event-journal` | Journal:EventJournal | Storage:Storage |
| tool-receipts | `phenix.basic.tool-receipts` | Receipts:ToolReceipts | Storage:Storage; Session:Session; Recovery:GraphToolSession |
| basic-frontends | `phenix.basic.frontend-terminal`, `phenix.basic.frontend-jsonl`, `phenix.basic.frontend-acp` (feature variants) | Frontend:Frontend | - |
| model-http | `phenix.model.http` | Model:Model; ModelControl:HttpModelSession | Journal:EventJournal; Storage:Storage? |
| storage-sqlite | `phenix.basic.storage-sqlite` | Storage:Storage | - |
| environment | `phenix.environment.default` | Environment:Environment | - |
| provider-builtins | `phenix.provider.builtins` | Catalog:ProviderCatalog; Registrations:ProviderRegistrations; Authentication:ProviderAuth | Contributors:ProviderRegistrations*0 |
| provider-openai-api | `phenix.provider.openai-api` | Catalog:ProviderCatalog; Registrations:ProviderRegistrations; Authentication:ProviderAuth | Credentials:ProviderCredentials |
| provider-chatgpt | `phenix.provider.chatgpt` | Catalog:ProviderCatalog; Registrations:ProviderRegistrations; Authentication:ProviderAuth; Bearer:ProviderCredentials | Credentials:ProviderCredentials |
| provider-credential-files | `phenix.provider.credential-files` | Credentials:ProviderCredentials | - |
| provider-secret-service | `phenix.provider.secret-service` | Credentials:ProviderCredentials | - |
| advanced-model-adapter | `phenix.advanced.model-adapter` | Model:Model | Upstream:Model; Meter:ModelTokenMeter?; Transcript:EventJournal?; Checkpoints:CompactionStore?; Summarizer:CompactionSummarizer? |
| advanced-preparer-graph | `phenix.advanced.graph-preparer` | Change:ModelPreparationChange; generated prepared_model_request property | Source:BasicPendingModelRequest; Advance:BasicTurnAdvance; Preparer:ModelPreparation |
| advanced-preparer-noop | `phenix.advanced.preparer-noop` | Prepare:ModelPreparation | - |
| advanced-summarizer | `phenix.advanced.summarizer` | Summarizer:CompactionSummarizer | Upstream:Model |
| compaction-checkpoints | `phenix.advanced.compaction-checkpoints` | Checkpoints:CompactionStore | Storage:Storage; Journal:EventJournal |
| evidence-journal | `phenix.advanced.evidence-journal` | Evidence:SourceEvidence | Journal:EventJournal |
| evidence-store | `phenix.advanced.evidence-store` | Store:EvidenceStore | Source:SourceEvidence; Storage:Storage; ProjectStorage:ProjectStorage; Membership:ProjectMembership; ProjectSource:SourceEvidence |
| project-storage-sqlite | `phenix.advanced.project-storage-sqlite` | ProjectStorage:ProjectStorage; Membership:ProjectMembership; ProjectSource:SourceEvidence | - |
| workspace-selection | `phenix.workspace.selection` | Workspace:Workspace | Storage:Storage |
| model-scripted | `phenix.model.scripted` | Model:Model | - |
| model-history-test | `phenix.model.history-test` | Model:Model | - |
| model-meter-fixture | `phenix.model.meter-fixture` | Model:Model; Meter:ModelTokenMeter; Summarizer:CompactionSummarizer | - |
| provider-loopback-fixture | `phenix.provider.loopback-fixture` | Catalog:ProviderCatalog; Authentication:ProviderAuth; Runtime:ProviderRuntime | - |
| tool-fixture | `phenix.tool.fixture-reverse` | Registrations:ToolRegistrations | - |
| tool-pending | `phenix.tool.pending-a`, `phenix.tool.pending-b` (`alternate`) | GraphExecutor:ToolGraphExecutor | Receipts:ToolReceipts; StorageAuthority:Storage? |
| storage-fixture | `phenix.basic.storage-fixture` | Storage:Storage | - |

The other 24 root packages are libraries/hosts/definition emitters/test runners, not native implementation plugins. Five definition-package identities are `phenix.basic-contracts`, `phenix.advanced-contracts`, `phenix.provider-contracts`, `phenix.environment-contracts`, `phenix.workspace-contracts`. Their `profile = "definitions"` packages have no native implementation binding. `define(...)` in a native plugin also does not create an import or grant: adapter defines EventJournal, Storage, ModelTokenMeter, CompactionStore and CompactionSummarizer; evidence-journal defines Storage; evidence-store defines EventJournal; checkpoint plugin defines Storage. Preserve these transitive authority ceilings.

### Shared Interface IDs

Each ID below includes `@1`. These are the current canonical interface names behind the preceding port table, not proposed new contracts. Sources are `src/lib.rs` plus `model_preparation.rs`, `model_tokens.rs`, `compaction_summary.rs` in Advanced and `credentials.rs`, `model_session.rs` in Provider.

| Contract set | Rust interface -> contract ID |
| --- | --- |
| Basic | Model -> `phenix.basic.model@1`; ToolRegistrations -> `phenix.basic.tool-registrations@1`; ToolCatalog -> `phenix.basic.catalog@1`; ToolExecute -> `phenix.basic.tool@1`; Skills -> `phenix.basic.skills@1`; Context -> `phenix.basic.context@1`; Session -> `phenix.basic.session@1`; EventJournal -> `phenix.basic.journal@1`; Storage -> `phenix.basic.storage@1`; BasicInput -> `phenix.basic.input@1`; BasicTurnPrepare -> `phenix.basic.turn-prepare@1`; ToolReceipts -> `phenix.basic.tool-receipts@1`; GraphToolSession -> `phenix.basic.graph-tool-session@1`; ToolGraphExecutor -> `phenix.basic.graph-tool-executor@1`; ToolGraph -> `phenix.basic.graph-tools@1`; BasicGraphTools -> `phenix.basic.graph-tool-turn@1`; BasicGraphTurn -> `phenix.basic.graph-turn@1`; BasicTurnAdvance -> `phenix.basic.turn-advance@1`; BasicOutput -> `phenix.basic.output@1`; Frontend -> `phenix.basic.frontend@1` |
| Basic properties | BasicPendingModelRequest -> `phenix.basic.agent.property.pending_model_request@1` (get/watch); AdvancedPreparedModelRequest -> `phenix.advanced.graph-preparer.property.prepared_model_request@1` (get/watch). The latter is included in the Basic definition emitter for optional-import validity; the former is an agent-owned property. |
| Advanced | SourceEvidence -> `phenix.advanced.source@1`; EvidenceStore -> `phenix.advanced.evidence@1`; ProjectStorage -> `phenix.advanced.project-storage@1`; ProjectMembership -> `phenix.advanced.project-membership@1`; CompactionStore -> `phenix.advanced.compaction-checkpoints@1`; CompactionSummarizer -> `phenix.advanced.compaction-summary@1`; ModelTokenMeter -> `phenix.advanced.model-tokens@1`; ModelPreparation -> `phenix.advanced.model-preparation@1`; ModelPreparationChange -> `phenix.advanced.model-preparation-change@1` |
| Provider | ProviderCatalog -> `phenix.provider.catalog@1`; ProviderRegistrations -> `phenix.provider.registrations@1`; ProviderAuth -> `phenix.provider.auth@1`; ProviderRuntime -> `phenix.provider.runtime@1`; ProviderCredentials -> `phenix.provider.credentials@1`; HttpModelSession -> `phenix.provider.http-model-session@1` |
| Environment | Environment -> `phenix.environment@1` |
| Workspace | Workspace -> `phenix.workspace@1` |

Basic declares event `phenix.basic.prompt` and action `phenix.basic.submit` (`lib.rs:110-124`). The definition emitter selects 21 interfaces (20 normal contracts plus AdvancedPreparedModelRequest), Advanced selects 9, Provider 6, Environment 1 and Workspace 1. Property-generated interfaces and native metadata definitions must not be confused with emitter totals. Graph-v2 and staged graph source exists, but its presence is not proof that all shipping products use it or satisfy full v2.4 parity.

## Dependency Diagrams

Arrows mean **depends on**, not data flow or grants. The complete direct Cargo adjacency list is the package mapping above; these diagrams show its architectural projections.

```text
BASELINE CARGO (production edges retained)
SDK -----------------------> macros -------> contract
 |-----------------------------------------> contract
Basic/Environment/Workspace contracts ------> SDK
Advanced contracts ------------------------> Basic contracts + SDK
Provider contracts ------------------------> Basic contracts + SDK + protocols
protocols --[optional model-codecs]---------> Basic contracts + SDK
builtins/auth-strategies -------------------> Provider contracts + protocols
discovery ---------------------------------> protocols
credential-files backend ------------------> libc + zeroize (no kernel/SDK)
native plugins ----------------------------> SDK + selected shared contract sets/helpers
compaction replay -------------------------> Advanced + Basic contracts + SDK
artifact ----------------------------------> contract
kernel ------------------------------------> artifact + contract
Basic host --------------------------------> kernel + SDK + Basic/Provider/Environment contracts
conformance/HTTP/five P2 runners -----------> kernel + their listed contract sets/helpers
```

```text
IMPLEMENTED RUNNER CONSOLIDATION
All production/library/plugin edges above remain unchanged.
five P2 runner package nodes -> one test-only phenix-p2-products package
phenix-p2-products -> kernel, contract, Basic, Advanced, compaction replay
conformance and HTTP selected runner remain separate.
No contract -> implementation edge; no plugin -> host/kernel edge is introduced.
No Basic agent/model/tool -> frontend/ACP dependency is introduced.
```

```text
CURRENT SELECTED RUNTIME (simplified ports; grants remain explicit)
host -> Frontend decode/render            host -> agent Input / action dispatch
agent -> selected Model, Tools/Catalog, Memory Skills/Context, Session, Journal
Memory / Session / Journal -> Storage (SQLite in Basic)
HTTP Model -> Journal                     HTTP model installation -> private ModelControl
Advanced: agent Model -> ModelAdapter -> selected upstream Model
Advanced: EvidenceStore -> EvidenceJournal -> Journal -> Storage
Advanced: EvidenceStore -> ProjectStorage / Membership / ProjectSource
Advanced: Checkpoints -> Journal + Storage
provider management/auth kernels -> selected provider -> exact-account Credentials

PROPOSED RUNTIME: identical identities, ports, selections and grants.
Test-runner package consolidation changes no runtime graph.
```

## Shipping And Test Selections

The authoritative current composition code is `flake.nix`, not the destination tree. `mixes/default.nix` imports production Basic/Advanced validators and testing helpers. `products/*.nix` in the layout guide are still proposed destinations, not current selection evidence.

| Current selection / source | Actual selection and distinction to preserve |
| --- | --- |
| `basicCompositionSpec`, `flake.nix:1783-1825` | Agent, scripted Model, tools, memory, session, journal, SQLite, terminal, JSONL, ACP; Basic definitions. Entry `phenix.basic.input@1/submit` on agent. 11 explicit bindings; root/descendant grants and authority resource `phenix.session.s1` remain declared. Frontends have no agent import slots. |
| `basicHttpCompositionSpec`, `1881-1926` | Removes scripted artifact, adds HTTP Model and Provider definitions, changes agent Model slot, binds HTTP Journal, adds journal append and private install/storage root grants. Not the default offline Basic preset. |
| `advancedCompositionSpec`, `1974-2122` | Basic plus evidence-journal, evidence-store, project-storage-sqlite, compaction-checkpoints, advanced-model-adapter and Advanced definitions. Agent Model binds adapter; adapter Upstream remains scripted. Explicit project/evidence/storage/checkpoint grants, not automatic authority. Meter, automatic summarizer and graph preparer are not selected by this preset. |
| `historyVerificationCompositionSpec`, `1930-1938` | Adds history-test artifact and changes only agent Model binding. Testing helper appends rather than removes the old artifact; unselected scripted remains in artifact list. |
| `advancedVerificationCompositionSpec`, `2123-2132` | Adds history-test and changes adapter Upstream, not Basic agent/state implementation. Original scripted artifact remains listed. |
| `providerManagementCompositionSpec`, `1830-1871` | Separate provider root selecting only builtin provider + Provider definitions; catalog/auth operations resource `phenix.providers.user`. Not provider execution authorization. |
| OpenAI management / credentials / resolver, `554-649` | Management selects OpenAI + file Credentials; credential management/resolver select file plugin. Exact resource `phenix.credentials.6f70656e6169.617069.706572736f6e616c`; provider gets resolve, credential management gets import/bind_environment/revoke. |
| ChatGPT management / resolver / logout, `678-807` | Separate provider + file Credentials roots for `phenix.credentials.6f70656e6169.63686174677074.706572736f6e616c`. Resolve/import/CAS grants are explicit; logout entry is provider revoke with downstream resolve/CAS, not broad frontend access to secrets. |
| Installed Basic wrappers, `packages` at `3008-3236` | `phenix-basic`, terminal/JSONL/ACP use Basic; managed-ACP adds builtin management; HTTP/HTTP-ACP use HTTP composition; OpenAI API ACP uses bundle; ChatGPT ACP uses management/resolver; P1 auth ACP selects both providers' separate management/credential/resolver roots. Preserve CLI names, state-directory behavior, profile checks and credential opt-in. |
| Advanced wrappers / manifest | `phenix-advanced` and verification outputs exist; foundation manifest explicitly sets `status = "scaffold"`, `p2_accepted = false` (`2133` onward). Availability is not complete Advanced release acceptance. |
| Packaged v2 Basic, `basicV2Composition*`, `1610-1725` | Separate feature-selected v2 host/agent/tools, receipts, controlled Model and fanout pending-tool variants. `phenix-basic-v2-*` outputs do not replace default P1 composition. Preserve separate exact-head gate. |
| Environment / Workspace products, `342-390` | Environment plugin + Environment definitions; workspace plugin + SQLite + Workspace and Basic definitions. Neither is silently added to default Basic/Advanced artifact lists. |
| `mixes/production/native-only.nix:5-12` | Guard rejects history-test, provider-loopback and four Python-name fragments. It does **not** exhaustively reject scripted/meter/tool/storage fixtures. Do not claim it proves complete production fixture exclusion. Scripted is deliberately selected today. |
| Public source, `flake.nix:22-40` | Public SDK includes contract, SDK, macros, include and manifests/lock; external example plugin source adds `examples`, not private kernel sources. Preserve this distribution boundary. |
| Native artifact packaging, `112-118`, `1220-1240` | Independently packaged `lib/plugin.so`, descriptor and digest-bearing `phenix.native-package.v2` record. Path/crate moves must not rename plugin IDs or collapse independently loadable artifacts. |

### Acceptance And Fixture Matrix

This inventories selection families and all root fixture packages, not a claim that any check ran in this task. The exhaustive exposed check assignment is `flake.nix:3262-3350` plus `declarativeCliChecks`; generic CI discovers it rather than maintaining a separate crate-based check list.

| Test family | Current implementations/fixtures and source evidence |
| --- | --- |
| Basic Model/tool/state/agent | `b4-scripted`, `b4-http`, `b3-tools`, `tool-contributors`, `b2-journal`, `b2-session`, `b2-sqlite`, `b3-memory`, `b5-agent`; scripted controlled-pending, tool contributor fixture, storage fixture, SQLite/restart, real journal/session/memory/tools. `flake.nix:1248-1573` and named conformance binaries. |
| Frontends/host/observers | `f1-frontends`, `f2-acp`, host smoke, credentials, history, streaming, HTTP/default-state gates. Feature-built terminal/JSONL/ACP; official SDK client remains standalone. SQLite observer crash-before/crash-after/replay scenarios remain in `sqliteRestartCheck:1392-1409`. ACP/observers are out of implementation scope. |
| HTTP selected native | `b4-http` invokes `phenix-http-selected-product` with HTTP Model, real journal, SQLite and Basic/Provider definitions (`1304-1324`); tests OpenAI chat/responses and Anthropic protocols. Preserve private install/CAS/denial tests. |
| Provider runtime/auth/backends | Loopback provider only through separate testing provider mix; production OpenAI loopback override is a separate `c11-loopback-test` artifact (`509-538`); builtins/OpenAI/ChatGPT + file/Secret Service gates remain independent (`392-1003`). Never add a loopback endpoint switch to production default code. |
| Model meter | `phenix-p2-model-meter-product` + meter fixture + Basic/Advanced definitions (`1021-1058`); synthetic byte tokens explicitly test-only. |
| Compaction/adapter/preparation | Replay pure-library checks; adapter pending HTTP/metered/compaction products; selected summarizer separate opt-in artifact; checkpoints + real journal/SQLite; staged agent + graph preparer + noop + optional meter fixture (`1061-1141`, `1450-1542`, named P2 test scripts). Preserve pending/cancel/restart semantics. |
| Source/evidence/project | Five P2 runner sources distinguish journal source verification, ledger, native SQLite project isolation/membership/source/project evidence; Basic seeds journal in separate process. `tests/p2-*-product.py`, corresponding `flake.nix` runner/check declarations and `crates/phenix-p2-*/src/main.rs` are owning evidence. |
| v2 controlled/fanout | Same agent/tools crates compiled with `graph-v2`, receipts artifact, scripted controlled variant and separate pending-a/b tool artifacts. Preserve physical worker cancellation/late-result and authority checks; do not infer whole graph-v2 parity. |
| Generic ABI/kernel | Conformance binaries and 36-member external example workspace cover native authoring, graph, value/state/reference/schema, import resolution, pending, lifecycle, generations and composition/shipping. Raw malformed fixtures and same-ID replacement libraries must remain independently built. |
| Testing mix catalog | graph: `m1-graph`; storage: `b2-sqlite`, project-storage/membership/source; frontend: f1/f2/host; advanced: adapter/checkpoints/evidence-store. `mixes/testing/{graph,storage,frontend,advanced}.nix` list checks, not runtime compositions. Model/provider helpers actually transform compositions. Python PoC helper is `production = false`, standalone source. |

## Nested Workspace Inventory

These packages are not omitted from the audit and are not counted as root members. **Current -> target is unchanged for every row in this section.** Retention is intentional: they prove public SDK independence, adversarial ABI behavior, replacement identity, graph/lifecycle ordering or protocol compatibility. Combining them into production crates would destroy that evidence. The `examples` workspace uses only SDK plus its local contract libraries; these are test/example APIs, not competing production contract sets.

In the next table, paths are `examples/<path>`, Cargo names are `example-<path>` unless explicitly noted. Every package has direct dependency `phenix-sdk`; the extra-dependency column is exhaustive. `build SDK` indicates a separate SDK build dependency. Port types are local fixture contracts, not Basic production interfaces. `?`, `*0`, `*1` retain optional/many meanings.

| Path(s) / package | Extra Cargo dependencies | Identity; exports; imports | Specific retention reason |
| --- | --- | --- | --- |
| contracts | None | No plugin; common Transform/Run/Count/Choice/Other/Scoped/Relay/Bump/Resource/Lifecycle/Pending/Revision schemas | Shared independent ABI fixture API. |
| agent-contracts | None | No plugin; Run/Complete/Execute/ToolCount schemas | Standalone agent fixture API. |
| reactive-contracts | None | `example.reactive-contracts`, inert descriptor emitter defining `example.echo@1` and `example.counter@1`, no imports/exports | Shared live-state example API. |
| graph-contracts | None | `example.graph-contracts`, definitions/graph package, not native effects | Separately package graph definitions/events/actions/properties. |
| authoring-api | None | `test.authored-api`, definitions Counter/Relay and property types | External authoring API independence. |
| provider | example-contracts | `example.provider`; Transform:TransformContract, ReadCount:ReadCountContract; no imports | Baseline independently loaded provider. |
| alternate | example-contracts | `example.alternate`; Transform:TransformContract; no imports | Replaceable provider selection. |
| consumer | example-contracts | `example.consumer`; Run:RunContract; Provider:TransformContract | Import resolution/invocation. |
| cycle | example-contracts | `example.cycle`; Cycle:local CycleContract; Again:CycleContract | Cycle rejection fixture. |
| choice | example-contracts | `example.choice`; Choice:ChoiceContract; Provider:TransformContract, OptionalCounter:ReadCountContract? | Optional selection behavior. |
| other | example-contracts | `example.other`; Other:OtherContract; Provider:TransformContract pinned to `example.provider` | Provider identity constraint. |
| scoped-client | example-contracts | `example.restricted`; ScopedRun:ScopedRunContract; Delegate:RelayContract, EffectLimit:BumpContract? | Attenuated transitive authority. |
| relay | example-contracts | `example.relay`; Relay:RelayContract; Effect:BumpContract | Nested effect/authority path. |
| resource-provider | example-contracts | `example.resources`; Bump:BumpContract, ResourceCount:ResourceCountContract; no imports | Resource-scoped state/effects. |
| lifecycle-provider | example-contracts | `example.lifecycle.z-provider`; LifecycleService:LifecycleServiceContract; no imports | Dependency lifecycle ordering independent of lexical order. |
| lifecycle-consumer | example-contracts | `example.lifecycle.a-consumer`; LifecycleRun:LifecycleRunContract; Service:LifecycleServiceContract | Matching lifecycle consumer and failure behavior. |
| pending | example-contracts | `example.pending`; PendingRun:PendingRunContract; Service:TransformContract | Pending ABI variants, LTO and security scenarios. |
| pending-immediate | example-contracts | Same `example.pending` identity/ports as pending | Immediate replacement artifact, not a second identity. |
| pending-consumer | example-contracts | `example.pending-consumer`; PendingConsumer:PendingConsumerContract; Worker:PendingRunContract, TransitiveEffect:TransformContract | Nested pending/effect propagation. |
| pending-security | example-contracts | `example.pending-security`; parent:PendingConsumerContract; Worker:PendingRunContract, Sibling:PendingRunContract*1, Service:TransformContract | Raw adversarial ABI metadata/tickets, intentionally not normal plugin macro. |
| revision-a, revision-b (two packages) | example-contracts | Both `example.revision`; Revision:RevisionContract; no imports | Separate bytes/digests/generations, both emit `libexample_revision.so`; must build separately. |
| agent | example-agent-contracts | `example.agent`; Run:RunContract; Model:CompleteContract, Tool:ExecuteContract | External agent Model/Tool composition. |
| agent-model | example-agent-contracts | `example.agent-model`; Complete:CompleteContract; no imports | Model behavior/adversarial reply fixture. |
| agent-alternate | example-agent-contracts | `example.agent-alternate`; Complete:CompleteContract; no imports | Selected model replacement. |
| agent-tool | example-agent-contracts | `example.agent-tool`; Execute:ExecuteContract, ToolCount:ToolCountContract; no imports | Tool effect witness. |
| authoring-provider | example-authoring-api | `test.authored-provider`; counter:Counter plus generated counter/observed properties; no imports | Stateful SDK authoring/property export. |
| authoring-client | example-authoring-api | `test.authored-client`; relay:Relay; counter:Counter, fallback:Counter?, all:Counter*0, count:Count, observed:Observed, notes:Notes | Typed imports, optional/many/property invocation. |
| authoring-field | None | `test.authored-field`; generated notes property; no imports | Direct field/property authoring. |
| graph-a | example-graph-contracts | `example.graph-a`; task:Task, slow:SlowTask, reference_source:ReferenceSource, result/signal/pending_signal properties; Workers:SlowTask*0 | Graph source/pending/property behavior. |
| graph-b | example-graph-contracts | `example.graph-b`; slow:SlowTask, each_collect:EachCollect, task:Task, consume:Consume, join_consumer:JoinConsumer, reference_consumer:ReferenceConsumer, optional_consumer:OptionalConsumer, many_consumer:ManyConsumer, nested_optional_consumer:NestedOptionalConsumer, result property; Workers:SlowTask*0 | Graph join/each/optional/reference consumer cases. |
| graph-c | example-graph-contracts | `example.graph-c`; task:Task, hook:StateHook, pending_hook:PendingStateHook, cancel_probe get/set and receipt get/set/watch properties; a:ResultA, b:ResultB, signal:SignalA, pending_signal:PendingSignalA | Graph multi-provider/property observer hooks. |
| value-provider | build SDK | `test.state-provider`; refs:`test.refs@1`, test:`test.provider@1`, counter/notes properties; no imports | Raw generated value/state descriptor and ABI. |
| value-client | build SDK | `test.state-client`; test:`test.state@1`; counter/notes provider properties, refs:`test.refs@1`, many:`test.refs@1`*1 | Raw reference/property consumer. |
| reactive-provider | build SDK | `test.state-provider` or `test.state-alternative`; same exports as value-provider | Feature variants bad-init/schema/table/alternative must stay independent artifacts. |
| reactive-client | build SDK | `test.state-client`; same exports/imports as value-client | Feature variants bad-init/schema/table, independent reactive ABI. |

Normal macro-defined fixture exports/imports also include their `define(...)` schemas. Property macros generate owner-qualified contract IDs; the graph/authoring property APIs in their contract sources are the authoritative expansion inputs. Raw state/value metadata comes from each `build.rs`; pending-security metadata is in `src/lib.rs:11-34`. Shared fixture contract definitions remain at the current `examples/{contracts,agent-contracts,graph-contracts,authoring-api}/src/lib.rs` paths and `examples/reactive-contracts/src/main.rs`. They are not relocated into the public production contract sets.

| Other workspace/standalone package and path | Direct dependencies | Identity / ports and retention reason |
| --- | --- | --- |
| `phenix-python-host-fixture`, `experiments/graph-v2-python/plugin` | SDK, example-contracts, pyo3 (`auto-initialize`); build: contract, example-contracts | `example.python-host`; Run:RunContract and Provider:TransformContract import pinned to `example.provider`; experimental graph/Python bridge, never production plugin. |
| `phenix-graph-provider-fixture`, `experiments/graph-v2-python/provider` | SDK, example-contracts; build: contract, example-contracts | Includes `examples/provider/src/lib.rs`, so `example.provider` and its Transform/ReadCount exports; adds experimental graph section, not a new provider identity. |
| `phenix-python-kernel-verification`, `experiments/graph-v2-python/runner` | contract, kernel, artifact, serde_json | No native plugin; standalone experimental integration runner, preserve private kernel outside public SDK. |
| `phenix-rust-greeter`, `examples/python-trivial/rust-equivalent` | SDK | `example.rust-greeter`; Api:`example.greeter@1`, no imports; Rust comparison artifact for standalone Python PoC. |
| `phenix-python-trivial-verification`, `examples/python-trivial/runner` | contract, artifact, kernel | No plugin; standalone equivalence/verification runner. |
| `phenix-p1-acp-client-check`, `tests/acp-sdk-client` | agent-client-protocol (=3.3.0, `process,unstable_protocol_v2`), tokio (`macros,rt-multi-thread`) | No native plugin; independent official ACP client/process test. **Untouched**, including its separate lockfile/workspace and binaries. |

The graph/Python workspace manifest selects plugin/provider/runner; two Python-trivial packages and ACP client declare their own workspaces. No nested package is merged merely because it uses the same SDK or plugin ID as another fixture.

## Blockers And Acceptance

1. **ACP and observers remain untouched.** No edits to Basic runtime ACP modules, frontend codecs/features, official client fixtures, journal observer storage/cursors/receipts, notification flow or cancellation/disconnect semantics are part of this proposal or this documentation change. Existing host/frontend crate boundaries remain explicit. A folder destination is not permission to refactor their implementations.
2. **No automatic Basic domain merge.** Agent/tools/journal/memory are independently loaded and own different lifecycle/state/authority. Staged graph preparer reads an agent-owned property; graph private node names and generated graph sections are additional coupling. Public typed exports/authorized observation and exact-head parity must exist before revisiting those boundaries. Source contains graph-v2 work; verify selected composition and gates, do not infer completion from names.
3. **Durability is not observation.** Journal append, receipts, storage CAS, checkpoints, membership and evidence versioning remain required owned effects. Do not turn them into asynchronous notifications, duplicate their state in a combined agent, or introduce another scheduler/event bus.
4. **Provider helper boundaries have real constraints.** Contracts currently depend on protocols; merging protocols upward creates a cycle. Builtin data deliberately has no network I/O, discovery has HTTP dependencies, auth has feature-gated browser flow, credentials have backend/OS restrictions. Smaller crate count alone does not justify broadening these closures.
5. **Fixture selection needs an explicit decision.** Scripted is both deterministic fixture-like code and the actual default offline Model. Keep its current selections during moves. A future production/offline split needs its own composition/guard acceptance; do not claim all fixtures are already excluded or silently change the shipping default.
6. **P2 runner consolidation has a cache tradeoff.** The original five-to-one merge broadened source invalidation; the committed isolation fix and automated regression now enforce 1/5 runner-source invalidation. Shared dependency/manifest edits can still invalidate all five. Individual executable names, tests and derivations are preserved; HTTP and generic conformance remain isolated. Timing/cache comparisons remain unmeasured, as detailed above.
7. **Relocations are atomic build changes.** Update Cargo members, path dependencies (including nested fixture/workspace paths), lockfile, build-script inputs, Nix filesets/public source exports/library copy names/product references, CI generation/prebuild/cache references, tests and tools together. `ci/flake-check-config.nix` discovers checks from flake, has five shards and a prebuild hotset with kernel/describe/conformance/examples/Basic contracts/journal/SQLite. Preserve source-isolated per-plugin derivations (`flake.nix:1214-1240`); do not batch production plugins just to match folders.
8. **Local and remote acceptance are distinct.** Historical local sessions passed focused gates but did not run the full flake/demo/release commands, and `devenv test` could not start. Remote generated full acceptance passed at the preceding recorded exact heads, including documentation reconciliation `9690f0d`. HTTP has seven local passing gates and full no-build evaluation only; remote exact-head CI must follow its commit. This does not prove performance, complete graph-v2 parity or removal of Advanced scaffold constraints. Keep existing M1 user flows supported while staged migration proceeds.

## Implemented Versus Proposed

| State | This document's claim |
| --- | --- |
| Implemented at inspected baseline | 55 root packages and the source-declared dependencies, identities, definitions, fixture variants and product selections above. Source inspection is not an assertion that all tests pass. |
| Implemented through this increment | Five P2 packages replaced by one unpublished multi-binary package under testing/products; per-runner Nix source isolation and automated regression implemented. Environment, Basic SQLite and now HTTP relocated to plugins/shared with implementation bytes unchanged. 51 root packages: 47 in crates, one in testing/products, three in plugins/shared; 133 internal dependency declarations; 31 unchanged plugin packages. |
| Proposed | Remaining product-aligned relocations; credential-files backend/plugin is next subject to restricted native-boundary audit. No production domain merge, runtime identity, contract, grant or selection change. |
| Change scope | Prior consolidation/relocations and runner regression retain their recorded commit evidence. This HTTP increment changes membership/relative paths, Nix closures/pruning and the HTTP source location with unchanged lockfile; accompanying documentation updates these two Run3 documents and plugin-layout.md. |
| Verified / integrated | Historical exact-head remote acceptance includes docs-only head 9690f0d; runner regression evidence remains at 9fac7b9. HTTP passes seven local gates and full no-build evaluation, with remote exact-head CI required after commit. Local devenv is unavailable. No performance or complete graph-v2 parity claim. Not merged/integrated; Run2, ACP, observers and broader graph/domain consolidation remain untouched. |
