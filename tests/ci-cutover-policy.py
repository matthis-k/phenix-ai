"""Fail-closed policy gate for the staged post-P1 flake-CI cutover.

This is a static trust and release-boundary check, not a replacement for
same-head Nix shipping, remote-store and official ACP interoperability tests.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXPECTED_SOURCE_PIN = 'c4467d7dc233ada85ea49823561ee8be57089591'
# Reviewed immutable phenix-flake-ci revision; PRs use its credential-free staged DAG.
EXPECTED_CANDIDATE_PIN = '25029578a370c0b1f57ae88881b6018ed1ac08df'


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def verify(generated, renderer, backend, check_config, flake, canonical, matrix, dag, acp, verifier):
    # Only main's trusted push may automatically receive remote credentials.
    require('  pull_request:' not in generated, 'untrusted remote PR event')
    require('  id-token: write' in generated, 'missing OIDC permission')
    require('on:\n  push:\n    branches:\n      - "main"\n  workflow_dispatch:' in generated,
            'privileged remote push admission must be main-only')
    require('      worker_count: 4' in generated, 'four-builder setting missing')
    require('      slots_per_worker: 2' in generated, 'two-slot setting missing')
    require('    secrets:\n      TS_OAUTH_CLIENT_ID:' in generated,
            'remote credential delegation missing')
    require("TS_AUDIENCE: ${{ format('api.tailscale.com/{0}', secrets.TS_OAUTH_CLIENT_ID || vars.TS_OAUTH_CLIENT_ID) }}" in generated,
            'OIDC audience must be derived from client id')
    require('TS_AUTHKEY:' not in generated, 'mixed auth-key and OIDC credential modes')

    refs = re.findall(r'flake-nix-builders\.yml@([0-9a-f]{40})', generated)
    library_refs = re.findall(r'library_ref: "([0-9a-f]{40})"', generated)
    require(refs == [EXPECTED_SOURCE_PIN] and library_refs == refs,
            'reusable workflow and checked out library must use the same reviewed immutable pin')
    require('libraryRef = "' + EXPECTED_SOURCE_PIN + '"' in renderer,
            'Nix generator pin differs from workflow')
    require('pullRequest = false;' in renderer, 'generated policy must forbid remote PRs')
    require('backend = "nix-remote";' in backend, 'selected backend drifted')
    require('workers = 4;' in backend and 'slotsPerWorker = 2;' in backend,
            'explicit measured worker/slot configuration missing')
    require('credentials = "oidc";' in backend, 'remote auth policy changed')
    require('ci/flake-check-config.nix' in generated and 'ci/flake-check-config.nix' in renderer,
            'planner config path differs between generator and workflow')

    # The Nix flake, not YAML, remains the complete check inventory.
    for check in ('p1-release-preflight', 'm1-shipping', 'basic-host-smoke',
                  'basic-pending-http-product', 'm1-imports', 'm1-public-sdk'):
        require(check in flake, 'required real Nix release check missing: ' + check)
    require('ci.mkFlakeCheckConfig' in check_config, 'not using generic flake check planner')
    require('ci.rustCheckStrategy' in check_config, 'existing Rust strategy absent')
    require('shardCount = 5;' in check_config, 'existing experimental matrix was altered')
    candidate_refs = re.findall(r'flake-dag-staged\.yml@([0-9a-f]{40})', matrix)
    candidate_libraries = re.findall(r'(?m)^      library_ref: ([0-9a-f]{40})$', matrix)
    require(candidate_refs == [EXPECTED_CANDIDATE_PIN] and candidate_libraries == candidate_refs,
            'unprivileged PR DAG must use the explicit reviewed candidate workflow/runtime pin')
    require(matrix.split('\njobs:\n', 1)[1:] == [
        '  candidate:\n'
        '    uses: matthis-k/phenix-flake-ci/.github/workflows/flake-dag-staged.yml@'
        + EXPECTED_CANDIDATE_PIN + '\n'
        '    with:\n'
        '      config: ci/flake-check-config.nix\n'
        '      library_ref: ' + EXPECTED_CANDIDATE_PIN + '\n'],
        'PR candidate must only delegate the complete inventory without secrets or publishers')

    # The slow local DAG remains manual-only, with no credential delegation.
    fallback_refs = re.findall(r'flake-local-dag\.yml@([0-9a-f]{40})', dag)
    fallback_libraries = re.findall(r'(?m)^      library_ref: ([0-9a-f]{40})$', dag)
    require(fallback_refs == [EXPECTED_CANDIDATE_PIN]
            and fallback_libraries == fallback_refs,
            'manual single-store DAG must retain the reviewed immutable library pin')
    require('  pull_request:' not in dag and '  workflow_dispatch:' in dag,
            'manual single-store DAG must not reintroduce an automatic PR gate')
    require('  id-token: write' not in dag and '    secrets:' not in dag,
            'manual fallback must not gain remote-builder credentials')
    require(matrix.split('\npermissions:\n', 1)[1:]
            and matrix.split('\npermissions:\n', 1)[1].split('\n\n', 1)[0]
            == '  contents: read', 'PR candidate permissions must remain read-only')
    # Preserve independent authoritative release checks until same-head remote
    # full-inventory, security and cold/warm checks are proven.
    require('branches: [main,' not in canonical and '  workflow_dispatch:' in canonical,
            'legacy canonical build still duplicates main')
    require('branches: [main,' not in matrix and '  pull_request:' in matrix,
             'PR full-inventory DAG missing or duplicates main')
    require('branches: [main,' not in dag and '  workflow_dispatch:' in dag,
            'legacy DAG still duplicates main')
    require('P1 official ACP SDK interop' in acp and 'branches: [main]' in acp,
            'independent ACP SDK main check removed')
    require('nix-community/cache-nix-action@7df957e333c1e5da7721f60227dbba6d06080569' in acp
            and 'remote-phenix-m1-x86_64-linux-' in acp
            and 'gc-max-store-size-linux: "6G"' in acp
            and 'save: false' in acp and 'save: true' not in acp,
            'independent ACP must restore only the trusted-main cache read-only')
    verify_refs = re.findall(r'(?m)^          ref: ([0-9a-f]{40})$', verifier)
    require(verify_refs == [EXPECTED_SOURCE_PIN],
            'verifier checkout does not match generated remote backend pin')
    require('      - ci/m1-p1-cache-benchmark' in verifier,
            'cache benchmark branch bypasses the verifier')


def files():
    def read(path):
        return (ROOT / path).read_text()
    return [
        read('.github/workflows/phenix-flake-ci-generated.yml'),
        read('ci/render-flake-workflow.nix'),
        read('ci/flake-backend.nix'),
        read('ci/flake-check-config.nix'),
        read('flake.nix'),
        read('.github/workflows/minimal-kernel.yml'),
        read('.github/workflows/minimal-kernel-flake.yml'),
        read('.github/workflows/minimal-kernel-dag.yml'),
        read('.github/workflows/p1-acp-sdk-interop.yml'),
        read('.github/workflows/verify-generated-flake-ci.yml'),
    ]


def self_test():
    originals = files()
    # Synthetic valid policy is permitted only for testing this validator;
    # the real repository policy is checked by verify(*files()) separately.
    good = originals.copy()
    good[2] = good[2].replace('credentials = "oidc";',
                              'credentials = "oidc";\n    workers = 4;\n    slotsPerWorker = 2;')
    good[9] = good[9].replace('          ref: 25da62028d5d72d2e5ae3aca9d1571c425eb0d09',
                               '          ref: ' + EXPECTED_SOURCE_PIN)
    good[9] = good[9].replace('      - ci/m1-p1-ci-cutover',
                               '      - ci/m1-p1-ci-cutover\n      - ci/m1-p1-cache-benchmark')
    good[5] = good[5].replace('branches: [main, rewrite/minimal-kernel]', 'branches: [rewrite/minimal-kernel]')
    good[5] = good[5].replace('  pull_request:\n', '')
    good[6] = good[6].replace('branches: [main, ci/m1-flake-generic-checks]',
                              'branches: [ci/m1-flake-generic-checks]')
    good[7] = good[7].replace('branches: [main, ci/m1-dag-staged]',
                              'branches: [ci/m1-dag-staged]')
    good[7] = good[7].replace('  pull_request:\n', '')
    verify(*good)
    mutations = [
        (0, '      - "main"', '      - "ci/unauthorized"'),
        (0, '      - "main"', '      - "main"\n      - "ci/temporary"'),
        (0, 'on:\n  push:', 'on:\n  pull_request:\n  push:'),
        (0, '      worker_count: 4', '      worker_count: 8'),
        (0, 'library_ref: "' + EXPECTED_SOURCE_PIN + '"',
             'library_ref: "' + 'f' * 40 + '"'),
        (2, 'workers = 4;', 'workers = 8;'),
        (4, 'p1-release-preflight', 'not-an-actual-release-check'),
        (9, '          ref: ' + EXPECTED_SOURCE_PIN,
             '          ref: ' + 'f' * 40),
        (9, '      - ci/m1-p1-cache-benchmark',
             '      - ci/unknown-benchmark'),
        (8, 'save: false', 'save: true'),
        (8, 'remote-phenix-m1-x86_64-linux-', 'phenix-m1-x86_64-linux-'),
        (6, 'flake-dag-staged.yml@' + EXPECTED_CANDIDATE_PIN,
             'flake-dag-staged.yml@' + 'f' * 40),
        (6, 'library_ref: ' + EXPECTED_CANDIDATE_PIN,
             'library_ref: ' + 'f' * 40),
        (6, EXPECTED_CANDIDATE_PIN, EXPECTED_SOURCE_PIN),
        (6, EXPECTED_CANDIDATE_PIN, 'f' * 40),
        (6, 'flake-dag-staged.yml@' + EXPECTED_CANDIDATE_PIN, 'flake-dag-staged.yml@main'),
        (6, 'library_ref: ' + EXPECTED_CANDIDATE_PIN, 'library_ref: main'),
        (6, 'flake-dag-staged.yml@', 'flake-local-dag.yml@'),
        (6, 'flake-dag-staged.yml@', 'flake-checks.yml@'),
        (6, 'matthis-k/phenix-flake-ci/', 'untrusted/phenix-flake-ci/'),
        (7, 'flake-local-dag.yml@', 'flake-dag-staged.yml@'),
        (7, '  workflow_dispatch:', '  pull_request:\n  workflow_dispatch:'),
        (6, '  contents: read', '  contents: read\n  id-token: write'),
        (6, '    with:\n', '    permissions:\n      id-token: write\n    with:\n'),
        (6, '    with:\n', '    secrets: inherit\n    with:\n'),
        (6, '    with:\n', '    secrets:\n      TOKEN: ${{ secrets.TOKEN }}\n    with:\n'),
        (6, '      config: ci/flake-check-config.nix', '      config: ci/reduced-checks.nix'),
        (6, '      config: ci/flake-check-config.nix',
             '      publish: true\n      config: ci/flake-check-config.nix'),
        (6, '  candidate:\n', '  publisher:\n    runs-on: ubuntu-latest\n  candidate:\n'),
    ]
    for index, before, after in mutations:
        sample = good.copy()
        require(before in sample[index], 'invalid test fixture: ' + before)
        sample[index] = sample[index].replace(before, after)
        try:
            verify(*sample)
        except AssertionError:
            pass
        else:
            raise AssertionError('forged CI policy was admitted: ' + before)


if __name__ == '__main__':
    if sys.argv[1:] == ['--self-test']:
        self_test()
    elif not sys.argv[1:]:
        verify(*files())
        print('Staged P1 CI remote-builder trigger and release policy passed')
    else:
        raise SystemExit('usage: ci-cutover-policy.py [--self-test]')
