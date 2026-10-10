#!/usr/bin/env bash
# Legacy script runner. Move this policy into phenix-flake-ci maintenance
# under #731's cleanup, preserving target coverage and negative CI fixtures.
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root/rust"

missing="$(
  cargo metadata --format-version 1 --no-deps --locked |
    jq -r '
      .packages[]
      | .targets[]
      | select((.kind | index("lib")) or (.kind | index("bin")))
      | .src_path
    ' |
    while IFS= read -r source; do
      if ! grep -Fq '#![forbid(unsafe_code)]' "$source"; then
        printf '%s\n' "$source"
      fi
    done
)"

if [[ -n "$missing" ]]; then
  printf '%s\n' "workspace production targets must declare #![forbid(unsafe_code)]:" >&2
  printf '%s\n' "$missing" >&2
  exit 1
fi
