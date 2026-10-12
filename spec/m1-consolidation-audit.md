# Verified clean-start architecture audit

Verified against S3b `ffa206d0e66d0ba7eb1c34b7aab25fde577ef28f`, main
`1dd0ffc8ba882d46c4e6c6d5a8cee328fa8f8063`, and the pinned PR heads below.
The [consolidation contract](m1-consolidation-contract.md) owns the transition.
M1-only removal remains unfinished. This branch implements C3, the ordering
part of C4 and the stateless handler part of C5. It introduces no new executor,
ticket manager, registry or crate.

## Findings checked against source

| Audit finding | Verification and correction |
| --- | --- |
| Schema/SchemaSpec coexist with ValueSchema | Confirmed in contract `descriptor/mod.rs` and `reference.rs`. StaticSchema is a const encoding of M1, not another supported semantic schema. |
| M1 metadata becomes an artificial Descriptor | Confirmed. Artifact `normalize_metadata` encodes ValueSchema into SchemaSpec::Live. SchemaSpec::accepts decodes those schema bytes during calls. Removing this translation also removes repeated schema decoding. |
| ABI 1/2/3 and wire v1/v2 are active | Confirmed in contract `abi.rs`, `value.rs`, native loader, runtime, macros and CLI. Removal remains C1/C2. |
| Three preparation paths | Three public input forms exist, but all enter Prepared::resolve. normalize_state is a phase of that resolver. The correction is to consolidate input forms and semantic representations, not to claim three independent resolvers. |
| Property operations have separate authorization policy | Both generic dispatch and direct state callbacks already called effect_scope. Generic property dispatch then repeated authorization after entering a native-style CallGuard. The risk was repeated selection/scope work and native reentry checks applied to kernel cells, not a wholly independent authority engine. |
| Legacy and modern macro profiles | Confirmed legacy expansion alongside M1 state and definition-only expansions. The two M1 forms already encode the same ContractMetadata. Packaging differences do not justify keeping the legacy schema or ABI. |
| One Portable trait | The baseline has two traits: phenix_contract::Portable and live::Portable, plus Portable and LivePortable derives. Both trait and derive families must be addressed by C1. |
| Resolver-local dependency DFS | Confirmed. This branch moves it to dag::order. Runtime trigger composition/readiness does not exist yet; sharing with it is still a G1-G3 integration requirement. |
| Main has overlapping orchestration | GenerationTopology holds config, components, dispatch topology, resources and triggers. runtime/dispatch resolves live chains; runtime/host checks selected component imports and provider fallback; tasks.rs owns TaskRuntime. These mechanisms exist. Whether each is redundant is an architectural judgment, not proven by naming alone. |
| #726 adds 17,807 lines, removes 263, changes 52 files | Confirmed from final PR metadata at be3d7b4b258735af2a83ee40b54de185b94585f3. Those numbers describe its final review diff against the native ABI extraction base, not current main. |
| #726 duplicates native execution with WorkflowPendingTasks | It adds a workflow-specific ledger, but spawn_classified delegates work to RootExecutionHandle::spawn_native_workflow_task and retains existing TaskHandle cancellation. It duplicates bookkeeping; calling it an independent native worker executor overstates the evidence. |
| #766-#768 cleanup exists on main | False if inferred. All three are closed and unmerged. The supplied audit correctly distinguishes proposals from merged cleanup. |
| #770 removes 858 unused context lines | Confirmed and already merged into the pinned main head. The deleted implementation.rs was outside the compiled module tree. |
| #733 audits 591 files | It classifies the 591-file source snapshot plus its own inventory, 592 rows. Its body explicitly says classification coverage is not semantic file-content audit completion. |
| #728-#731 added a complete large framework | They proposed the broader framework. #728 has executable contributions and #729/#730 have partial implementations; #731 remains a migration proposal. Their completion must not be inferred from proposal scope. |
| PreparedOperation can replace synthetic contracts | Agreed, with one constraint: preparation freezes selected schemas, ceilings, grants and slot/revision identities. Root/frame authority attenuation remains a runtime decision through effect_scope. |

The audit's central duplication finding is correct. Its evidence supports
consolidation. It does not support treating every extra data representation,
packaging form or task record as an independent executor.

## Historical references

| PR | Pinned head | State |
| --- | --- | --- |
| #726 | be3d7b4b258735af2a83ee40b54de185b94585f3 | Closed, unmerged |
| #728 | 98d2682f035b8d5154a6ab6aac2e71d0cd8ba510 | Closed, unmerged |
| #729 | c2be8b6143da9b59c2e830fbe747660bfba853be | Closed, unmerged |
| #730 | c26b3d6a296671dc7aca60bb730130e17ef5f949 | Closed, unmerged |
| #731 | 7fca307b9844cfcca64f2be4b279d05cc2b0e98c | Closed, unmerged |
| #733 | dea80c8edcd63f27b1cabd8178c102b75d1c41f1 | Merged classification document |
| #765 | 00a62b04178a87a255720d3b926812fa9a4084cf | Closed, unmerged |
| #766 | fed0a9723f5c34cfb2098fc148d42de8c0c0ef16 | Closed, unmerged |
| #767 | 8f21551cd109951d27550e59a7c95c8341f24bc9 | Closed, unmerged |
| #768 | d2c1a5b8a4e3b6e124828931bda82b4040c4051d | Closed, unmerged |
| #770 | 62d9826e73269109a4c7dcebf7592557f54ec741 | Merged deletion |

## File and acceptance matrix

C1 cannot delete Descriptor while C2 still consumes it. Replace both sides in
one coordinated transition, with independently reviewable commits. Continuing
S3b pending imports on the compatibility representation would add throwaway work.

| Owner and files | Required disposition | State |
| --- | --- | --- |
| C1, contract descriptor/mod.rs and descriptor/encode.rs | Delete M0 Schema, SchemaSpec, Contract, Contribution, Descriptor and encoders. Keep identifier/operation identity helpers in the canonical module. | PENDING |
| C1, contract lib.rs, abi.rs, value.rs, scope.rs and include/phenix.h | One M1 Portable/Operation family, ABI 3 and reference-capable wire. Remove ABI 1/2 and v1/profile-selection APIs. | PENDING |
| C1, macros lib.rs/state.rs/definitions.rs and SDK lib.rs/live.rs | Delete legacy expansion, old host and old derives. Definition-only/native packaging keeps one semantic metadata encoder. | PENDING |
| C1/C2, artifact lib.rs and describe/value/metadata probes | Delete empty_descriptor and normalize_metadata. Only canonical native/definition packages verify. Preserve offline ELF, digest, path and padding rejection. | PENDING |
| C2, kernel resolve.rs and state_resolve.rs | One declarative composition and resolved M1 operation/binding map, retaining cardinality, revision, substitutions, exclusions and explicit grants. | PENDING |
| C2, kernel native.rs, runtime.rs, pending.rs, generation.rs and kernel CLI | Consume selected M1 operations directly, load ABI 3 only, keep the existing tickets/leases/generation lifecycle and common attenuation. | PENDING |
| C1/C2, examples/contracts, provider/consumer/alternate/choice/other/cycle, resource-provider/relay/scoped-client | Port provider selection, optional/exact imports, cycles and zero-effect denials to annotated M1 fixtures. | PENDING |
| C1/C2, examples/lifecycle-provider/lifecycle-consumer, pending/pending-consumer/pending-immediate, revision-a/revision-b | Port lifecycle failure/rollback, nested pending/cancel/wake and pinned-generation/retirement semantics to ABI 3. | PENDING |
| C1/C2, conformance main.rs, phenix-m01-conformance, phenix-m02-lifecycle, phenix-m02-pending, phenix-m02-imports and phenix-m03-generations | Contemporary M1 suite retains each semantic assertion before old interface tests retire. | PENDING |
| C1/C2, examples/agent-contracts/agent/agent-model/agent-alternate/agent-tool and phenix-a0-agent | Port accepted synchronous preflight to M1 selected contracts. Keep agent logic outside the kernel. | PENDING |
| C1/C2, tests descriptor.py/abi.c/abi-negative.c/product.py/composition-product.py/lifecycle-product.py/pending-product.py/generation-identity.py/shipping-product.py, flake.nix and workflow | Replace obsolete format-specific vectors; retain independent ABI, offline, packaged CLI, generation and shipping checks. Remove old active gates after migration. | PENDING |
| C3, resolve.rs/runtime.rs/state_effects.rs | Prepared native/cell targets; one dispatch scope calculation; cell owner/identity/resource checks; common atomic storage. | TESTED locally |
| C3, Root::invoke_selected | Trusted calls retain admitted root ID, narrowed authority/resource and pinned generation. Unknown selections reject before provider-dependent encoding. | TESTED locally |
| C3, authoring-provider and phenix-m1-authoring | Actual worker and FD remain live during get/set/watch. Native reentry, conflicting/forged cell requests and grant/resource/root-scope denials have no unauthorized effects. Poll/cancel join the worker; root drop clears watches. | TESTED locally |
| C4, dag.rs/resolve.rs | Resolver uses one extracted deterministic cycle/order function. Joined and disconnected projections have ordering tests. | TESTED locally; trigger/readiness integration remains |
| C5, macros state.rs and SDK live.rs | Generated empty State, optional State/Host parameters, owned request or unit request, ordinary response/Result/CallResult. | TESTED locally |
| C5, SDK stateless_authoring.rs and authoring-client | Generated ABI 3 works with stateless business handlers and a stateless native importing client. Four new compile failures retain request/response/parameter checks. | TESTED locally |
| Full M1-only acceptance | External provider/consumer, all migrated semantic gates, obsolete symbol absence, exact-head CI evidence. | NOT PROVEN |

## Measurements

| Metric | Baseline | This intervention | Final target |
| --- | --- | --- | --- |
| Semantic contract/schema models | M0 + M1 | M0 + M1 | M1 |
| Supported native ABI majors | 3 | 3 | 1 |
| Wire encodings | 2 | 2 | 1 |
| Public preparation input shapes | 3 | 3 | 1 |
| Resolver implementations | 1 | 1 | 1 |
| Dynamic authority calculators | 1 | 1 | 1 |
| Pending-ticket managers | 1 | 1 | 1 |
| Kernel agent executors | 0 | 0 | 0 |
| Production crates | 5 | 5 | Measure after C1/C2 |
| Authored State needed for stateless plugin | Yes | No | No |
| Production-crate Rust source lines | 10,665 | 10,999 | Measure after C1/C2 |
| Kernel Rust source lines | 3,905 | 4,090 | Measure after C1/C2 |
| Public contract/SDK type declarations | 81 | 83 | Measure after C1/C2 |

Source line counts include inline unit tests and exclude the conformance crate
and external examples. Public declaration counts include hidden doc-test marker
types; they count declarations, not generated exports. This intervention grows
SDK/dispatch code and test coverage. It has not yet achieved the removal-driven
size reduction proposed by the audit.

Local validation covers workspace tests, 44 compile failures, warnings-denied
Clippy, stateless generated ABI and the expanded external native authoring gate.
CI must pass on this branch's exact implementation commit before acceptance.
