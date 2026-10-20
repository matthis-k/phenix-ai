#!/usr/bin/env bash
# Existing CI entrypoint; the evidence harness only edits detached Git worktrees.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
exec python3 .github/scripts/basic-plugin-cache-evidence.py "$@"
