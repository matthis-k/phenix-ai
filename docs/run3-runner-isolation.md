# Run3 Runner Source Isolation

The follow-up to PR #1221's original `a014a07d1822b39cc7e35ed791c3a3e104663121`
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

The filtered worktree passed the five runner product checks
(`p2-model-token-meter-product`, `p2-compaction-checkpoints-product`,
`p2-source-evidence-product`, `p2-evidence-store-product`,
`p2-project-storage-product`), `m1-contract` and `m1-public-sdk` via
`nix build --no-link path:.#checks.x86_64-linux.<name>`. These local results
included the separately pending Environment relocation, not an exact-head CI
claim. Three timed-out builds preceded the successful retry with `--max-jobs 2`.
`devenv test` could not start because the executable is absent.

Counts remain 51 workspace packages and 133 internal dependency declarations.
Original exact-head CI evidence remains in the consolidation inventory; it must
not be attributed to this new increment before its own CI completes.
