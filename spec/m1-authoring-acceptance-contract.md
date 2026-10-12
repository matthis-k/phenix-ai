# S3b acceptance completion

This addendum follows accepted consolidation head
`367420e33bbaaa55b6d680394b9cc99a3c6f43e6` in PR #780. It completes
property discovery and pending-import security before G1. The existing
preparation, dispatcher, authority calculation, cells and tickets own execution.

## Property discovery

The frozen `phenix.host.property.v2` command takes the selected import slot and
a canonical String operation key. It returns a Ref for that selected cell.
The host authenticates the call before reading bytes, checks the exact selected
slot and prepared cell target, then applies existing operation and resource
authority checks. Native operations, absent cells, missing selections and denied
scopes return typed kernel failures. Discovery allocates no cell or subscription
and performs no native callback. Every generated property operation exposes
`locate(host, slot)` returning its typed ValueRef. Independent field-only callers
need no provider service to obtain that reference.

## Pending security

ABI 3 pending commands retain the accepted M1-only reply encoding: statuses
0/1/2 carry success/domain/kernel outcomes, 3 carries a begin ticket or Null
while polling, 4 carries cancellation Null, and 5 carries draining Null or a
host-command Failure. This supersedes the earlier proposed list envelopes in
`m1-contract-authoring-contract.md`; there is one active encoding.

Independent native fixtures call the real ABI callback with wrong slots,
operations, ticket IDs and provider selections. Both poll and cancel reject
before child polling, cancellation or settlement. A valid handle remains usable
after rejection. Malformed begins acquire no native worker or descriptor.
Existing root, parent, caller, resource and authority authentication remains.

## Gate

Run all contemporary M1 checks and release evidence. Record exact source commit
and CI artifact before accepting S3b. Retain cancellation, lifecycle rollback,
generation retirement, dependency ordering, attenuation and A0 fixture gates.
The shared DAG ordering code has distinct lifecycle and future reactive meanings;
reactive integration remains G1-G4.
