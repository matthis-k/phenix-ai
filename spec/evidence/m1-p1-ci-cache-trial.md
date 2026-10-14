# Post-P1 remote Nix cache trial

Status: first trusted coordinator cache saved; subsequent restoration not yet verified.
Date: 2026-10-14.

## Baseline evidence

- P1 accepted on `main` at `68f46f6379ff129b5bb9f2bad27229269aec0110`; original branch retained as `oldmain`.
- Four-worker, two-slot release trial [38565268482](https://github.com/matthis-k/phenix-ai/actions/runs/38565268482) passed its exact-head plan, Nix shipping and independent evidence gate with no project Nix store cache.
- Trial [38567944923](https://github.com/matthis-k/phenix-ai/actions/runs/38567944923), revision `c8d031f52368b6641e1d0512c321d8441229363b`, passed.
- Cache-enabled trial [38567971821](https://github.com/matthis-k/phenix-ai/actions/runs/38567971821), revision `2f0d3f932bdbfeb91eea3c76e58579b0177f3542`, also passed. It used pinned `phenix-flake-ci` `a1480620950508215bdda4a5404f046becf65cf8`.
- The cache-enabled coordinator reported no matching full key or restore prefix. At 13:08:25 UTC it saved 3,667,233,232 bytes of Nix store under the `remote-phenix-m1-x86_64-linux-8956978d008aa1b2bbc2-` namespace.
- The two trial runs started 13 seconds apart on different commits. They do not establish warm-cache speedup.

## Follow-up acceptance

The next trusted push must show an explicit Nix store cache restore **after** that save, then pass all evaluated checks and the final exact-head shipping gate. Compare cache keys and source lockfiles. Different Git commits invalidate a claim of a strictly same-head cold/warm result even when only documentation changes.

For a controlled same-head comparison, run cold then warm sequentially, use a shared verified source revision and check inventory, and record restore/save log excerpts, build wall time, complete-workflow wall time and runner minutes. If either check set fails, report failure rather than a speedup.

The generated-workflow verifier regression has a tests-first dependency in [#880](https://github.com/matthis-k/phenix-ai/pull/880) and minimal fix in [#881](https://github.com/matthis-k/phenix-ai/pull/881). Promotion must keep the independent official ACP SDK interoperability workflow and existing full release gates until the trusted and credential-free replacements prove equivalent coverage.
