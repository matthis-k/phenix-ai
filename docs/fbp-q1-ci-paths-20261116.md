# Q1 CI Path Coverage, 2026-11-16

## Scope

Filter baseline: `89171a8437c6fc2aebcd9029eb119d8c7505d389`.
Audited HEAD: `ca684fbbb8f3339b718a005662d95b292c447ff9`, plus the
uncommitted runtime and compose extractions present during validation.
All 14 `.github/workflows/fbp-*.yml` files at HEAD were byte-identical to
the baseline before this patch. This note records path-selection evidence,
not new runtime, benchmark, model-checking or exact-head closure evidence.

Only eight `pull_request.paths` lists change, each by adding
`packages/phenix-graphs/src/runtime/**` immediately after the retained
`packages/phenix-graphs/src/runtime/mod.rs`. Existing paths, workflow names,
job names, dispatch behavior, commands, source pins and metadata are unchanged.
No additional source hashes are needed for this path-only change; existing
candidate-head checkout and commit provenance remain intact.

## Before And After

Workflow filenames below are relative to `.github/workflows/`.

| Workflow | Before | After |
| --- | --- | --- |
| `fbp-benchmark.yml` | Runtime root only | Root plus recursive runtime children |
| `fbp-h4-scoped-sparse-benchmark.yml` | Runtime root only | Root plus recursive runtime children |
| `fbp-h4-eos-endpoint-benchmark.yml` | Runtime root only | Root plus recursive runtime children |
| `fbp-h4-targeted-wake-benchmark.yml` | Runtime root and explicit `wake.rs` | Both retained, plus recursive runtime children |
| `fbp-h4-optional-idle-benchmark.yml` | Runtime root, `request.rs`, `recovery.rs` | All retained, plus recursive runtime children |
| `fbp-h4-notified-custom-benchmark.yml` | Runtime root, `wake.rs`, `readiness.rs` | All retained, plus recursive runtime children |
| `fbp-h5-closure-benchmark.yml` | Runtime root, `closure.rs`, `readiness.rs`, `request.rs` | All retained, plus recursive runtime children |
| `fbp-q5-loom-actual-wake.yml` | Runtime root and explicit `wake.rs` | Both retained, plus recursive runtime children |
| `fbp-h4-mixed-agent-benchmark.yml` | Already `runtime/**` | Byte-identical |
| `fbp-h6-agent-allocation-benchmark.yml` | Already `runtime/**` | Byte-identical |
| `fbp-h5-benchmark.yml` | `connection.rs`, `runtime/request.rs`, `runtime/closure.rs` | Byte-identical; independent review below |
| `fbp-q5-notifier-retirement-concurrency.yml` | Production filter is `runtime/wake.rs` | Byte-identical; independent review below |
| `fbp-q5-inflight-worker-recovery.yml` | No production source paths | Byte-identical; independent review below |
| `fbp-q5-session-ownership.yml` | Kernel ownership and standalone verifier paths | Byte-identical; unrelated to these extractions |

Other original example, test, documentation, verifier and workflow-self paths
are retained verbatim. Recursive runtime coverage follows the previously
triggered monolith's admission, scheduling, execution, observation and terminal
recovery responsibilities, including future child-only changes. It deliberately
does not become an all-source glob.

No owned FBP workflow matched `network.rs` or `compose/mod.rs` at the baseline.
Their new children therefore inherit zero specialized FBP gates, not invented
network/compose triggers. The unchanged `flake-ci.yml` has an unfiltered
`pull_request` event and remains the overall CI gate for these files. This
patch does not claim that every specialized workflow covers every dependency.

## Independent Review

- Request-index: its triggered production files `connection.rs`, `request.rs`
  and `closure.rs` are unchanged against `89171a843`. The benchmark uses
  admission to fill the queue and request APIs to inspect membership, but
  `runtime/mod.rs` was not a trigger. Moving admission/scheduling out of that
  untriggered root does not justify adding `runtime/**`, network or compose
  coverage to this workflow.
- Notifier retirement: `wake.rs`, including target retirement and listener
  synchronization, is unchanged. Recovery invokes `retire_target`, scheduling
  provides public handles, and execution drives the integration workload;
  these callers were not covered by this workflow's old root filters. Its
  existing caller-coverage gap remains explicit rather than being silently
  broadened as an extraction repair.
- In-flight recovery: the workload exercises request cancellation/replacement,
  dispatch/completion, wake handles and process handoff ownership. Its baseline
  only triggered on the test, evidence document and workflow itself, not any
  production monolith. No inherited production filter can be repaired here;
  that pre-existing specialized-trigger gap remains, with overall CI unchanged.
- Actual-wake Loom: `verification/fbp-wake-loom/src/lib.rs` still directly
  includes production `runtime/wake.rs`, unchanged against the baseline. Adding
  runtime-child triggers preserves the old root's workflow-selection semantics;
  it does not model admission, scheduling, execution, observation or recovery.

## Validation Results

Read-only validation used existing Ruby 3.4.9 / Psych 5.2.2. Psych treats the
YAML key `on` as boolean `true`, handled explicitly by the checker. GitHub's
terminal `/**` is modeled as a recursive prefix; Ruby's plain `fnmatch?` for a
terminal `**` is not recursive with `FNM_PATHNAME`. An initial checker run
exposed that matcher mismatch on the nested probe; the corrected checker
passed without any further workflow change.

Counts below are specialized FBP workflows only, excluding overall Flake CI.
Each child was checked as an existing file independently, as if it were the
only changed file. Every after-set equals its old monolith's baseline gate
set, not merely the same count.

| Actual case under `packages/phenix-graphs/src/` | Baseline matches | After matches |
| --- | --- | --- |
| `runtime/mod.rs` | 10 | 10 |
| `runtime/admission.rs` | 2 | 10 |
| `runtime/scheduling.rs` | 2 | 10 |
| `runtime/execution.rs` | 2 | 10 |
| `runtime/observation.rs` | 2 | 10 |
| `runtime/recovery.rs` (existing child receiving root code) | 3 | 10 |
| `network.rs` | 0 | 0 |
| `network/authoring.rs` | 0 | 0 |
| `network/compiler.rs` | 0 | 0 |
| `network/resolution.rs` | 0 | 0 |
| `network/validation.rs` | 0 | 0 |
| `compose/mod.rs` | 0 | 0 |
| `compose/edit.rs` | 0 | 0 |
| `compose/planning.rs` | 0 | 0 |
| `compose/replay.rs` | 0 | 0 |

The runtime set is the eight amended workflows plus the unchanged mixed-agent
and H6 allocation workflows. Synthetic `runtime/future/nested/child.rs` matches
those same 10 gates. Equivalent network/compose nested probes still match zero.
Negative controls `docs/fbp-q1-ci-paths-20261116.md`,
`docs/unrelated-ci-negative-control.md`, `README.md` and
`packages/phenix-graphs/src/value.rs` each match zero before and after.

The full local checker parsed all 14 workflows, asserted that the only parsed
YAML differences were the eight expected path insertions, and checked all
baseline-tracked paths: all 84 original file/workflow matches remain covered.
The six untouched FBP workflows were also checked byte-for-byte. Workflow and
job names, commands, pins and metadata are unchanged.

A compact, read-only reproduction of the root/child and negative-control proof
uses the existing Ruby executable (no new test file or dependency is required):

```sh
/nix/store/k5zrvznvg3h4b7spivvflxsb1xxw1i0k-ruby-3.4.9/bin/ruby -e '
require "yaml"; require "open3"
files = Dir.glob(".github/workflows/fbp-*.yml").sort
old = files.to_h do |f|
  text, status = Open3.capture2("git", "show", "89171a843:#{f}")
  abort "git show failed" unless status.success?
  [f, YAML.safe_load(text)]
end
now = files.to_h { |f| [f, YAML.load_file(f)] }
gates = ->(docs, path) do
  docs.keys.select do |f|
    docs[f].fetch(true).fetch("pull_request").fetch("paths").any? do |p|
      p.end_with?("/**") ? path.start_with?(p.delete_suffix("**")) : p == path
    end
  end
end
families = {
  "runtime/mod.rs" => %w[admission scheduling execution observation recovery],
  "network.rs" => %w[authoring compiler resolution validation],
  "compose/mod.rs" => %w[edit planning replay]
}
families.each do |root, children|
  root = "packages/phenix-graphs/src/#{root}"
  expected = gates.call(old, root)
  abort "root mismatch: #{root}" unless gates.call(now, root) == expected
  dir = root.end_with?("/mod.rs") ? File.dirname(root) : root.delete_suffix(".rs")
  children.each do |child|
    path = "#{dir}/#{child}.rs"
    abort "child mismatch: #{path}" unless File.file?(path) && gates.call(now, path) == expected
    puts "#{path}: #{gates.call(old, path).size} -> #{expected.size}"
  end
  abort "nested mismatch" unless gates.call(now, "#{dir}/future/nested/child.rs") == expected
end
%w[docs/fbp-q1-ci-paths-20261116.md docs/unrelated-ci-negative-control.md README.md packages/phenix-graphs/src/value.rs].each do |path|
  abort "negative matched: #{path}" unless [old, now].all? { |d| gates.call(d, path).empty? }
end
puts "PASS: all roots, 12 children, three recursive probes, four negatives"
'
```

`nix eval --offline --raw .#checks.x86_64-linux.workflow.drvPath` and
`nix build --offline --no-link .#checks.x86_64-linux.workflow` both passed.
This evaluates/checks the existing workflow-sync derivation; it does not
regenerate or modify `flake-ci.yml`. `git diff --check` passed as well.

No application tests, benchmarks or Loom exploration were run for this
path-only patch. No GitHub workflow execution is claimed: overall CI will
validate the eventual head containing these edits. GitHub event delivery and
its changed-file limits are outside this local matcher proof. Production
sources, tests, sourcegraph/ledger files and `flake-ci.yml` were not edited;
no commit or push was performed.
