# Decorated Python authoring prototype — native Phenix ABI language adapter

**Isolation:** this is a standalone experimental C ABI, **not** a patch to shipping Phenix ABI 3. It is stacked on the native-ABI / Python-adapter experiment.

## Proposed author-side syntax

~~~python
from dataclasses import dataclass
from phenix import plugin, expose, Host

@dataclass(frozen=True)
class Query:
    text: str
    limit: int

@dataclass(frozen=True)
class SearchResults:
    matches: list[str]
    calls: int

@plugin("example.search")
class Search:
    def __init__(self):
        self.calls = 0

    @expose
    def query(self, request: Query) -> SearchResults:
        self.calls += 1
        return SearchResults(request.text.split()[:request.limit], self.calls)

    @expose
    def enriched_query(self, request: Query, host: Host) -> SearchResults:
        result = host.invoke("example.index/search", {"query": request.text})
        self.calls += 1
        return SearchResults(result["matches"][:request.limit], self.calls)
~~~

The author writes Python domain logic only. One compiled **Python-hosting .so** handles module loading, scoped host injection, invocation, ownership, conversion, errors, typed export discovery and a deterministic contract manifest. The injected Host is **call-local**, becomes invalid when its native invocation returns and does not confer authorization just because Python code holds it.

## Reproduction (Linux with Python development headers)

~~~sh
cd experiments/native-abi/ergonomic-python
cc -std=c11 -Wall -Wextra -Werror -fPIC -shared \
  -o /tmp/libphenix_decorated_python.so native/python_adapter.c \
  $(python3-config --includes --embed --ldflags) -pthread
cc -std=c11 -Wall -Wextra -Werror \
  -o /tmp/phenix_annotated_python_test native/test_ergonomic.c -ldl
PYTHONPATH="$PWD/python" \
  /tmp/phenix_annotated_python_test /tmp/libphenix_decorated_python.so
~~~

A mock host callback enforces exact scope and a selected operation. This simulates the kernel boundary; the **real** Phenix Host.invoke/grant dispatcher is **not** integrated yet. The isolated transport uses JSON solely to shorten the experiment; production should use Phenix's portable Value/ValueSchema and ABI 3.

## Verified locally; also run in GitHub CI

Positive: independent instances, generated method manifest, typed dataclass inputs, structured returns and Host.invoke forwarded through a native callback with an authenticated-scope test.

Negative: wrong int/string (including bool-as-int), unknown record field, invalid JSON, unexported method, invalid return, Python exception, denied host service, host ABI mismatch, missing module/class and a retained host handle called after the invocation has ended.

Not demonstrated: actual prepared-kernel selection/grants, generational packaging, attested schema digest, interpreter isolation, concurrent interpreter callbacks, async/pending calls, streaming, CPython unload safety or hostile plugin sandboxing.

## Crate decisions revisited

| Need | Preferred candidate | Assessment |
| --- | --- | --- |
| Canonical Phenix executable ABI | Existing ABI 3 and typed ValueSchema | Keep; no second dispatcher |
| Native .so load and lease | Existing kernel loader / libloading approach | Keep generational pins |
| Python runtime hosted in a .so | PyO3 (Rust adapter) | Best next experiment; fewer handwritten unsafe CPython C-API calls than this minimal C spike. Not compiled in this experiment |
| Lua runtime hosted in a .so | mlua (fixed Lua version, optional vendored build) | Good future adapter, with serde and async support. Not compiled here |
| Rust-to-Rust trait interfaces | cglue / stabby | Optional Rust-specific interface tools. The parent experiment compiled both behind the same C ABI |
| Recursive Rust ABI layout checks | abi_stable | Useful reference; standard plugin design doesn't support unloading |
| Native rebuild watcher | hot-lib-reloader | Development-only trigger; kernel still owns promotions and draining |
| Wasm language adapter | WIT / Component Model | Future optional backend |

### Key design improvements

1. Declare one **portable contract**. Python hints and Rust macros must lower to the same canonical schemas; JSON inspection manifests here are not production contract metadata.
2. Only explicitly exposed methods are callable. The foreign object's private state and memory layout never cross the ABI.
3. Language adapters cannot decide grants, create arbitrary authorized plugin identities, or bypass the kernel. They invoke selected operations through authenticated host callbacks.
4. Host callback lifetimes are an explicit ABI guarantee. An escaped Python host handle must fail closed, not call stale native memory.
5. Reload changes to Python code need not replace the native adapter .so. Native generation updates need Phenix-controlled promotion and leases; hot-lib-reloader is only a developer watcher.
6. Preserve typed domain, denied, schema and infrastructure failures independently; do not collapse errors into arbitrary strings in production.

**Status:** proof-of-concept ergonomics; no P1 kernel behavior changed.
