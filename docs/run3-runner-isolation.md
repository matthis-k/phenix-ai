# Run3 Runner Source Isolation

The committed follow-up `50e7df808f4fb5a9b7255820b030b02fd1c35d44` to PR #1221's original `a014a07d1822b39cc7e35ed791c3a3e104663121`
retains the unpublished shared package and its five binary names. Each Nix runner
closure now contains the unchanged package manifest and only its own `src/bin`
file. Cargo's default binary autodiscovery handles this directly; no generated
manifest, dummy source, compatibility crate or runtime change is needed.

On 2026-10-31, controlled single-blank-line edits to each of the five runners
independently changed only its corresponding product derivation (1/5 each).
The four unrelated product derivations and kernel, Environment product and public
SDK controls remained identical in all five experiments. Every perturbation was
removed afterward. This contrasts with the original committed consolidation's
measured 5/5 invalidation for a source-evidence edit, versus 1/5 before consolidation.
Shared dependency/manifest changes can still invalidate all runners; compilation
remains five separate derivations. No timing or cache-hit benefit is claimed.

During the original local isolation session, the filtered worktree passed the five runner product checks
(`p2-model-token-meter-product`, `p2-compaction-checkpoints-product`,
`p2-source-evidence-product`, `p2-evidence-store-product`,
`p2-project-storage-product`), `m1-contract` and `m1-public-sdk` via
`nix build --no-link path:.#checks.x86_64-linux.<name>`. These local results
included the then-pending Environment relocation, not an exact-head CI
claim. Three timed-out builds preceded the successful retry with `--max-jobs 2`.
`devenv test` could not start because the executable was absent; that local gate did not pass. Remote full generated acceptance subsequently passed, independently of this local limitation.

The original isolation commit passed [exact-head generated CI 41167127142](https://github.com/matthis-k/phenix-ai/actions/runs/41167127142), including evaluation, all four builders, both runnable tests and independent final shipping-evidence acceptance. Environment subsequently committed at `d7412fba7e862a8a0f470a5c566aa3bede2d0d67` and SQLite at `6a5d3424930e685a119693c377f5f8731a4b2b3f`, with their own successful exact-head runs [41167975166](https://github.com/matthis-k/phenix-ai/actions/runs/41167975166) and [41170082526](https://github.com/matthis-k/phenix-ai/actions/runs/41170082526). See the [consolidation inventory](run3-crate-consolidation.md) for relocation evidence and the original consolidation run 41157912213.

## Automated Regression

Historical regression commit `9fac7b992b338021bf2ecd8a5c60d7514a18f018`, parent `6a5d3424930e685a119693c377f5f8731a4b2b3f`, adds `tests/p2-runner-isolation.py` and makes it a mandatory step in the existing cache-isolation workflow. Run it locally with:

```sh
nix shell nixpkgs#python3 -c python3 tests/p2-runner-isolation.py
```

The script copies tracked and untracked nonignored files into a temporary snapshot, excluding Git metadata and ignored build outputs; it never edits the checkout. It evaluates the five actual runner derivation paths and kernel/Environment/public-SDK controls, then adds one blank line to each runner independently. Each perturbation must change exactly that runner's derivation and leave all controls unchanged. A `finally` block restores the source bytes after every perturbation, and reevaluation must match the complete baseline before proceeding. The temporary snapshot is removed on exit, including on failure.

At that exact regression SHA, [generated CI 41194107549](https://github.com/matthis-k/phenix-ai/actions/runs/41194107549) passed evaluation, all four builders, the scheduler, both runnable test packages and independent final acceptance. On 2026-10-31 UTC, the final acceptance job completed at `10:50:36Z`, verifying output commit identity, shipping evidence and executable-test results. The [existing cache-isolation workflow run 41194107132](https://github.com/matthis-k/phenix-ai/actions/runs/41194107132) also succeeded: the five-runner regression step completed at `10:47:17Z`, and the job at `10:50:41Z`. This is workflow enforcement, not a claim that GitHub branch-protection settings changed.

The subsequent docs-only reconciliation at `9690f0d1e843ba6a9a77fe9d56f3842a90e3ddc1` (parent `9fac7b9`) passed [generated CI 41197840798](https://github.com/matthis-k/phenix-ai/actions/runs/41197840798): 89/89 checks, all four builders, both executables and final acceptance at that exact head. It changed no sampled derivations versus `9fac7b9`; the new regression evidence above still belongs to `9fac7b9`, not the documentation head.

## HTTP Relocation Increment

HTTP Model is committed at `e58b5e68ce76c0c027fb329e9df0110125f2b52d`, parent `9690f0d1e843ba6a9a77fe9d56f3842a90e3ddc1`, under `plugins/shared/phenix-plugin-model-http`; see the [inventory's HTTP audit and verification](run3-crate-consolidation.md#http-relocation-increment). Final counts are 51 packages (47 in crates, one in testing/products, three in plugins/shared), 133 internal dependency declarations and 31 plugin packages. Five Rust files totaling 56,763 bytes and Cargo.lock are unchanged. Seven local Nix gates passed, including contract formatting/Clippy/tests and public SDK isolation; full `--no-build` evaluation passed, not a full build. `devenv` remains unavailable.

[Exact-head generated CI 41198992527](https://github.com/matthis-k/phenix-ai/actions/runs/41198992527) passed 89/89 checks with matching plan/evidence, actual builds on all four builders, both runnable packages and independent final acceptance verifying the source SHA and shipping/executable evidence. All other workflows passed except the intentionally skipped legacy candidate; trusted store publication was skipped. [Regression workflow 41198992212](https://github.com/matthis-k/phenix-ai/actions/runs/41198992212) reports five PASS results on synthetic merge `22de1f2f3b68dd9c3b18def92abc80d36545c083`, which includes the source head, not on that exact source SHA.

On 2026-10-31 UTC the generated run took `11:57:38Z` to `12:22:17Z`, **24m39s**, versus the parent's **3m58s**. Scheduler artifacts report **1269.76s** versus **26.96s**. There were 295 main plus two executable derivations built and 297 remote dispatches across all four builders. Actions cache had a primary miss, fallback restored 1,490,795,226 bytes plus an additional 40,055,477 bytes, and saved 1,573,177,199 bytes. These observations are not a controlled speed comparison or an exact hit rate; portable evidence is in the linked CI runs/artifacts, including [parent run 41197840798](https://github.com/matthis-k/phenix-ai/actions/runs/41197840798).

Downloaded CI artifact comparison found all 89 check derivation paths changed versus the parent. All 15 local sampled derivations also changed once during relocation: five runners, three controls, HTTP and six unrelated packages (`phenix-basic-runtime`, `phenix-describe`, `phenix-plugin-environment`, `phenix-plugin-model-scripted`, `phenix-plugin-provider-builtins`, `phenix-plugin-storage-sqlite`). HTTP's own path invalidation is expected; broad unrelated root-manifest/pruning invalidation is undesirable and calls for future isolated-source work. This does not contradict source-perturbation isolation: a local HTTP-only edit changed HTTP alone, leaving all 14 other samples unchanged. Exact HTTP derivation paths are recorded in the inventory; portable remote evidence is in the linked CI artifacts, not ephemeral local logs.

Existing resolved-address policy validation and literal `anthropic-version` header responsibility caveats predate the move; behavior fixes need separate minimal reviews, not a relocation refactor. Next candidate is provider credential-files backend/plugin after restricted native-boundary audit, not a protocols merge. These increments are not merged/integrated. No speed, cache-hit or binary-size improvement is measured; complete graph-v2 parity and Advanced scaffold constraints remain outside this isolation result.
