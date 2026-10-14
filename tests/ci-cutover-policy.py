"""Fail-closed policy gate for the staged post-P1 flake-CI cutover.

This is a static trust and release-boundary check, not a replacement for
same-head Nix shipping, remote-store and official ACP interoperability tests.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REMOTE_BRANCH = 'ci/m1-p1-ci-cutover'
OLD_BENCHMARK_BRANCH = 'ci/m1-generated-backend-consumer'
EXPECTED_SOURCE_PIN = 'a1480620950508215bdda4a5404f046becf65cf8'


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def verify(generated, renderer, backend, check_config, flake, canonical, matrix, dag, acp):
    # Neither a fork PR nor an unreviewed main push may receive tailnet
    # credentials before the remote backend completes cutover acceptance.
    require('  pull_request:' not in generated, 'untrusted remote PR event')
    require('  id-token: write' in generated, 'missing OIDC permission')
    require('      - "' + REMOTE_BRANCH + '"' in generated, 'cutover branch not enabled')
    require('      - "main"' not in generated, 'remote main promotion is premature')
    require('      - "' + OLD_BENCHMARK_BRANCH + '"' in generated,
            'established remote benchmark branch disappeared')
    require('on:\n  push:\n    branches:' in generated, 'remote workflow needs push-only event')
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
    # Preserve independent authoritative release checks until same-head remote
    # full-inventory, security and cold/warm checks are proven.
    require('branches: [main,' in canonical, 'canonical main gate removed prematurely')
    require('branches: [main,' in matrix, 'flake-matrix main gate removed prematurely')
    require('branches: [main,' in dag, 'generated DAG main gate removed prematurely')
    require('P1 official ACP SDK interop' in acp and 'branches: [main]' in acp,
            'independent ACP SDK main check removed')


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
    ]


def self_test():
    originals = files()
    # Synthetic valid policy is permitted only for testing this validator;
    # the real repository policy is checked by verify(*files()) separately.
    good = originals.copy()
    good[0] = good[0].replace('      - "' + OLD_BENCHMARK_BRANCH + '"',
                              '      - "' + OLD_BENCHMARK_BRANCH + '"\n      - "' + REMOTE_BRANCH + '"')
    good[2] = good[2].replace('credentials = "oidc";',
                              'credentials = "oidc";\n    workers = 4;\n    slotsPerWorker = 2;')
    verify(*good)
    mutations = [
        (0, '      - "' + REMOTE_BRANCH + '"', '      - "main"'),
        (0, 'on:\n  push:', 'on:\n  pull_request:\n  push:'),
        (0, '      worker_count: 4', '      worker_count: 8'),
        (0, 'library_ref: "' + EXPECTED_SOURCE_PIN + '"',
             'library_ref: "' + 'f' * 40 + '"'),
        (2, 'workers = 4;', 'workers = 8;'),
        (4, 'p1-release-preflight', 'not-an-actual-release-check'),
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
