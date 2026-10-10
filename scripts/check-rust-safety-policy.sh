#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root/rust"

# The native plugin loader is the single audited in-process FFI boundary.
# Requiring its exact Cargo identity, metadata and library path prevents any
# other workspace crate or additional target from inheriting this exception.
native_loader="$repo_root/rust/crates/phenix-native-loader/src/lib.rs"
missing="$(
  cargo metadata --format-version 1 --no-deps --locked |
    jq -r '
      .packages[]
      | . as $package
      | .targets[]
      | select((.kind | index("lib")) or (.kind | index("bin")))
      | [$package.name, ($package.metadata.phenix.unsafe_boundary // "none"), .src_path]
      | @tsv
    ' |
    while IFS=
\t' read -r package boundary source; do
      if [[ "$package" == "phenix-native-loader" &&
            "$boundary" == "native-plugin-abi-v1" &&
            "$source" == "$native_loader" ]]; then
        if grep -Fq '#![deny(unsafe_op_in_unsafe_fn)]' "$source"; then
          continue
        fi
      elif grep -Fq '#![forbid(unsafe_code)]' "$source"; then
        continue
      fi
      printf '%s\n' "$source"
    done
)"

if [[ -n "$missing" ]]; then
  printf '%s\n' "workspace targets must forbid unsafe code; only the named native ABI loader may use audited unsafe with deny(unsafe_op_in_unsafe_fn):" >&2
  printf '%s\n' "$missing" >&2
  exit 1
fi
