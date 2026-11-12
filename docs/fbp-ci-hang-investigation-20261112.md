# FBP CI hang diagnosis — 12 November 2026

## Evidence and problem

Parent FBP exact-head run 43149315879 and scoped repair run 43156807756
show shard-00 succeeding while shard-01 and shard-02 remain active without
terminal evidence for more than an hour. This is not a passing or completed
M7 gate. The repeated behavior on the unchanged parent weakens but does not
eliminate the hypothesis that the scoped driver fixes are responsible.

The current flake invokes cargo test in Nix derivation buildPhase without any
per-suite deadline. A never-ending test can occupy GitHub runners indefinitely
and conceal the responsible test from completed-job logs.

## Diagnostic change, not a runtime feature

Temporarily run each existing Cargo test target through coreutils timeout:
480-second TERM grace and a 15-second SIGKILL grace. Use `-- --nocapture`
so any test progress and errors are retained in the resulting completed job
logs. Preserve the existing package/feature/target selection and full test
coverage; do not suppress or ignore failures. Exit 124 is a failed test gate,
not a passing or accepted limitation.

## Acceptance and follow-up

Read exact-head job outputs and identify the last running Rust test or build
phase. Fix the underlying cause with a deterministic regression, then rerun
unfiltered shard checks. Review whether to retain the bounded CI guard or
restore the prior invocation once root cause and normal execution time are
measured. Do not relabel a timeout as a valid test result.

This does not change the FBP graph, scheduler, plugin policy, or stored data.
