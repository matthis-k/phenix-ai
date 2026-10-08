# First-party development commands are regular phenix-flake-ci maintenance leaves.
# No separate Nix package or shell script path is needed.
let
  candidate = normalize: ''
    set -euo pipefail
    # Normalize a staged candidate before creating its commit. Never stage unrelated
    # working-tree changes, including those produced by a repository-wide formatter.
    if (( $# != 1 )) || [[ -z "''${1//[[:space:]]/}" ]]; then
      echo "usage: maintenance commit 'commit message'" >&2
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

    mapfile -d "" candidate_paths < <(git diff --cached --name-only -z)
    if (( ''${#candidate_paths[@]} == 0 )); then
      echo "prepare-commit: stage the candidate diff before committing" >&2
      exit 1
    fi

    declare -A candidate=()
    for path in "''${candidate_paths[@]}"; do
      candidate["$path"]=1
    done

    # Preserve pre-existing untracked paths. New files created by normalization
    # must be explicitly staged in a new candidate, not left behind silently.
    declare -A untracked_before=()
    while IFS= read -r -d "" path; do
      untracked_before["$path"]=1
    done < <(git ls-files --others --exclude-standard -z)

    ${normalize}
    # Formatters may operate on the whole repository. Refuse to silently include
    # unrelated edits or leave out-of-candidate changes behind after the commit.
    while IFS= read -r -d "" path; do
      if [[ ! -v candidate["$path"] ]]; then
        echo "prepare-commit: normalization changed an unstaged path: $path" >&2
        echo "Review the change and stage it explicitly before retrying." >&2
        exit 1
      fi
    done < <(git diff --name-only -z)

    while IFS= read -r -d "" path; do
      if [[ ! -v untracked_before["$path"] ]]; then
        echo "prepare-commit: normalization created an unstaged file: $path" >&2
        echo "Review the file and stage it explicitly before retrying." >&2
        exit 1
      fi
    done < <(git ls-files --others --exclude-standard -z)

    git add -A -- "''${candidate_paths[@]}"
    git diff --cached --check
    if git diff --cached --quiet; then
      echo "prepare-commit: normalization left no staged changes" >&2
      exit 1
    fi

    # Keep normal Git hooks enabled. The existing pre-commit hook also runs
    # normalization when a developer does not use this convenience entry point.
    git commit -m "$message"
  '';
in
{
  command = {
    description = "Normalize a staged candidate and create one commit";
    dependencies = [ [ "fix" ] ];
    runtimeInputs = pkgs: [ pkgs.git ];
    exec = candidate ''"$0" fix'';
  };

  test = ''
        set -euo pipefail
        # Exercise the same command implementation, substituting only the fix
        # boundary. The real Git index, working tree, and commit are used.
        fixture_root="$(mktemp -d)"
        trap 'rm -rf "$fixture_root"' EXIT
        cat > "$fixture_root/candidate" <<'CANDIDATE'
    #!/usr/bin/env bash
    ${candidate "maintenance fix"}
    CANDIDATE
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
    if [[ "''${FIX_UNRELATED:-}" == 1 ]]; then
      printf 'unexpected\n' > unrelated.txt
    fi
    if [[ "''${FIX_NEW_UNTRACKED:-}" == 1 ]]; then
      printf 'unexpected\n' > generated.txt
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
          PATH="$pass/bin:$PATH" bash "$fixture_root/candidate" "normalize the candidate"
        )
        [[ "$(git -C "$pass" show HEAD:selected.txt)" == normalized ]]
        [[ "$(git -C "$pass" rev-list --count HEAD)" == 2 ]]
        [[ -z "$(git -C "$pass" status --porcelain --untracked-files=no)" ]]
    
        unrelated="$(make_fixture unrelated)"
        printf 'candidate\n' > "$unrelated/selected.txt"
        git -C "$unrelated" add selected.txt
        if (
          cd "$unrelated"
          FIX_UNRELATED=1 PATH="$unrelated/bin:$PATH" bash "$fixture_root/candidate" "must not commit"
        ); then
          echo "prepare-commit fixture: unrelated formatter change was accepted" >&2
          exit 1
        fi
        [[ "$(git -C "$unrelated" rev-list --count HEAD)" == 1 ]]
        [[ "$(git -C "$unrelated" show HEAD:unrelated.txt)" == unchanged ]]
    
        generated="$(make_fixture generated)"
        printf 'candidate\n' > "$generated/selected.txt"
        git -C "$generated" add selected.txt
        if (
          cd "$generated"
          FIX_NEW_UNTRACKED=1 PATH="$generated/bin:$PATH" bash "$fixture_root/candidate" "must not commit"
        ); then
          echo "prepare-commit fixture: generated unstaged file was accepted" >&2
          exit 1
        fi
        [[ "$(git -C "$generated" rev-list --count HEAD)" == 1 ]]
        [[ -f "$generated/generated.txt" ]]
    
        unstaged="$(make_fixture unstaged)"
        printf 'candidate\n' > "$unstaged/selected.txt"
        git -C "$unstaged" add selected.txt
        printf 'private edit\n' > "$unstaged/unrelated.txt"
        if (
          cd "$unstaged"
          PATH="$unstaged/bin:$PATH" bash "$fixture_root/candidate" "must not commit"
        ); then
          echo "prepare-commit fixture: unstaged changes were accepted" >&2
          exit 1
        fi
        [[ "$(git -C "$unstaged" rev-list --count HEAD)" == 1 ]]
    
        empty="$(make_fixture empty)"
        if (
          cd "$empty"
          PATH="$empty/bin:$PATH" bash "$fixture_root/candidate" "must not commit"
        ); then
          echo "prepare-commit fixture: empty candidate was accepted" >&2
          exit 1
        fi
        [[ "$(git -C "$empty" rev-list --count HEAD)" == 1 ]]
    
        echo "prepare-commit fixtures passed"
  '';
}
