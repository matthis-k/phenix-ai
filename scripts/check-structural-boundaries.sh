#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

pattern='std::any::Any|std::any::\{[^}]*Any|any::Any|dyn[[:space:]]+Any'
json_field_pattern='pub[[:space:]]+[[:alnum:]_]+[[:space:]]*:[^,]*serde_json::Value'
raw_invoke_pattern='\.invoke_value[[:space:]]*\('
legacy_plugin_authoring_pattern='phenix_plugin![[:space:]]*'
hidden_static_discovery_pattern='inventory::|linkme::|link_section'
host_process_spawn_pattern='std::process::Command|process::Command|Command::new[[:space:]]*\('
workspace_host_fs_pattern='std::fs::|[[:space:]]fs::(read|write|metadata|read_dir|create_dir|create_dir_all|remove_|rename|copy)'

check_fixture() {
  local pattern="$1"
  local input="$2"
  local message="$3"
  if [[ "$input" =~ $pattern ]]; then
    printf '%s\n' "$input"
    printf '%s\n' "$message" >&2
    return 1
  fi
}

if [[ -n "${PHENIX_STRUCTURAL_BOUNDARY_FIXTURE:-}" ]]; then
  check_fixture \
    "$pattern" \
    "$PHENIX_STRUCTURAL_BOUNDARY_FIXTURE" \
    "public Phenix structural boundaries must not depend on std::any::Any"
  exit
fi

if [[ -n "${PHENIX_JSON_BOUNDARY_FIXTURE:-}" ]]; then
  check_fixture \
    "$json_field_pattern" \
    "$PHENIX_JSON_BOUNDARY_FIXTURE" \
    "neutral Phenix contracts must use PhenixValue instead of serde_json::Value"
  exit
fi

if [[ -n "${PHENIX_RAW_INVOKE_FIXTURE:-}" ]]; then
  check_fixture \
    "$raw_invoke_pattern" \
    "$PHENIX_RAW_INVOKE_FIXTURE" \
    "statically known plugin consumers must use typed invocation helpers"
  exit
fi

if [[ -n "${PHENIX_LEGACY_PLUGIN_AUTHORING_FIXTURE:-}" ]]; then
  check_fixture \
    "$legacy_plugin_authoring_pattern" \
    "$PHENIX_LEGACY_PLUGIN_AUTHORING_FIXTURE" \
    "Rust plugin authoring must use the canonical attribute API"
  exit
fi

if [[ -n "${PHENIX_HIDDEN_STATIC_DISCOVERY_FIXTURE:-}" ]]; then
  check_fixture \
    "$hidden_static_discovery_pattern" \
    "$PHENIX_HIDDEN_STATIC_DISCOVERY_FIXTURE" \
    "static plugin wiring must not use hidden global discovery"
  exit
fi

targets=(
  rust/crates/phenix-core/src/contract.rs
  rust/crates/phenix-core/src/plugin.rs
  rust/crates/phenix-core/src/plugin_context.rs
  rust/crates/phenix-provider-sdk/src
  rust/crates/phenix-sdk/src
)

for crate in rust/crates/phenix-{adapter,plugin}-*; do
  [[ -d "$crate/src" ]] || continue
  targets+=("$crate/src")
done

if git grep -n -E "$pattern" -- "${targets[@]}"; then
  printf '%s\n' "public Phenix structural boundaries must not depend on std::any::Any" >&2
  exit 1
fi

json_targets=(
  rust/crates/phenix-core/src/agent.rs
  rust/crates/phenix-core/src/configuration.rs
  rust/crates/phenix-core/src/manifest.rs
  rust/crates/phenix-domain/src/lib.rs
  rust/crates/phenix-domain/src/debug.rs
  rust/crates/phenix-sdk/src/contracts/frontend.rs
  rust/crates/phenix-sdk/src/contracts/models.rs
)

if git grep -n -E "$json_field_pattern" -- "${json_targets[@]}"; then
  printf '%s\n' "neutral Phenix contracts must use PhenixValue instead of serde_json::Value" >&2
  exit 1
fi

for crate in rust/crates/phenix-{adapter,plugin}-*; do
  [[ -d "$crate/src" ]] || continue
  [[ "$crate" == "rust/crates/phenix-plugin-debug" ]] && continue
  if git grep -n -E "$raw_invoke_pattern" -- "$crate/src"; then
    printf '%s\n' "statically known plugin consumers must use typed invocation helpers" >&2
    exit 1
  fi
done

if git grep -n -E "$legacy_plugin_authoring_pattern" -- 'rust/crates/**/*.rs'; then
  printf '%s\n' "Rust plugin authoring must use the canonical attribute API" >&2
  exit 1
fi

for crate in rust/crates/phenix-{adapter,plugin}-*; do
  [[ -d "$crate/src" ]] || continue
  if git grep -n -E "$hidden_static_discovery_pattern" -- "$crate/src"; then
    printf '%s\n' "static plugin wiring must not use hidden global discovery" >&2
    exit 1
  fi
done

if git grep -n -E "$hidden_static_discovery_pattern" -- rust/crates/phenix-sdk-macros/src; then
  printf '%s\n' "static plugin wiring must not use hidden global discovery" >&2
  exit 1
fi

for crate in rust/crates/phenix-{adapter,plugin}-*; do
  [[ -d "$crate/src" ]] || continue
  [[ "$crate" == "rust/crates/phenix-plugin-environment-local" ]] && continue
  while IFS= read -r -d '' file; do
    if awk '/#\[cfg\(test\)\]/{exit} {print}' "$file" | grep -n -E "$host_process_spawn_pattern"; then
      printf '%s\n' "runtime plugin host process execution must route through Environment" >&2
      printf '%s\n' "offending file: $file" >&2
      exit 1
    fi
  done < <(find "$crate/src" -type f -name '*.rs' -print0)
done

workspace_impl="rust/crates/phenix-plugin-workspace/src/implementation.rs"
if awk '/#\[cfg\(test\)\]/{exit} {print}' "$workspace_impl" | grep -n -E "$workspace_host_fs_pattern"; then
  printf '%s\n' "Workspace host filesystem access must route through Environment" >&2
  exit 1
fi

fixture='pub type ErasedBoundary = Box<dyn std::any::Any>;'
if PHENIX_STRUCTURAL_BOUNDARY_FIXTURE="$fixture" \
  bash "$repo_root/scripts/check-structural-boundaries.sh" >/dev/null 2>&1; then
  printf '%s\n' "structural boundary fixture unexpectedly accepted std::any::Any" >&2
  exit 1
fi

json_fixture='pub struct NeutralBoundary { pub payload: serde_json::Value }'
if PHENIX_JSON_BOUNDARY_FIXTURE="$json_fixture" \
  bash "$repo_root/scripts/check-structural-boundaries.sh" >/dev/null 2>&1; then
  printf '%s\n' "structural boundary fixture unexpectedly accepted serde_json::Value" >&2
  exit 1
fi

raw_invoke_fixture='context.sdk.models.invoke_value(&request);'
if PHENIX_RAW_INVOKE_FIXTURE="$raw_invoke_fixture" \
  bash "$repo_root/scripts/check-structural-boundaries.sh" >/dev/null 2>&1; then
  printf '%s\n' "structural boundary fixture unexpectedly accepted raw static invocation" >&2
  exit 1
fi

legacy_plugin_authoring_fixture='phenix_plugin! { "fixture.legacy"; }'
if PHENIX_LEGACY_PLUGIN_AUTHORING_FIXTURE="$legacy_plugin_authoring_fixture" \
  bash "$repo_root/scripts/check-structural-boundaries.sh" >/dev/null 2>&1; then
  printf '%s\n' "structural boundary fixture unexpectedly accepted legacy static plugin authoring" >&2
  exit 1
fi

hidden_static_discovery_fixture='inventory::submit! { Plugin }'
if PHENIX_HIDDEN_STATIC_DISCOVERY_FIXTURE="$hidden_static_discovery_fixture" \
  bash "$repo_root/scripts/check-structural-boundaries.sh" >/dev/null 2>&1; then
  printf '%s\n' "structural boundary fixture unexpectedly accepted hidden static plugin discovery" >&2
  exit 1
fi

exit 0
