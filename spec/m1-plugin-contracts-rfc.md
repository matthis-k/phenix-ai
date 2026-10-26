# RFC: Declarative plugin contracts and Rust annotation API

Status: historical authoring proposal, partly implemented. Current interface/ABI details come from source and [M1 contract authoring](m1-contract-authoring-contract.md). Future graph contribution semantics come from [v2.4](graph-architecture-v2.4.md), not this older RFC.
Date: 2026-10-11
Depends on: [clean-start RFC](clean-start-rfc.md)
Related: [M1 value/ref and reactive DAG RFC](m1-values-reactive-dag-rfc.md)
Roadmap: [m1-roadmap.md](m1-roadmap.md)

## 1. Design rule

A plugin is an independently identified owner of zero or more `define`, `require`, and `provide` declarations. These are the only core declaration roles. Properties, callbacks, subscriptions, state initialization, and hooks compile to ordinary typed operations and immutable graph contributions. The kernel never recognizes tools, skills, models, or agent-loop categories.

An ordinary service-only plugin remains valid without a graph or value-cell subsystem. Definition-only plugins need no executable artifact and contribute contract descriptors without `dlopen`.

## 2. Canonical contract and import semantics

- `define(Contract)` publishes a unique, versioned interface contract with named operations, schema and behavioral guarantees.
- `require(Contract)` declares an import **slot**, not a concrete provider. The slot specifies contract revision range, cardinality, optionality, and a local symbolic name. Selection is resolved independently for each consuming plugin and slot.
- `provide(Contract)` declares an export binding with operation implementations. One plugin may require and provide the same interface through different import/export slots. Self-import is rejected unless explicitly backed by a separately selected compatible provider and a valid dependency graph.
- Providers cannot grant themselves authority by declaring exports; deployment policy and root/caller grants remain authoritative.
- Missing required providers, ambiguous single imports, incompatible version/schema, identical public export IDs from two owners without policy resolution, and required dependency cycles reject during preparation.

For `require(Contract)` the default cardinality is exactly one. `optional` allows zero or one. `many(min = n)` resolves a deterministically ordered finite provider set, may be empty when `n = 0`, and requires an explicit ordering key or canonical sort. A plugin can name distinct imports of the same contract, e.g. `require(Model, as = "fast")` and `require(Model, as = "strong")`. The kernel does not choose heuristically.

Contract definitions have stable major revisions and canonical schema descriptors. Rust `TypeId` and a shared trait spelling alone do not establish compatibility between independently compiled plugins.

## 3. Authoring API (target syntax)

The following is a specification example, not compiled M0 code.

```rust
#[phenix::contract(id = "agent.skills@1")]
pub trait Skills {
    fn list(&self) -> Result<Vec<Skill>, SkillError>;
    fn load(&self, name: String) -> Result<Skill, SkillError>;
}

#[phenix::plugin(
    id = "phenix.agent.contracts",
    version = "1.0.0",
    define(Skills),
    define(Model),
    define(Agent),
)]
pub struct AgentContracts;

#[phenix::plugin(
    id = "phenix.agent.loop",
    version = "1.0.0",
    require(Model),
    require(Skills, optional),
    provide(Agent),
)]
pub struct AgentLoop {
    session: Session,
}
```

`#[contract]` generates typed operation marker definitions and schemas that the outer `#[plugin]` macro can reference through normal Rust name resolution. It does not require the compiler to inspect other crates' source. `#[plugin]` emits exactly one canonical descriptor and ABI glue as applicable. No handwritten manifest or side registry is allowed.

## 4. Field annotations and generated property contracts

```rust
#[phenix::plugin(id = "phenix.skills.basic", version = "1.0.0")]
pub struct SkillsPlugin {
    #[get(id = "skills")]
    #[watch]
    skills: Vec<Skill>,

    #[get]
    #[set]
    enabled: bool,

    cache: HashMap<String, Skill>,
}
```

The enclosing procedural macro parses helper field attributes and generates a plugin-owned versioned property contract with a stable operation identity for each exposed read, watch, and write. A public field rename requires an unchanged explicit `id` or a contract revision. No annotation means no contract export. `#[watch]` is not implied by `#[get]`, and `#[set]` does not imply `#[watch]`.

A public property must not expose raw memory, local Rust references, or unrestricted mutable access. The macro may rewrite the authored `Vec<Skill>` field into a typed SDK-managed value handle after parsing its initial type, so normal untracked `skills.push()` cannot silently alter published state. Generated operations use the kernel's authorized value protocol. Initial state is declared before host binding; no host calls occur from `Default`.

Field annotations generate a **plugin-owned property contract**. They do not redefine independent reusable `Skills` or `Model` interfaces. A shared contract lives in a small definition/API crate; an implementation may explicitly `provide` it. The public SDK supplies generated operation symbols to independent callers.

## 5. Operations and graph annotations

The SDK must accept a module form for related state, handlers, and hook contributions:

```rust
#[phenix::plugin(id = "phenix.search", version = "1.0.0")]
mod search {
    #[state]
    pub struct State { /* private state */ }

    #[provide(Search::query)]
    fn query(state: &mut State, request: SearchRequest) -> SearchResult {
        /* ... */
    }

    #[on_change(skills_api::SKILLS, id = "search.reindex")]
    fn reindex(ctx: &HookContext, change: Change<Vec<Skill>>) -> HookResult {
        /* ... */
    }
}
```

Hooks generate immutable graph-contribution descriptors, never runtime registration callbacks. An `after = [other_plugin::HOOK]` target must resolve to a published authorized hook ID and an operation binding. Failed resolution or unauthorized insertion rejects the candidate generation before activation. `after` alone orders work; it does not trigger a hook.

At runtime, the kernel calls the generated handlers through the existing scoped dispatcher. Operation errors preserve declared domain errors versus kernel failures. Cancellation and physical leases remain with the host.

## 6. Macro and ABI implementation rules

- Rust field attributes require an enclosing attribute macro or derive helper; there is no standalone field proc macro invocation.
- A macro on a struct cannot inspect external `impl` blocks. Authors may use annotated functions inside the plugin module or explicit exported impl annotations.
- Descriptor content and order must be deterministic. Generation must not depend on timestamps, filesystem enumeration, or dynamic registration callbacks.
- Compile-time trait coherence validates Rust implementations; the runtime independently verifies ABI version, portable schemas, import grants and ownership.
- A separately compiled contract-only package must be inspectable without loading native code. Definition-only metadata may use a canonical package record without a shared-library artifact; M0's native artifact package format remains unchanged until the new metadata profile is accepted.
- `Value<T>`, `ValueRef<T>`, and value encodings are specified by [the value RFC](m1-values-reactive-dag-rfc.md); this document owns contract/declaration authoring rules.

## 7. Acceptance

An external fixture must define a contract, publish its metadata without executable code, implement it in one native plugin, and import it in another. The same kernel must support a non-agent fixture. Tests check required/optional/multi/named imports; decorator imports; structural incompatibility; duplicate and forged declarations; and compile errors on unsupported macro placement. A field-only plugin with `#[get]` must be callable without a manually written getter or contract manifest. The generated ABI must expose no native Rust object or pointer to an independent plugin.
