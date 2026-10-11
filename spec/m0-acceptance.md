# M0.0 acceptance

Accepted for the frozen synchronous x86_64 Linux trusted-native profile on 2026-10-11. This is a milestone implementation review, not merge authorization.

Head `799d74d9c4beec0965a7044f62319c0f4838f90d` passed [full CI 38097076854](https://github.com/matthis-k/phenix-ai/actions/runs/38097076854). All four canonical Nix checks ran. Exact-head evidence includes commit, run, SDK/contract versions, artifact/descriptor digests and realized outputs. Its archive digest is `sha256:7c8aab5b16a699095fd7f3f94139ed3daeb126785890b7d8df5f25b160fbb01a`.

| RFC requirement | Review evidence |
| --- | --- |
| P0 offline descriptors | Macro-generated ELF section survives release/LTO/GNU/lld/GC and stripping; 15 final strip hashes; extraction never loads code |
| Public plugin boundary | Separate example workspace and lockfile builds from SDK distribution with private kernel sources absent |
| Two native ABI directions | CLI enters consumer; its scoped host import enters the explicitly selected provider; typed result returns |
| Selection and authority | Missing, ambiguous, absent and incompatible selections reject before loading; denied invocation leaves protected counter unchanged |
| Ownership and lifecycle | Producer-owned buffers release once; malformed tables/replies reject; recoverable startup rolls back acquired resources; stop cannot double free |
| Thread and panic rules | Wrong-thread/forged/expired scopes reject before pointer access; same-instance cycles reject; catchable panic poisons instance |
| Inspection | Parsed versioned JSON, golden semantic fields, handled/replaced errors and admission recovery |
| Minimality | One resolver, dispatcher and authority boundary; no AI runtime, hidden registration or Nix provider resolver |

The [ABI review](m0-abi-review.md) records unsafe pointer obligations and immutable-package assumptions. Native code is trusted and synchronous; process-fatal recovery is not promised. Equal code/message errors may retain nested observation context because the ABI has no origin token. This does not expand authority or change typed settlement. Advanced attribution remains outside this gate.

M0.1 may now extend this implementation. Its addendum must preserve the M0.0 wire bytes and executable path.
