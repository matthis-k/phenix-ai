from pathlib import Path

# application_projection is now a normal public Harness module. Do not compile the
# source file again under the integration-test crate root.
path = Path("rust/crates/phenix-harness/tests/application_projection.rs")
path.write_text('''use phenix_harness::application::{
    APPLICATION_EVENT_CAPACITY, APPLICATION_INVOCATION_CAPACITY, CLIENT_CAPABILITY_CAPACITY,
};

#[test]
fn fixed_application_queue_capacities_match_the_runtime_contract() {
    assert_eq!(APPLICATION_INVOCATION_CAPACITY, 64);
    assert_eq!(CLIENT_CAPABILITY_CAPACITY, 64);
    assert_eq!(APPLICATION_EVENT_CAPACITY, 256);
}
''')

# Keep the structural product fixture on the canonical session wire shape.
path = Path("rust/crates/phenix-harness/tests/supported_product_journeys.rs")
source = path.read_text()
for session_id in ("root", "child"):
    old = f'json!({{"operation": "create", "id": "{session_id}"}})'
    new = (
        'json!({"operation": "create", "session": {'
        f'"id": "{session_id}", "working_directory": null, "title": null, "lifecycle": "open"'
        '}})'
    )
    if old not in source:
        raise SystemExit(f"missing stale session fixture for {session_id}")
    source = source.replace(old, new, 1)
path.write_text(source)

# session-tree deliberately projects the mutation service through a private DTO.
# Keep that DTO and call payload structurally aligned with the canonical contract.
path = Path("rust/crates/phenix-plugin-session-tree/src/implementation.rs")
source = path.read_text()
old = '''enum SessionMutationRequest {
    PrepareCreate { id: SessionId },
}'''
new = '''enum SessionMutationRequest {
    PrepareCreate { session: SessionRecord },
}'''
if old not in source:
    raise SystemExit("stale session-tree mutation request shape missing")
source = source.replace(old, new, 1)
old = '''            &SessionMutationRequest::PrepareCreate {
                id: session_id.clone(),
            },'''
new = '''            &SessionMutationRequest::PrepareCreate {
                session: SessionRecord::new(session_id.clone()),
            },'''
if old not in source:
    raise SystemExit("stale session-tree PrepareCreate call missing")
source = source.replace(old, new, 1)
path.write_text(source)

# Remove this one-shot patcher and restore the ordinary maintenance workflow.
workflow = Path(".github/workflows/sync-maintenance.yml")
source = workflow.read_text()
source = source.replace("          python3 .phenix/work/pr-504-fix-worker-tests.py\n", "")
source = source.replace(
    '          git commit -m "fix(application): align worker migration tests"\n',
    '          git commit -m "chore: apply CI autofixes"\n',
)
workflow.write_text(source)
Path(__file__).unlink()
