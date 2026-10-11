# Minimal kernel implementation

`spec/clean-start-rfc.md` is normative. `spec/m0-contracts.md` fixes the trusted-native x86_64 Linux profile. P0 and M0.0 through M0.4 are accepted. Each milestone's acceptance document and matrix record its semantic proof. PR #771 owns current-head CI status.

Use `nix flake check -L` for acceptance. All fourteen gates and the release evidence product passed on implementation commit `f4fbe2ffd14a6ef7c71dcfc8694e5f073392b23d` in CI run 38110344943. `nix run .#phenix-m0-demo` exercises the packaged CLI. `nix build .#phenix-m0-release` materializes the complete evidence product. The public SDK and external examples build without private kernel sources.

Update the affected milestone matrix with changed files, semantic tests, evidence and remaining work. Record current-head CI in the PR body. CI evidence includes the commit, package digests and realized Nix outputs. The existing M0.0 through M0.4 matrices have no remaining implementation or acceptance tasks. Agent API and domain plugins are subsequent work; new facilities need their own contract gates.

Keep one provider resolver, one dispatcher and one effect authority boundary. Typed annotations own contributions and ABI glue. A new kernel mechanism needs an executable non-AI fixture and a reason an ordinary plugin cannot supply it. Freeze each addendum before implementing its mechanisms.

The branch starts at the original root commit. Preserve reference branches and closed PR histories. PR #771 is the sole open M0 implementation PR and stays ready for review so full conformance CI runs. Keep it unmerged.
## M1 specification and task ownership

M1 live runtime and DAG stages remain proposed. S1a and S1b portable gates are accepted; S1b evidence is bound to `51017e07067bae786131775e18686044b78eb625` in CI 38128644908. [Contract/macro semantics](spec/m1-plugin-contracts-rfc.md) and [value/reactive DAG semantics](spec/m1-values-reactive-dag-rfc.md) own the M1 requirements. [The single M1 task/evidence tracker](spec/m1-roadmap.md) owns the stage order, exact affected files, negative tests, and acceptance states. Implement each stage in its own PR based on the accepted baseline or on its explicitly named predecessor. Do not revise M0 milestone status to claim M1 functionality. Keep implementation out of the specification-only PR. Reuse the existing resolver, dispatcher, authority lineage, tickets, and generation manager; no duplicate reactive executor. Before marking a stage PROVEN, record executable evidence from its exact commit and ensure existing M0 conformance still passes.

