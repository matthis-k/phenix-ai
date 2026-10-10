# Embedded descriptor decision

Production uses `.phenix` in Linux x86_64 ELF shared libraries. The exported `PHENIX_DESCRIPTOR` symbol retains the macro-generated static through linker garbage collection. The kernel extracts metadata by parsing file bytes. There is no descriptor callback or sidecar fallback.

The first Nix proof passed with pinned nixpkgs `e7a3ca8092b61ff85b6a45bf863ea2b2d6a661b3`, Rust 1.96.1, GNU binutils 2.46 and LLVM lld 21.1.8. Five independent derivations produced the same descriptor digest: default thin LTO, fat LTO, LTO off, lld with `--gc-sections`, and another clean thin-LTO build. Each artifact also passed extraction after `--strip-debug`, `--strip-unneeded` and `--strip-all`. A removed section and malformed ELF were rejected.

`checks.x86_64-linux.m0-descriptor` repeats this proof for the final declarations. Its `evidence.json` records package paths, exact final artifact and descriptor digests, and linker settings. Later schema changes require rerunning that check. The first proof established section feasibility before kernel resolution and invocation were implemented.

The supported target is x86_64 Linux. A different architecture, linker family or packaging transform needs its own retention proof before becoming supported. The decoder rejects missing or duplicate sections, a section of the wrong size, an out-of-bounds length, nonzero padding, malformed canonical values, invalid contributions and unsupported ABI requirements.

A package record binds descriptor identity separately from artifact identity. Nix extracts after all ELF fixups and hashes every final `.so` byte. The kernel compares package metadata with the embedded bytes and rechecks exact artifact bytes before loading. Native constructors remain trusted process code.
