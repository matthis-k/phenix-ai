#!/usr/bin/env python3
"""Network-free fail-closed tests for the temporary exact-ref cleanup."""
import importlib.util
import os
from pathlib import Path
import unittest
from unittest import mock
from urllib.parse import unquote

SCRIPT = Path(__file__).with_name("cleanup-incremental-probe-branches.py")
spec = importlib.util.spec_from_file_location("historical_probe_cleanup", SCRIPT)
cleanup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cleanup)
REF_PREFIX = "/git/ref/heads/"
TRUSTED = {
    "GITHUB_REPOSITORY": cleanup.REPO,
    "GITHUB_EVENT_NAME": "push",
    "GITHUB_REF": "refs/heads/main",
}


class CleanupTests(unittest.TestCase):
    def test_rejects_new_pr_head_before_deleting_anything(self):
        calls = []
        first = next(iter(cleanup.EXPECTED))
        heads = iter([set(), {first}])

        def request(method, url):
            calls.append((method, url))
            self.assertEqual(method, "GET")
            self.assertTrue(url.startswith(REF_PREFIX))
            name = unquote(url[len(REF_PREFIX):])
            return 200, {"object": {"sha": cleanup.EXPECTED[name]}}

        with mock.patch.dict(os.environ, TRUSTED), \
             mock.patch.object(cleanup, "open_pr_heads", side_effect=lambda: next(heads)), \
             mock.patch.object(cleanup, "request", side_effect=request):
            with self.assertRaisesRegex(RuntimeError, "became an active PR head"):
                cleanup.execute()
        self.assertEqual(len(calls), len(cleanup.EXPECTED))
        self.assertFalse(any(method == "DELETE" for method, _ in calls))

    def test_rejects_moved_ref_before_deletion(self):
        calls = []
        first = next(iter(cleanup.EXPECTED))
        counts = {}

        def request(method, url):
            calls.append((method, url))
            self.assertEqual(method, "GET")
            name = unquote(url[len(REF_PREFIX):])
            counts[name] = counts.get(name, 0) + 1
            if name == first and counts[name] == 2:
                return 200, {"object": {"sha": "0" * 40}}
            return 200, {"object": {"sha": cleanup.EXPECTED[name]}}

        with mock.patch.dict(os.environ, TRUSTED), \
             mock.patch.object(cleanup, "open_pr_heads", return_value=set()), \
             mock.patch.object(cleanup, "request", side_effect=request):
            with self.assertRaisesRegex(RuntimeError, "moved before deletion"):
                cleanup.execute()
        self.assertEqual(len(calls), len(cleanup.EXPECTED) + 1)

    def test_deletes_only_exact_verified_refs(self):
        refs = dict(cleanup.EXPECTED)
        deletes = []

        def request(method, url):
            self.assertTrue(url.startswith(REF_PREFIX))
            name = unquote(url[len(REF_PREFIX):])
            self.assertIn(name, cleanup.EXPECTED)
            if method == "GET":
                return (200, {"object": {"sha": refs[name]}}) if name in refs else (404, None)
            self.assertEqual(method, "DELETE")
            deletes.append(name)
            del refs[name]
            return 204, None

        with mock.patch.dict(os.environ, TRUSTED), \
             mock.patch.object(cleanup, "open_pr_heads", return_value=set()), \
             mock.patch.object(cleanup, "request", side_effect=request):
            cleanup.execute()
        self.assertFalse(refs)
        self.assertEqual(set(deletes), set(cleanup.EXPECTED))
        self.assertEqual(len(deletes), len(cleanup.EXPECTED))

    def test_untrusted_context_denies_before_read_or_delete(self):
        with mock.patch.dict(os.environ, {**TRUSTED, "GITHUB_EVENT_NAME": "pull_request"}), \
             mock.patch.object(cleanup, "request") as request:
            with self.assertRaisesRegex(RuntimeError, "trusted main-only"):
                cleanup.execute()
            request.assert_not_called()


if __name__ == "__main__":
    unittest.main()
