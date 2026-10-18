#!/usr/bin/env bash
# Confirm content-addressed Nix derivations respect plugin source boundaries.
# This test evaluates, but does not build, the plugin and composition outputs.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
tool_source=crates/phenix-plugin-basic-tools/src/lib.rs
agent_source=crates/phenix-plugin-basic-agent/src/lib.rs
test -f "$tool_source"
test -f "$agent_source"
trap 'git checkout -- "$tool_source" "$agent_source"' EXIT

attrs=(
  phenix-plugin-basic-tools
  phenix-plugin-basic-agent
  phenix-plugin-model-scripted
  phenix-plugin-event-journal
  phenix-plugin-basic-session
  phenix-plugin-basic-memory
  phenix-basic-composition
)

nix_drv() {
  nix eval --raw ".#packages.x86_64-linux.${1}.drvPath"
}

declare -A initial after_tool after_agent
for attr in "${attrs[@]}"; do
  initial["$attr"]="$(nix_drv "$attr")"
done

# Append a harmless comment to just the Tool provider's tracked Rust source.
printf '\n// Nix cache isolation probe: Tool provider only.\n' >> "$tool_source"
for attr in "${attrs[@]}"; do
  after_tool["$attr"]="$(nix_drv "$attr")"
done
for attr in "${attrs[@]}"; do
  if [[ "$attr" == phenix-plugin-basic-tools || "$attr" == phenix-basic-composition ]]; then
    test "${initial[$attr]}" != "${after_tool[$attr]}" || {
      echo "::error::Changing basic-tools did not invalidate $attr"
      exit 1
    }
  else
    test "${initial[$attr]}" == "${after_tool[$attr]}" || {
      echo "::error::Changing basic-tools also invalidated unrelated $attr"
      exit 1
    }
  fi
done

# A second independent edit must leave Tools' derivation unchanged and only
# update the selected Agent and its generated composition configuration.
printf '\n// Nix cache isolation probe: Basic agent only.\n' >> "$agent_source"
for attr in "${attrs[@]}"; do
  after_agent["$attr"]="$(nix_drv "$attr")"
done
for attr in "${attrs[@]}"; do
  if [[ "$attr" == phenix-plugin-basic-agent || "$attr" == phenix-basic-composition ]]; then
    test "${after_tool[$attr]}" != "${after_agent[$attr]}" || {
      echo "::error::Changing basic-agent did not invalidate $attr"
      exit 1
    }
  else
    test "${after_tool[$attr]}" == "${after_agent[$attr]}" || {
      echo "::error::Changing basic-agent also invalidated unrelated $attr"
      exit 1
    }
  fi
done

echo "Basic Tools and Agent each invalidate only themselves and the composition."
