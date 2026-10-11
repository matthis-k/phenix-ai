# Minimal kernel implementation

`spec/clean-start-rfc.md` is normative. `spec/m0-contracts.md` fixes the trusted-native x86_64 Linux profile. P0 and M0.0 through M0.4 are accepted. Each milestone's acceptance document and matrix record its semantic proof. PR #771 owns current-head CI status.

Use `nix flake check -L` for acceptance. All fourteen gates and the release evidence product passed on implementation commit `f4fbe2ffd14a6ef7c71dcfc8694e5f073392b23d` in CI run 38110344943. `nix run .#phenix-m0-demo` exercises the packaged CLI. `nix build .#phenix-m0-release` materializes the complete evidence product. The public SDK and external examples build without private kernel sources.

Update the affected milestone matrix with changed files, semantic tests, evidence and remaining work. Record current-head CI in the PR body. CI evidence includes the commit, package digests and realized Nix outputs. The existing M0.0 through M0.4 matrices have no remaining implementation or acceptance tasks. Agent API and domain plugins are subsequent work; new facilities need their own contract gates.

Keep one provider resolver, one dispatcher and one effect authority boundary. Typed annotations own contributions and ABI glue. A new kernel mechanism needs an executable non-AI fixture and a reason an ordinary plugin cannot supply it. Freeze each addendum before implementing its mechanisms.

The branch starts at the original root commit. Preserve reference branches and closed PR histories. PR #771 is the sole open implementation PR and stays ready for review so full conformance CI runs. Keep it unmerged.
