from pathlib import Path
import re

implementation = Path("rust/crates/phenix-plugin-sessions/src/implementation.rs")
source = implementation.read_text()

recursive = '''fn require_open_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<SessionRecord, String> {
    let session = require_open_session(context, id)?;
    require_open(&session)?;
    Ok(session)
}'''
fixed = '''fn require_open_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<SessionRecord, String> {
    let session = read_session(context, id)?.ok_or_else(|| format!("unknown session: {id}"))?;
    require_open(&session)?;
    Ok(session)
}'''
if recursive not in source:
    raise SystemExit("recursive require_open_session fragment missing")
source = source.replace(recursive, fixed, 1)

continue_old = '''fn continue_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    kind: SessionInputKind,
    content: Bytes,
) -> Result<SessionResponse, String> {
    let session = read_session(context, id)?.ok_or_else(|| format!("unknown session: {id}"))?;'''
continue_new = '''fn continue_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    kind: SessionInputKind,
    content: Bytes,
) -> Result<SessionResponse, String> {
    let session = require_open_session(context, id)?;'''
if continue_old not in source:
    raise SystemExit("continue_session lifecycle fragment missing")
source = source.replace(continue_old, continue_new, 1)

# The original migration intentionally skipped this file while rewriting Create
# patterns, so migrate only its test module now that the production match arm is stable.
head, tests = source.split("#[cfg(test)]", 1)
tests = re.sub(
    r"SessionCommand::Create\s*\{\s*id:\s*([^,\n{}]+)\s*,?\s*\}",
    lambda match: (
        "SessionCommand::Create { session: SessionRecord::new("
        + match.group(1).strip()
        + ") }"
    ),
    tests,
)
tests = re.sub(
    r"SessionRecord\s*\{\s*id:\s*([^,\n{}]+)\s*,?\s*\}",
    lambda match: "SessionRecord::new(" + match.group(1).strip() + ")",
    tests,
)
source = head + "#[cfg(test)]" + tests

regression = r'''

    #[test]
    fn application_metadata_and_closed_lifecycle_are_durable() {
        let path = temp_db("session-metadata");
        let root = SessionId::parse("root").unwrap();
        {
            let mut kernel = kernel_with(&path);
            let created = SessionRecord::application(
                root.clone(),
                "/workspace".into(),
                Some("initial".into()),
            );
            assert!(matches!(
                invoke(
                    &mut kernel,
                    &SessionCommand::Create {
                        session: created.clone(),
                    },
                )
                .unwrap(),
                SessionResponse::Created { session } if session == created
            ));
            assert!(matches!(
                invoke(
                    &mut kernel,
                    &SessionCommand::Rename {
                        id: root.clone(),
                        title: "renamed".into(),
                    },
                )
                .unwrap(),
                SessionResponse::Updated { ref session }
                    if session.title.as_deref() == Some("renamed")
                        && session.working_directory.as_deref() == Some("/workspace")
            ));
        }

        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(
                &mut restored,
                &SessionCommand::Get { id: root.clone() },
            )
            .unwrap(),
            SessionResponse::Session { session: Some(ref session) }
                if session.title.as_deref() == Some("renamed")
                    && session.working_directory.as_deref() == Some("/workspace")
                    && session.lifecycle == SessionLifecycle::Open
        ));
        assert!(matches!(
            invoke(
                &mut restored,
                &SessionCommand::Close { id: root.clone() },
            )
            .unwrap(),
            SessionResponse::Updated { ref session }
                if session.lifecycle == SessionLifecycle::Closed
        ));
        let error = invoke(
            &mut restored,
            &SessionCommand::Continue {
                id: root.clone(),
                kind: SessionInputKind::User,
                content: b"closed".to_vec().into(),
            },
        )
        .unwrap_err();
        assert!(error.contains("session is closed"));
        drop(restored);

        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(&mut restored, &SessionCommand::Get { id: root }).unwrap(),
            SessionResponse::Session { session: Some(ref session) }
                if session.lifecycle == SessionLifecycle::Closed
        ));
        let _ = fs::remove_file(path);
    }
'''
if "fn application_metadata_and_closed_lifecycle_are_durable()" not in source:
    source = source.rstrip()
    if not source.endswith("}"):
        raise SystemExit("session implementation test module terminator missing")
    source = source[:-1] + regression + "}\n"

implementation.write_text(source)

# Restore the repository's ordinary maintenance workflow. This cleanup script is
# one-shot and removes itself so no migration machinery remains on the PR branch.
workflow = Path(".github/workflows/sync-maintenance.yml")
workflow.write_text('''name: Maintenance autofix

on:
  pull_request:

permissions:
  actions: write
  contents: write

concurrency:
  group: maintenance-autofix-${{ github.event.pull_request.number }}
  cancel-in-progress: true

jobs:
  autofix:
    if: github.event.pull_request.head.repo.full_name == github.repository
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd # v5
        with:
          ref: ${{ github.event.pull_request.head.ref }}

      - uses: cachix/install-nix-action@a49548c11d9846ad46ecc0115273879b045f001c # v31
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}
          extra_nix_config: |
            experimental-features = nix-command flakes
            accept-flake-config = true

      - name: Apply deterministic maintenance fixes
        run: |
          set -euo pipefail
          nix flake update phenix-flake-ci
          printf '%s\\n' '{"command":"fix","source":{"type":"maintenance-autofix"}}' |
            nix run --quiet .#phenix-maintenance-command-1c6e6c4c02e5 -- invoke
          nix develop --command bash -lc 'cd rust && cargo metadata --format-version 1 >/dev/null'
          git diff --check

      - name: Commit autofixes
        id: commit
        env:
          HEAD_REF: ${{ github.event.pull_request.head.ref }}
        run: |
          set -euo pipefail

          if test -z "$(git status --porcelain=v1 --untracked-files=all)"; then
            echo "changed=false" >> "$GITHUB_OUTPUT"
            exit 0
          fi

          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          git add -A
          git commit -m "chore: apply CI autofixes"
          git push origin "HEAD:$HEAD_REF"
          echo "changed=true" >> "$GITHUB_OUTPUT"

      - name: Validate corrected head
        if: steps.commit.outputs.changed == 'true'
        env:
          GH_TOKEN: ${{ github.token }}
          HEAD_REF: ${{ github.event.pull_request.head.ref }}
        run: gh workflow run ci.yml --ref "$HEAD_REF"
''')
Path(__file__).unlink()
