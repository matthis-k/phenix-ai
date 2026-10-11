# Minimal kernel implementation

`spec/clean-start-rfc.md` is normative. `spec/m0-contracts.md` fixes the implemented Linux profile. M0.0 is accepted in `spec/m0-acceptance.md`. Continue M0.1–M0.4 in dependency order. Freeze each addendum before implementing its mechanisms. Keep one active PR and update its milestone matrix.

Use `nix flake check -L` for acceptance. `nix run .#phenix-m0-demo` exercises the packaged CLI. The public SDK and external examples build without private kernel sources.

Update `spec/m0-matrix.md` with the files, semantic acceptance test, evidence and remaining work for each change. Record exact-head CI in the PR body. CI evidence includes the commit, package digests and realized Nix outputs.

Keep one provider resolver, one dispatcher and one effect authority boundary. Typed annotations own contributions and ABI glue. A new kernel mechanism needs an executable non-AI fixture and a reason an ordinary plugin cannot supply it.

The branch starts at the original root commit. Preserve reference branches and closed PR histories. PR #771 is the sole open implementation PR and stays ready for review so full conformance CI runs. Keep it unmerged.
