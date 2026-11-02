# Exact-head CI evidence

## fe71369afea8359dc2d18f4e4cd5cf44d770330f

* Run: https://github.com/matthis-k/phenix-ai/actions/runs/41418279133
* Exact-head Nix planning/evaluation passed. Formatting and generated-workflow
  verification passed. See the PR's live checks for independent SDK/isolation jobs.
* Coordinator job 125187941395 failed **before any Nix build** in the pinned
  `phenix-flake-ci` helper `lib/nix-remote-workers.py`, `peers`, line 65:

  ```python
  for peer in status.get("Peer", {}).values():
  # AttributeError: 'NoneType' object has no attribute 'values'
  ```

  Tailscale was connected; its initial status had a null Peer map. This is an
  infrastructure/discovery failure, not an engine compilation or semantic failure.
  No native acceptance success is claimed for this experiment head.
* GitHub refused to rerun the planning job while sibling builder jobs remained
  active (`403: The workflow run containing this job is already running`).
* This evidence-only update triggers the existing PR concurrency rule, which
  cancels the obsolete run and schedules the unchanged experiment sources again.
  No workflow, CI policy, upstream pin or production PR is modified to bypass the
  failure. The new run must independently pass before acceptance is claimed.

## Control parent (separate evidence)

PR #1239 at `33d8c4bbec8d450e9352d71c7b790854fbb468a2` passed run
https://github.com/matthis-k/phenix-ai/actions/runs/41402562438 including four
builders, native/shipping checks and runnable tests. This is not evidence for any
later experimental head. Local experiment results and scope limits are in README.

## Structural-port correction after 4bba634c

The native control now exposes structural Portable Message, Identity and ResultValue
schemas. The old Text/JSON wrapper only let the kernel validate the outer string;
that was weaker evidence for typed-port equivalence. The host now also asserts
that a malformed Text-only payload fails with SchemaRejected before admission.
Domain operations, graph scheduling, authority, pending/cancel hooks and ABI-3 are
unchanged. All changes remain experimental.

The local environment became unavailable before this correction. It is reviewed
source, **not locally compiled/tested**. Its exact-head Nix check must build/load
the real plugin and execute the native assertions before it can be called verified.
Existing measurements remain explicitly attached to the older JSON-wrapper control.
No new benchmark numbers are claimed.

The experimental Nix check also records fresh 2,000-delivery structural-native
and candidate samples, with compiler and CPU details, on one builder. The check
output retains those files and prints them in the coordinator logs. Existing local
measurements remain historical; CI results are unverified until the run succeeds.
