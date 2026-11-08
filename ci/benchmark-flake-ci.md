# Exact-head CI cold/warm benchmark

The isolated branch \`bench/flake-ci-cold-warm-20261108\` exercises the full
\`checks.x86_64-linux\` inventory from the current Phenix AI prototype with
immutable \`phenix-flake-ci\` tooling. It does **not** alter required CI, the
consumer's Nix lock, or the consumer's shipping check definitions.

## Method

- **Cold project cache:** a unique per-run and per-attempt Actions cache key,
  with no restore prefixes. This is not a source-only build; the standard
  public Nix binary substituters remain enabled.
- **Warm project cache:** same consumer commit, CI library revision, Nix
  planner, DAG, runner image and exact cache key. Warm starts in a fresh runner
  and restores only the cache exported from the preceding cold job.
- Nix evaluates and runs all checks in a **single native Nix DAG** in each
  phase, then verifies exact-head outputs. Cold roots pin verified outputs
  through the cache action's post-job GC.
- Before the warm build, inspect **every Nix store path** in the cold check
  output closure with offline \`nix path-info\`. Record missing paths even when
  a fallback warm rebuild succeeds; acceptance rejects partial retention.
- Require an actual **primary-key cache miss** on cold and a **primary-key
  cache hit** on warm, rather than inferring the hit from wall-clock time.
- Capture per-phase build duration, direct Nix derivation build events,
  repeated derivations in the same store, Cargo compile events and repeated
  crate-name compile events, and full GitHub job elapsed times.
  The report includes the cold-to-warm sequential critical path and handoff.

The runner's \`ImageVersion\`, \`nix --version\`, plan digest, graph digest,
outputs and closure identities must all match before results can be compared.
The check planner and log reporter come from the same pinned Flake CI commit.
Nix build events are exact derivation identities. The Cargo metrics count
same-crate-name compilation events; feature sets and profiles can differ,
so those events do **not** prove identical reusable Cargo artifacts. No direct Tailscale access, secrets, or cache publication to
production cache keys is used.

This intentionally measures the **native single-store backend**. It must
not be presented as a measurement of the distributed four-worker backend or
isolated GitHub matrix shards. Run a separately controlled backend benchmark
before claiming cross-backend speedups.

## Artifacts

\`phenix-ci-bench-cold\`: cold graph, exact-head evidence, Nix build log,
complete closure inventory, metrics.

\`phenix-ci-bench-warm\`: same evidence plus pre-build offline cache-retention
report and metrics.

\`phenix-ci-bench-compare\`: JSON and Markdown comparison with GitHub job
timestamps. Cache retention below 100% fails the comparison.

The push trigger is intentionally restricted to this benchmark branch. A
fresh push on that branch executes the benchmark once; \`workflow_dispatch\`
can be used after adopting the workflow on the repository default branch.
