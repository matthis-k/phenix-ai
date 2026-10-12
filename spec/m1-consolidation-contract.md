# M1 consolidation contract

The 12 October architecture audit authorizes M1 as the sole eventual product.
This addendum records the coordinated transition from S3b head
`ffa206d0e66d0ba7eb1c34b7aab25fde577ef28f`. Historical M0 acceptance remains
evidence, not a requirement to retain its formats. Breaking removal replaces
interfaces and ports their semantic tests in the same owning change.

## Prepared effects

Preparation fixes each exported operation's execution target as native or cell.
Dispatch verifies root, selection, request schema and inherited authority before
effects. A cell target additionally requires its property resource. The selected
cell's owner, property and live identity must match before storage access.
Cell execution uses the existing atomic storage operations and never enters the
owner's native instance. Physical native work blocks native reentry, not access
to independent kernel-owned cells. Generic cell results receive the same
declared response/error validation as native results.

A trusted Root may invoke another explicitly selected operation in its pinned
generation through `invoke_selected`. It retains the root's narrowed authority,
resource and ID. The existing dispatcher checks the operation grant. This lets
control code inspect cells while that root owns physical pending native work.
The plugin SDK receives neither Root nor this control-plane method.

Direct state callbacks retain live-scope authentication and property selection.
They calculate authority through `effect_scope`, as service calls do. Every
batch member must pass authorization before a batch reaches atomic commit.
Initialization creates declared cells under the existing single-use owner scope.

## Stateless Rust handlers

An executable plugin may omit `State`; the macro then generates empty default
state and the existing ABI 3 lifecycle. An authored State keeps its own Default
implementation and field semantics. A synchronous handler accepts at most one
owned request, an optional State reference and an optional shared Host reference.
Parameter order is irrelevant. A handler without a request must implement an
operation with a unit request. Native pending handlers retain PendingResult and
matching poll/cancel declarations. A synchronous handler returns its response,
`Result<Response, Error>` or `CallResult<Response, Error>`.

M1 define/require/provide declarations select the executable macro form without
requiring `profile = "state"`. Omitted version means `0.1.0`. Definition-only
packaging keeps its explicit packaging choice until C1 unifies macro selection.
Minimal business logic therefore needs no authored State, ABI table or metadata.
Handler coverage and contract request/response/error types remain compiler checks.

## Dependency ordering

One small ordering function validates the resolver's required dependency
projection and returns deterministic cycle paths. Lifecycle edges and future
reactive edges remain separate semantic projections. This change supplies shared
ordering code; it claims neither reactive readiness nor graph execution.

## Remaining transition

C1 and C2 remove M0 schemas/descriptors, v1 wire, ABI 1/2, legacy macro/SDK
authoring and the old deployment/composition input shapes. They also replace
synthetic M1 descriptor translation with a canonical prepared operation schema.
They must migrate offline verification, authority, pending/cancellation,
generation and retirement conformance onto M1. No new compatibility adapter,
registry or execution manager is authorized by this addendum.
