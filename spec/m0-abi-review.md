# M0.0 ABI and trust review

This review covers the frozen synchronous Linux x86_64 profile in RFC §§14–16. It adds no later lifecycle, generation or isolation mechanism.

| Boundary | Ownership and rejection rule | Executable evidence |
| --- | --- | --- |
| Offline preparation | Parse final ELF and packaged descriptors, compare exact digests, then resolve all imports and grants before loading | Constructor witness remains absent during positive and negative preparation |
| Loaded table | Read the fixed header before the remaining table; reject unsupported version/features, short table and null entries | Foreign C fixtures with valid embedded SDK metadata and invalid native tables |
| Foreign reply | Represent the release pointer as nullable; reject null before payload access; valid callbacks run once even after malformed payload or unknown status | Null-release C fixture and counted-release contract tests |
| Failed create | Release its reply; stop any instance actually returned; drop instances already started in this attempted deployment | Foreign failing create records allocation, reply release, stop and stop-reply release; consumer records one matching stop |
| Normal stop | The native owner takes its instance before calling stop; repeated host stop rejects without producer reentry; final drop does not stop it twice | Native-owner stop test, plus actual rollback witness |
| Invocation | One scoped dispatcher checks thread, caller, selected import, schema and effective grants before provider entry | Native pair, protected counter, scope and cycle cases |
| Panic | Generated boundaries catch unwind; failed invocation poisons state; process-fatal events remain fatal | Actual provider panic and subsequent poisoned invocation |
| Observation | The host stores failure, phase and context together; handled errors remain observable and replacement errors receive their current context | Native handled-denial/replacement/admission recovery fixtures, CLI golden and subsequent-success checks |

The C layout is unchanged: `Reply` is 32 bytes with its pointer at offset 8; the optional Rust release function occupies the same nullable C pointer slot. There is one buffer format and one release callback per transferred buffer. `Reply::consume` consumes a non-cloneable owner. Raw C callbacks require unique ownership and valid producer addresses; replaying an already consumed raw pointer violates that unsafe ABI contract. The host owner rejects repeated stop rather than calling a freed instance.

## Trusted source assumption

The CLI authenticates control-plane input by running as the deployment operator. It requires canonical package paths within the Nix store. The library preparation API names its stronger caller obligation explicitly: `verify_trusted_read_only`. A caller using mutable files does not obtain TOCTOU safety from a digest recheck. Tests mutate scratch packages to exercise rejection; those paths are test inputs, not supported mutable deployments.

Final artifact hashes are checked for all packages before the first load, then checked immediately before each load. Metadata never runs constructors. Dynamic loading does run trusted constructors before ABI negotiation. Nix fixes artifact bytes and assembles their dependent-library closure; plugins and native dependencies remain trusted code. Hashes establish consistency, not authorship. A privileged writer, hostile native pointer or malicious process memory modification falls outside this profile.

Recoverable startup failure creates no root and releases acquired host-owned instances and replies. The rollback test checks event counts and absence of invocation, rather than accepting a successful compile as evidence. Invalid foreign callbacks cannot be repaired by this ABI: a null release returns `AbiIncompatible`, but the host cannot free an unknown producer allocation. Abort, signals and invalid non-null pointers can compromise the process. No isolation or process-fatal cleanup guarantee is claimed.

Golden inspection fields in `tests/inspection-golden.json` freeze JSON types and semantic values. Additive fields are allowed within `phenix.inspect.v1`; incompatible field meanings or types require a new format version. Artifact hashes remain recorded in realized conformance output rather than copied into a handwritten fixture.

Descriptor decoding checks U64 version and feature tags before JSON conversion can erase integer signedness. The offline fixture substitutes an I64 ABI major while updating both embedded metadata and package hashes; it rejects without running the constructor. Unit vectors cover all four numeric descriptor fields.
