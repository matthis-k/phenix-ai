# Q3 Scale Coverage, 2026-11-23

## Phase 1: Constraints

Limit Q3 changes to scale coverage; do not change the already optimized planner
or runtime scheduler. Retain all existing native workloads, use production public
APIs, add no dependency, and avoid large test runs. Timing includes construction,
validation, selection, replay/compilation when required, assertions and teardown;
it is not planner-only timing or allocation instrumentation.

## Phase 2: Design Compare

- More reverse-lexical chains alone would miss width, shared prerequisites,
  uneven degrees and early rejection. Keep those chains as a reference instead.
- Add wide diamonds and skewed hub/fan-in/tail DAGs at 16, 64 and 128 registrations.
  Every registration adds a distinct primitive with its exact grant, exercising
  replay and compilation rather than only the unchanged-topology fast path.
  A single function varies the fan-in position; no alternate planner or graph
  generation framework is needed.
- Add 64, 256 and 1,024 independent equal-priority replacements of one component.
  All pairs overlap, but the first lexical pair must reject before selection.
  This targets early-conflict index construction, not dense successful selection.
- Reuse the native harness's two warmups and seven raw microsecond samples.
  Fixed sizes bound local work; retain the CI timeout as an operational stop,
  not a performance budget. No timing threshold is inferred from these samples.
- Follow `fbp-h7-request-work-paired.yml`: check out exact candidate and pinned
  `ab7f5e93de460e6c472f847f7b7dbc62319907e7`, copy and compare the candidate
  harness, precompile both, then alternate run order over three repetitions.
  The reference already has the planner optimizations: this is a scale reference,
  not evidence of an optimization speedup. Hash harness/manifests/lockfiles and
  record commits, compiler and CPU provenance; preserve raw output on failure.

## Phase 3: Adversarial Review

- Cardinality-only assertions can miss wrong nodes: compare exact active and
  removed sets, exact write footprints, compiled process counts and per-ID
  compiled presence. Remove one branch first: only that branch and join/tail may
  disappear. Root revocation then removes every remaining registration while
  preserving the frozen base component. Shared paths must not duplicate removals.
- Reverse admission order for dense conflicts. Assert the exact first/second IDs
  and footprint, preserve an already active sentinel, and revoke a rejected ID
  to check registry rollback. A conflict or selection-budget error of another
  kind must not count as success.
- Include all graph sources (composition, compiler, definitions, runtime and
  derive), schema, plugin and Kernel packages (including Kernel's storage
  dev-dependencies), workspace/lock/toolchain/Nix inputs in workflow paths.
  Kernel/plugin/storage are conservative consumer triggers; the native harness
  does not itself measure Kernel startup or build those consumers.
- Shared-runner noise and assertion/setup overhead remain. Equal-priority early
  rejection does not cover successful dense priority selection, arbitrary solver
  search, nested collision variants, allocation counts or unbounded scale.

## Phase 4: Implementation And Verification

The benchmark retains the original 12 cases and adds nine bounded cases. A
prepared workflow keeps pinned actions, read-only permissions, exact PR-head
checkout and always-uploaded artifacts, adding paired reference input and broader
triggers. Publishing workflow modifications is blocked by the current OAuth
token's missing `workflow` scope. That patch remains in the local worktree
`/tmp/opencode/fbp-foundation-20261123`; source-only publication uses the existing
Q3 workflow, whose benchmark-file trigger does cover this change. The source-only
PR does not claim that its existing narrow path filter has already been fixed.
Only benchmark-focused compilation/execution and formatting checks are intended;
no workspace-wide test suite is required for this coverage-only change.

Verified locally on 2026-11-23:

- `nix develop --command cargo fmt --all --check`: passed, as did the targeted
  `rustfmt --check --edition 2024 packages/phenix-graphs/benches/native_fbp.rs`.
- `nix develop --command cargo bench --locked -p phenix-graph --bench native_fbp
  --no-run --jobs 2 --target-dir /tmp/opencode/fbp-q3-scale-target-20261123`:
  passed in candidate and a separate detached baseline worktree. The isolated
  target directory and two build jobs avoid the scheduler agent's build outputs.
- The same command without `--no-run` (with `-q`) passed all 21 workloads and
  their assertions in both worktrees. One local candidate-then-baseline pair was
  run, with baseline compilation between them; the three interleaved CI pairs
  have not been executed here. No large test suite was run.
- `cmp` confirmed byte-identical harnesses. Harness SHA-256:
  `f1882b43e7b407965408e2406756a88164b5ff4c19a657de48aa0087259a35ef`.
- `nix shell --inputs-from . nixpkgs#actionlint --command actionlint
  .github/workflows/fbp-q3-composer-scale.yml`: passed. `git diff --check` passed.

Both local source HEADs were the pinned `ab7f5e93de460e6c472f847f7b7dbc62319907e7`.
Candidate changes were confined to the three owned files at measurement time;
production graph/schema/derive sources, workspace manifests, Cargo/Nix locks and
`flake.nix` had no diff against the pin. Thus these measurements validate the
same-harness reference mechanism and fixtures, not a production-code speedup.
Nix 2.34.8 supplied rustc 1.96.1 (`31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`,
LLVM 21.1.8), targeting `x86_64-unknown-linux-gnu`. Host: Linux 6.18.38,
Intel Core i7-8550U, four cores/eight logical CPUs; frequency was not pinned.

### Raw Local Samples

Every array below is the seven original, unsorted elapsed samples in microseconds
after two warmups. The complete stdout (including medians) is also retained at
`/tmp/opencode/fbp-q3-scale-candidate-20261123.txt` and
`/tmp/opencode/fbp-q3-scale-baseline-20261123.txt`. Local shared-worktree timing is
descriptive, not a performance acceptance budget. Setup, correctness assertions,
host activity and frequency variation affect these short runs; no ratios or
budgets are inferred.

Candidate:

```text
source_map_sink_1000: samples_us=[6470, 5499, 5994, 5286, 4974, 6630, 4052]
capacity1_fanout_1000_retry8: samples_us=[889, 921, 609, 593, 562, 565, 559]
stateful_split_join_1000: samples_us=[6019, 6962, 6757, 6261, 5211, 6159, 5956]
finite_feedback_1001: samples_us=[1842, 1729, 1615, 2330, 1882, 2414, 1687]
nested16_compile_inspect_x20: samples_us=[3650, 3030, 2942, 3577, 4801, 2899, 3449]
compose16_replace_revoke_x20: samples_us=[18926, 22409, 22558, 21683, 21421, 21346, 21823]
compose_revlex_chain16_register_revoke: samples_us=[98, 120, 112, 101, 99, 69, 65]
compose_revlex_chain64_register_revoke: samples_us=[590, 255, 263, 261, 254, 249, 263]
compose_revlex_chain128_register_revoke: samples_us=[746, 549, 523, 540, 531, 612, 533]
compose_revlex_add16_register_revoke: samples_us=[187, 187, 183, 196, 182, 183, 184]
compose_revlex_add64_register_revoke: samples_us=[645, 683, 661, 1034, 879, 1159, 855]
compose_revlex_add128_register_revoke: samples_us=[1568, 1475, 1463, 1544, 1538, 1935, 2045]
compose_diamond_add16_register_partial_revoke: samples_us=[411, 420, 397, 414, 410, 528, 726]
compose_diamond_add64_register_partial_revoke: samples_us=[1665, 1639, 1578, 2271, 2242, 3108, 2129]
compose_diamond_add128_register_partial_revoke: samples_us=[5306, 4337, 3614, 4795, 3483, 4535, 3538]
compose_skewed_add16_register_partial_revoke: samples_us=[527, 464, 333, 340, 497, 491, 353]
compose_skewed_add64_register_partial_revoke: samples_us=[1207, 1640, 1376, 1314, 1302, 1106, 1085]
compose_skewed_add128_register_partial_revoke: samples_us=[2115, 2377, 2280, 2232, 2109, 2516, 2149]
compose_dense_equal64_reject: samples_us=[230, 222, 217, 222, 246, 224, 277]
compose_dense_equal256_reject: samples_us=[864, 1035, 1650, 1044, 1474, 884, 1195]
compose_dense_equal1024_reject: samples_us=[6468, 6195, 4564, 6153, 5464, 5702, 6473]
```

Pinned baseline with identical harness:

```text
source_map_sink_1000: samples_us=[4535, 6456, 4468, 3508, 4201, 6161, 4273]
capacity1_fanout_1000_retry8: samples_us=[471, 478, 828, 700, 874, 789, 746]
stateful_split_join_1000: samples_us=[5155, 5220, 5339, 4534, 7257, 4679, 6047]
finite_feedback_1001: samples_us=[1664, 1536, 1444, 1490, 1421, 1410, 1776]
nested16_compile_inspect_x20: samples_us=[2657, 2477, 2535, 2753, 3080, 2890, 3906]
compose16_replace_revoke_x20: samples_us=[18280, 15478, 17352, 16629, 16429, 18504, 17022]
compose_revlex_chain16_register_revoke: samples_us=[59, 57, 55, 60, 55, 52, 53]
compose_revlex_chain64_register_revoke: samples_us=[214, 232, 227, 215, 208, 327, 367]
compose_revlex_chain128_register_revoke: samples_us=[455, 469, 422, 438, 425, 425, 427]
compose_revlex_add16_register_revoke: samples_us=[156, 150, 147, 146, 144, 144, 145]
compose_revlex_add64_register_revoke: samples_us=[536, 524, 492, 520, 542, 675, 908]
compose_revlex_add128_register_revoke: samples_us=[1720, 1842, 2215, 1876, 1570, 1839, 1959]
compose_diamond_add16_register_partial_revoke: samples_us=[313, 295, 326, 298, 310, 288, 293]
compose_diamond_add64_register_partial_revoke: samples_us=[1170, 1774, 1771, 1592, 1961, 1778, 1672]
compose_diamond_add128_register_partial_revoke: samples_us=[2770, 3448, 4413, 3915, 3123, 2515, 3162]
compose_skewed_add16_register_partial_revoke: samples_us=[405, 380, 402, 391, 367, 337, 248]
compose_skewed_add64_register_partial_revoke: samples_us=[887, 921, 918, 1048, 918, 936, 899]
compose_skewed_add128_register_partial_revoke: samples_us=[2883, 2451, 1812, 2246, 1829, 2240, 3297]
compose_dense_equal64_reject: samples_us=[190, 204, 189, 208, 290, 200, 187]
compose_dense_equal256_reject: samples_us=[700, 718, 721, 720, 714, 712, 736]
compose_dense_equal1024_reject: samples_us=[4343, 3136, 3158, 4095, 4979, 4399, 3247]
```
