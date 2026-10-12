# Basic agent parity

Target: the original Basic group on main, not the richer Full profile.
Source baseline: main tree `1dd0ffc8ba882d46c4e6c6d5a8cee328fa8f8063`.

The group contains independently selected model, tools, skills, context and
session services, plus a runnable agent loop. Shared contracts remain reusable
outside Basic. Full must extend the same contributions through replacements or
additions. No application plugin, memory, compaction or planning is required to
run the minimal loop. Real model transport is a replaceable provider.

| Behavior | Existing proof | Remaining gate |
| --- | --- | --- |
| Prompt/model/tool/final loop and provider substitution | A0, PR #775 | richer typed contracts |
| Actual tool continuation and domain recovery | A0 | full transcript and multiple calls |
| No implicit turn limit; explicit caller limit | A0 | session policy integration |
| Typed catalog search, query-bound cursors and revision checks | original basic-tools tests | new native catalog and independent negatives |
| Lazy schema loading and authorized execution | original basic-tools | replacement and denial fixtures |
| Skill register/list/load, packaged default skills | original basic-skills | native provider and durable round-trip |
| Content-addressed context register/get/list | original basic-context | immutable revisions and durable round-trip |
| Session aggregate and restart restoration | original basic_suite | ordinary storage provider and session tests |
| Progress, pause/resume/cancel and pending provider work | original agent_loop | existing ticket/cancellation integration |
| Model usage and provider metadata | original basic-model/agent_loop | provider-neutral typed results |
| Live exposed state and reactive contributions | S2/S3 values and native authoring | G1-G4 triggered graph integration |
| Real model I/O | follow-on product requirement | local HTTP fixture, credentials external to code |
| CLI profile and independent external providers | Contemporary M1/A0 packaged tests | complete Basic composition and product gate |

The [roadmap](m1-roadmap.md) owns stage order and PR scope. G1-G4 supply
triggered execution; B1-B5 restore Basic contracts and services; F1/F2 add terminal,
JSONL and ACP frontends. P1 proves the complete product. Memory, compaction and
MCP remain later plugins. A0 supplies synchronous model/tool regression evidence;
it does not replace the final triggered agent.
