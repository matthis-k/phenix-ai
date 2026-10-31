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

Current committed HEAD `9fac7b992b338021bf2ecd8a5c60d7514a18f018`, parent `6a5d3424930e685a119693c377f5f8731a4b2b3f`, adds `tests/p2-runner-isolation.py` and makes it a mandatory step in the existing cache-isolation workflow. Run it locally with:

```sh
nix shell nixpkgs#python3 -c python3 tests/p2-runner-isolation.py
```

The script copies tracked and untracked nonignored files into a temporary snapshot, excluding Git metadata and ignored build outputs; it never edits the checkout. It evaluates the five actual runner derivation paths and kernel/Environment/public-SDK controls, then adds one blank line to each runner independently. Each perturbation must change exactly that runner's derivation and leave all controls unchanged. A `finally` block restores the source bytes after every perturbation, and reevaluation must match the complete baseline before proceeding. The temporary snapshot is removed on exit, including on failure.

At this exact HEAD, [generated CI 41194107549](https://github.com/matthis-k/phenix-ai/actions/runs/41194107549) passed evaluation, all four builders, the scheduler, both runnable test packages and independent final acceptance. On 2026-10-31 UTC, the final acceptance job completed at `10:50:36Z`, verifying output commit identity, shipping evidence and executable-test results. The [existing cache-isolation workflow run 41194107132](https://github.com/matthis-k/phenix-ai/actions/runs/41194107132) also succeeded: the five-runner regression step completed at `10:47:17Z`, and the job at `10:50:41Z`. This is workflow enforcement, not a claim that GitHub branch-protection settings changed.

Committed counts remain 51 workspace packages (48 in crates, one in testing/products, two in plugins/shared), 133 internal dependency declarations and 31 plugin packages. The other agent's dirty HTTP relocation remains proposed until its next committed increment and is not covered by these exact-head runs. These commits are not merged/integrated. No speed, cache-hit or binary-size improvement is measured; complete graph-v2 parity and Advanced scaffold constraints remain outside this isolation result.
