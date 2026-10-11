# S3b contract and native authoring freeze

S3b follows S3a PR #777. It owns constant metadata lowering in
`phenix-contract/src/descriptor/static_metadata.rs`, contract/property/plugin
macros, SDK declaration symbols and generated ABI 3 glue. External API,
definition-only, field-only, provider, decorator and pending caller fixtures
prove authoring without private kernel sources or handwritten metadata/glue.
The same resolver, dispatcher, authority checker and ticket manager remain.

`#[contract(id="namespace.name@1")]` on a trait declares named methods with
zero or one portable request and Result<Response,Error>. It lowers to a public
API module with operation marker symbols and one complete StaticContract.
This is a declaration, not Rust trait objects across the ABI. Operation names
and canonical schema identities establish compatibility. No runtime registry.

`#[plugin(id="...",version="...",profile="state",define(Api),
require(Api,slot="local",optional),provide(Api,slot="public"))]` uses the
existing plugin annotation in an explicit ABI 3 module. Default require is One;
`many(min=0)` is explicit. Named slots are mandatory for repeated contracts.
`#[provide(Api::method)]` on functions implements exported operations.
`#[pending(Api::method)]`, matching poll and cancel annotations use the same
native pending lifecycle. Generated typed import symbols are local to the
require slot; undeclared imports have no generated symbol. Runtime selection
and grants independently enforce the same declaration boundary.

Definition-only module form uses profile="definitions" and define declarations,
requires no State and emits no native table. Its generated METADATA bytes can
be written by an ordinary SDK-only packaging binary without loading native
code. A native module may expose only annotated fields with no service handler.

Exposed State fields use #[get], #[set], #[watch] with an optional stable id.
The macro rewrites only those fields to Cell<T> and initializes them exactly
once under the existing init callback. Generated `properties` symbols publish
typed locations and complete property contracts. An API-only `#[property]` type
alias can publish the same typed property symbols for independent callers.
There is no direct mutable dereference or Drop commit. Plain fields remain plain.
Start after binding may inspect local fields but cannot call service imports
while initialization is active; stop owns native cleanup. Caught panics poison
ordinary invocation while cancel/poll remain available to drain physical work.

Constant metadata encodes complete definitions, explicit slots and properties
in canonical order. Native M1 supports the previous exact 16 KiB section and a
new exact 128 KiB section, with length prefix, v2 metadata and zero padding.
M0 still accepts only its frozen exact 16 KiB section. Oversize/noncanonical
metadata rejects before constructors. Larger schemas need this bounded M1
profile; no native loading or build-script manifest is used to discover types.

A native define declaration is also an explicit maximum delegation ceiling for
its declared operations. It creates no local import binding, effective grant,
provider discovery or automatic access. This allows a reusable intermediate
service to call an explicitly granted dependency while its ancestor declares
that delegated schema without selecting the dependency itself. Root and every
ancestor maximum, operation grants and resource intersections still apply.
Definition-only packages carry no runtime authority.

ABI 3 pending imports use exact commands phenix.host.pending.{begin,scoped,
poll,cancel}.v2 with the selected slot supplied through the existing ABI slot
argument. Begin carries the v2 map {operation,request:Bytes}; scoped carries the
existing ScopedRequest fields wrapped in v2 with v2 request bytes. Poll/cancel
carry {operation,ticket}. Begin/poll return status 0 with one of:
["ready",status:U64,value], ["pending",ticket:U64] for begin,
["pending"] for poll, ["draining"], ["cancelled"]. Ready status is 0 success,
1 domain, 2 kernel result. Host command failures are outer status 2. Cancel
returns success Bool. These envelopes do not repurpose native status 3/4, which
remain Null. Child tickets authenticate root, parent, operation and selected
slot provider. Existing v1 host command bytes and responses remain unchanged.

Synthetic property operations access kernel cells without invoking a native
instance and may run while that owner has a pending native call. Request
schema, selected binding, root restriction, every operation/resource grant and
property ownership checks remain identical. Native service operations retain
physical pending/reentrancy rejection. Cursor ownership inherits the nearest
native ticket. A trusted Root::invoke_selected method uses its existing root,
scope and central dispatcher for explicit control operations; it is not a new
admission or executor. Cell edits still publish through the checked protocol.

Acceptance includes independent canonical metadata, field-only native calls,
full definition-only packages, annotated reusable contracts, decorator imports,
required/optional/Many/named slots, transitive delegation denial counters,
pending children/domain recovery/cancel/abandonment, property control while
native work is pending, and compile failures for duplicate exports, invalid
fields, mismatched handlers, missing hooks and undeclared generated imports.
All twenty-two predecessor checks must continue to pass.

ABI 3 also exposes the existing read-only resource context through
phenix.host.resource.v2, empty slot, v2 Null request and success String or Null.
It returns the current admitted resource identity, never a scope token or grant.
Scope/thread authentication occurs before bytes are read. Native scoped pending
imports narrow the same inherited authority and cannot switch the active
resource. This gives ordinary storage/session implementations the reusable M0
resource context without embedding session semantics in the kernel.

## Generated SDK symbols

State modules expose `imports::<slot>::invoke::<Api::operation>` and
`count` through the declared slot. Generated slot symbols are Rust
identifiers. Each contract supplies a sealed `Member` trait for its own
operation markers. Slot helpers accept only that contract's members.
The kernel independently validates selected bindings and inherited grants.

An API-only property uses
`#[property(owner="example.provider", id="counter", get, set, watch)]`
on a nongeneric portable type alias. It lowers to a module with `CONTRACT`
and only the declared `get`, `set`, `watch` operation markers.
State fields publish the same modules under `properties::<field>`.
The stable property ID can differ from the Rust field name. Property
operations use the existing synthetic schemas and domain errors.
`set` takes `PropertyUpdate<T>` with location, expected revision and value.

State hooks use `&mut State`, `&live::Host` and the declared request.
Synchronous providers return `CallResult<Response,Error>`; pending providers
and polls return `live::PendingResult<Response,Error>`. Cancel hooks return
`Result<bool,Failure>`. Start runs after cell binding with local state only.
Stop runs once during native teardown. Every exported contract operation
requires exactly one matching handler. Field-only providers need no handlers.

## Property location discovery

`phenix.host.property.v2` accepts a v2 String containing a property operation
key and uses the selected import slot from the ABI slot argument. It returns a
typed location for that selected owner's property. Selection, the declared
operation, the active authority and the property resource must pass the same
state authorization checks before the location is returned. A location grants
no authority and cannot discover unselected providers.

Each generated property operation marker exposes `locate(host, slot)`.
This lets an independent caller consume a field-only provider without an extra
service whose only purpose is to return the field's location. Ordinary get,
set and watch retain their frozen schemas and authorization.
