#!/usr/bin/env bash
set -euo pipefail

# Normalize a staged candidate before creating its commit. Never stage unrelated
# working-tree changes, including those produced by a repository-wide formatter.
if (( $# != 1 )) || [[ -z "${1//[[:space:]]/}" ]]; then
  echo "usage: scripts/prepare-commit.sh 'commit message'" >&2
  exit 2
fi
message=$1

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "prepare-commit: not inside a Git repository" >&2
  exit 1
}
cd "$repo_root"

if ! git diff --quiet --no-ext-diff; then
  echo "prepare-commit: stage or discard unstaged tracked changes first" >&2
  exit 1
fi

mapfile -d '' candidate_paths < <(git diff --cached --name-only -z)
if (( ${#candidate_paths[@]} == 0 )); then
  echo "prepare-commit: stage the candidate diff before committing" >&2
  exit 1
fi

declare -A candidate=()
for path in "${candidate_paths[@]}"; do
  candidate["$path"]=1
done

if command -v maintenance >/dev/null 2>&1; then
  maintenance fix
elif command -v nix >/dev/null 2>&1; then
  printf '%s\n' '{"command":"fix","source":{"type":"prepare-commit"}}' |
    nix run --quiet --no-write-lock-file '.#phenix-maintenance-command-1c6e6c4c02e5' -- invoke
else
  echo "prepare-commit: maintenance or Nix is required to normalize the candidate" >&2
  exit 1
fi

# Formatters may operate on the whole repository. Refuse to silently include
# unrelated edits or leave out-of-candidate changes behind after the commit.
while IFS= read -r -d '' path; do
  if [[ ! -v candidate["$path"] ]]; then
    echo "prepare-commit: normalization changed an unstaged path: $path" >&2
    echo "Review the change and stage it explicitly before retrying." >&2
    exit 1
  fi
done < <(git diff --name-only -z)

git add -A -- "${candidate_paths[@]}"
git diff --cached --check
if git diff --cached --quiet; then
  echo "prepare-commit: normalization left no staged changes" >&2
  exit 1
fi

# Keep normal Git hooks enabled. The existing pre-commit hook also runs
# normalization when a developer does not use this convenience entry point.
git commit -m "$message"
