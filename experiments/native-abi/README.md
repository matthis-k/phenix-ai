# Native ABI and language-adapter experiment (not the Phenix runtime)

Independent Cargo workspace to evaluate **cglue 0.3.5** and **stabby 72.1.16** inside two separately compiled `cdylib` plugins. Consumers do **not** link provider implementations. Both Rust plugins and the independent **C language-adapter .so** expose the **same stable `extern "C"` table**, with an opaque state handle, nested request struct, error status, destruction callback and exact ABI version/table-size/schema gates.

## Run on Linux

```sh
cargo build --manifest-path experiments/native-abi/Cargo.toml --workspace
cargo test --manifest-path experiments/native-abi/Cargo.toml --workspace
cargo run --manifest-path experiments/native-abi/Cargo.toml -p native-abi-consumer -- \
  experiments/native-abi/target/debug/libnative_abi_cglue_provider.so \
  experiments/native-abi/target/debug/libnative_abi_stabby_provider.so
cc -Wall -Wextra -Werror -o /tmp/phenix-abi-c-consumer \
  experiments/native-abi/consumer/c-consumer.c -ldl
/tmp/phenix-abi-c-consumer \
  experiments/native-abi/target/debug/libnative_abi_cglue_provider.so \
  experiments/native-abi/target/debug/libnative_abi_stabby_provider.so
cargo check --manifest-path experiments/native-abi/Cargo.toml -p native-abi-consumer \
  --features hot-reload-adapter
```

## What this proves / does not prove

- The provider's private compiled code executes through independently loaded `.so` images.
- Both a Rust consumer and a **real C consumer** can call the two Rust implementations and the C adapter through the same typed, version-checked function table; this is the language-adapter neutral seam.
- Different code generations can be loaded at once, and an old instance remains callable after its public `Generation` reference is dropped; old code remains loaded while the instance holds a library lease.
- The native provider may use Rust-specific `cglue` or `stabby` internally; **this experiment does not claim that their generated Rust trait vtables themselves are interoperable with C**. Their vtable representation is intentionally not the portable Phenix contract.
- No automatic execution of arbitrary untrusted plugins. No Phase P1 replacement, no production hot-swap, no authority bypass, no zero-copy guarantees beyond borrowed call-scoped inputs. The `unsafe` C ABI still needs a trusted/native loader boundary.
- The optional `hot-lib-reloader` crate is compiled as a developer watcher adapter. The actual prototype uses **staged immutable shared libraries** instead of swapping an active plugin image. Rebuild and stage a new image; perform kernel generation promotion only at a safe boundary; retain old images until all root/ticket/object leases drain. Never overwrite active code or share Rust pointers across generations.

### Language-adapter comparison

| Technology | Rust author ergonomics | Native language-neutral ABI | Compatibility | Reload strategy |
|---|---|---|---|---|
| cglue | generated trait objects/groups | C wrappers needed for general language consumers | optional layout checks | holds library context |
| stabby | checked traits/types/exported functions | C wrappers needed for stable universal surface | signature report/canaries | keep old image leased |
| Flat C table + typed schema | generated Rust/C/Zig wrappers; manual prototype now | **yes** | exact version/size/schema check | staged images with leases |
| WIT Component Model | bindings generation in several languages | cross-language **Wasm component** ABI, not native `.so` ABI | WIT package interface contract | separate component lifecycle |

**Provisional approach:** Phenix already has ABI 3, M1 typed contracts and a central authorization dispatcher. Keep them as canonical; treat cglue/stabby as **optional Rust developer-facing implementation choices**. Generalize the C-facing table only through generated SDK adapters; do not add a second kernel dispatcher or make one Rust crate's trait layout a mandatory cross-language contract. A future WIT adapter should translate the same typed Phenix operations into the component model, not replace the Rust domain types with generated boilerplate.

See upstream: https://docs.rs/cglue/0.3.5/cglue/ , https://docs.rs/stabby/72.1.16/stabby/ , https://docs.rs/hot-lib-reloader/0.8.2/hot_lib_reloader/ .
