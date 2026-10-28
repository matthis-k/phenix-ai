# Annotation-Authored Python Plugin

`greeter.py` is the entire Python plugin: one dataclass, explicit plugin and
contract identities, and one decorated typed operation. There is no per-operation
registration, Rust mapping, schema encoder, field conversion, or graph declaration
in the example. `tools/phenix_python` is the public SDK build frontend.

## Build And Verify

From the repository root, with Rust, a C linker and Python development libraries
(tested with Rust 1.96.1 and CPython 3.13):

```sh
export PYTHONPATH="$PWD/tools"
python3 -m unittest phenix_python.test_frontend
python3 examples/python-trivial/verify.py /tmp/opencode/python-trivial-artifacts
python3 examples/python-trivial/verify.py /tmp/opencode/python-trivial-artifacts --release --strip
```

Exact Nix environment used for the review regressions on this machine (the PATH
assignment happens **inside** the Nix shell and preserves its Python/linker tools):

```sh
nix shell --inputs-from . nixpkgs#python313 nixpkgs#gcc nixpkgs#binutils -c bash -c '
  export PATH="/tmp/opencode/rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin:$PATH"
  export RUSTFLAGS="-C link-arg=-fuse-ld=bfd"
  export PYTHONPATH="$PWD/tools"
  python3 -m unittest phenix_python.test_frontend &&
  cargo fmt --manifest-path examples/python-trivial/runner/Cargo.toml --check &&
  python3 examples/python-trivial/verify.py /tmp/opencode/phenix-python-trivial/generated/review-verification &&
  python3 examples/python-trivial/verify.py /tmp/opencode/phenix-python-trivial/generated/review-verification --release --strip
'
```

To generate a consumer crate directly:

```sh
PYTHONPATH=tools python3 -m phenix_python examples/python-trivial/greeter.py \
  /tmp/opencode/python-greeter --sdk "$PWD/crates/phenix-sdk"
cargo build --manifest-path /tmp/opencode/python-greeter/Cargo.toml
```

The verifier copies **only public SDK, contract, macros and ABI headers** into a
scratch SDK tree and builds the generated crate against that tree. Neither the
frontend nor generated plugin depends on the kernel or artifact crate. The
separate verification runner does, because it exercises the actual product.

Generated `contract.rs` uses `Portable` and `#[contract]`; `src/lib.rs` uses
`#[plugin]`, instance-owned State and generated PyO3 conversion. These existing
macros own canonical ValueSchema, ContractMetadata and ABI-3 entry points.
`build.rs` reads the generated operation's static definition and calls #1186's
`GraphContributionV2::native_section()`. A retained link-time `.phenix.graph`
static embeds a Source -> Invoke graph. No objcopy, Python schema encoding,
registry, executor or kernel modification is involved.

`verify.py` packages the independently built `.so` with `phenix-describe` and
runs `runner` against it. Assertions cover exact outputs on repeated real ABI-3
dispatcher calls, two simultaneously live native instances with independent
Python state, a real V2 Source -> Invoke execution, malformed Python field type
rejection (`PluginFailed`) and malformed kernel request rejection
(`SchemaRejected`). Composition authority and source/operation attestations are
explicitly supplied by the trusted harness, never by the Python author.

The `regressions` runner additionally dispatches two independently generated
artifacts concurrently. The internal `tools/phenix_python/testdata/concurrent.py` fixture uses top-level filesystem rendezvous and
GIL-releasing sleeps, then checks separate global tokens and instance counters.
It also replaces its own `sys.modules` entry and verifies cleanup leaves that
replacement untouched. Runtime module names use Python UUIDs, not image-local
counters; each module executes in its own dictionary. Both artifacts compile
dataclasses named `Operation` and `ValueSchema`, which must not collide with
build-tool imports. The internal `tools/phenix_python/testdata/scalar.py` fixture provides repeated `str -> str` dispatch.
The same assertions run against debug and release/stripped plugin artifacts;
the kernel verification binaries themselves run in debug mode.

`rust-equivalent` independently compiles the same `Text`, contract operation and
stateful greeting in ordinary Rust SDK syntax. The harness asserts exact equality
of both artifacts' offline canonical contract definitions. It demonstrates the
equivalent portable contract, not an additional Python mapping layer. Its native artifact
does not add a graph; the generated Python artifact is the graph execution target.

## Bounded Surface

- Exactly one plugin class and one synchronous exposed method `(self, request)`.
- Request and response: `str`, or non-inherited flat dataclasses whose fields are
  all `str`. No implicit `int` width, nested container, union, reference or Host
  conversion. Unsupported annotations, missing annotations, extra parameters,
  async and generator methods fail at generation, rather than silently widening.
- Use direct `from phenix_python import plugin, expose` decorators. Authoring
  imports/decorators are stripped from the embedded runtime source; no Python
  frontend installation is needed at runtime. The author module executes at
  build time and again for each runtime instance. Inputs must be trusted.
- Python errors and malformed returns become kernel `PluginFailed`; this slice
  has no Python-authored domain errors (the generated contract uses `String` as
  its error type). Response dataclass and string field types are checked exactly.
- Instance state is the generated native State's module/object, not an executor
  registry or interpreter global. The CPython interpreter and imported third-party
  modules remain process-global; this is not a sandbox or arbitrary module-global
  isolation guarantee. No async, Host bridge, hot reload or cancellation support.
- Initialize the shared process interpreter with one serialized first call before
  concurrent calls across independent images. Independent PyO3 copies do not
  serialize cold CPython startup; racing those first calls can abort the process.
  The concurrency regression first invokes the scalar artifact, then concurrently
  initializes the other two artifacts' modules. It proves concurrent module and
  instance isolation, **not** concurrent cold interpreter startup.
- Linux shared-library deployment with the build-selected CPython library is the
  current target. Generated glue promotes that same libpython to RTLD_GLOBAL for
  CPython extension imports under the kernel's RTLD_LOCAL plugin loading. It is
  retained for process lifetime; CPython is not finalized per plugin. Phenix ABI-3
  is unrelated to Python's similarly named stable `abi3` extension ABI.
- The generated crate is a build product; regenerate it when annotations change.
  This is a bounded authoring implementation, not full Graph-v2/P1 parity.

This base contains no `maintenance.nix`, `.stitch.json` or devenv configuration.
The focused commands above are not a claim that canonical `nix flake check -L`
or `devenv test` has passed.

## Parent Integration Evidence

PR #1187 integrates #1186 head `b39624392ddcee7ee23238c99e081a1fe270341c`
by normal merge, including #1184 readiness audit fixes, #1185 native failure
draining and the restored direct Rust panic regression. The merge was
conflict-free; the latest normative design and bounded Python limits above
remain unchanged.

Local post-merge checks on 28 October 2026 passed both verifier profiles above,
all eight frontend tests, runner formatting, and unit/integration/doc tests for
`phenix-contract`, `phenix-artifact`, `phenix-sdk`, `phenix-macros` and
`phenix-kernel` (71 kernel tests). This includes the generated native-authoring
synchronous/pending failure regression. Exact integration SHA and remote CI
status are recorded in #1187; these local results are not full-DAG CI acceptance.
