#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT

make_fixture() {
  local name=$1
  local directory="$fixture_root/$name"
  mkdir -p "$directory/bin"
  git -C "$directory" init --quiet
  git -C "$directory" config user.name "Phenix fixture"
  git -C "$directory" config user.email "fixture@localhost"
  printf 'original\n' > "$directory/selected.txt"
  printf 'unchanged\n' > "$directory/unrelated.txt"
  git -C "$directory" add selected.txt unrelated.txt
  git -C "$directory" commit --quiet -m baseline

  # Stub only the normalizer. Exercise the real git index and git commit.
  cat > "$directory/bin/maintenance" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == fix ]]
printf 'normalized\n' > selected.txt
if [[ "${FIX_UNRELATED:-}" == 1 ]]; then
  printf 'unexpected\n' > unrelated.txt
fi
STUB
  chmod +x "$directory/bin/maintenance"
  printf '%s\n' "$directory"
}

pass="$(make_fixture success)"
printf 'candidate\n' > "$pass/selected.txt"
git -C "$pass" add selected.txt
(
  cd "$pass"
  PATH="$pass/bin:$PATH" bash "$repo_root/scripts/prepare-commit.sh" "normalize the candidate"
)
[[ "$(git -C "$pass" show HEAD:selected.txt)" == normalized ]]
[[ "$(git -C "$pass" rev-list --count HEAD)" == 2 ]]
[[ -z "$(git -C "$pass" status --porcelain)" ]]

unrelated="$(make_fixture unrelated)"
printf 'candidate\n' > "$unrelated/selected.txt"
git -C "$unrelated" add selected.txt
if (
  cd "$unrelated"
  FIX_UNRELATED=1 PATH="$unrelated/bin:$PATH" bash "$repo_root/scripts/prepare-commit.sh" "must not commit"
); then
  echo "prepare-commit fixture: unrelated formatter change was accepted" >&2
  exit 1
fi
[[ "$(git -C "$unrelated" rev-list --count HEAD)" == 1 ]]
[[ "$(git -C "$unrelated" show HEAD:unrelated.txt)" == unchanged ]]

unstaged="$(make_fixture unstaged)"
printf 'candidate\n' > "$unstaged/selected.txt"
git -C "$unstaged" add selected.txt
printf 'private edit\n' > "$unstaged/unrelated.txt"
if (
  cd "$unstaged"
  PATH="$unstaged/bin:$PATH" bash "$repo_root/scripts/prepare-commit.sh" "must not commit"
); then
  echo "prepare-commit fixture: unstaged changes were accepted" >&2
  exit 1
fi
[[ "$(git -C "$unstaged" rev-list --count HEAD)" == 1 ]]

empty="$(make_fixture empty)"
if (
  cd "$empty"
  PATH="$empty/bin:$PATH" bash "$repo_root/scripts/prepare-commit.sh" "must not commit"
); then
  echo "prepare-commit fixture: empty candidate was accepted" >&2
  exit 1
fi
[[ "$(git -C "$empty" rev-list --count HEAD)" == 1 ]]

echo "prepare-commit fixtures passed"
