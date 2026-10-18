#!/usr/bin/env python3
"""Single-schema plugin source preparation regressions (stdlib only)."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

file = Path(os.environ.get("PHENIX_SOURCE_TOOL",
                           str(Path(__file__).resolve().parents[1] / "tools/phenix-plugin-sources.py")))
spec = importlib.util.spec_from_file_location("phenix_plugin_sources", file)
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def config(self, **extra):
        doc = {"format": "phenix.composition.v3",
               "artifacts": ["/nix/store/example-package"],
               "entry": {"interface": "i", "operation": "o", "provider": "p"}}
        doc.update(extra)
        path = self.root / "input.json"
        path.write_text(json.dumps(doc))
        return path

    def prepare(self, path):
        output = self.root / "output.json"
        tool.prepare(path, output, "describe", self.root / "cache", "local")
        return json.loads(output.read_text())

    def test_pass_through_and_reject_v2(self):
        original = self.config()
        self.assertEqual(self.prepare(original), json.loads(original.read_text()))
        legacy = self.config(format="phenix.composition.v2")
        with self.assertRaisesRegex(ValueError, "composition.v3"):
            self.prepare(legacy)

    def test_existing_package_is_referenced_without_rebuilding(self):
        package = self.root / "package"
        package.mkdir()
        (package / "package.json").write_text("{}")
        result = self.prepare(self.config(plugin_sources=[{"source": "package"}]))
        self.assertIn(str(package), result["artifacts"])
        self.assertNotIn("plugin_sources", result)

    def test_library_is_described_and_packaged(self):
        library = self.root / "plugin.so"
        library.write_bytes(b"dummy-test-shared-object")
        digest = hashlib.sha256(library.read_bytes()).hexdigest()
        descriptor = b"test-descriptor"
        def describe(*argv, cwd=None):
            self.assertEqual(argv[0], "describe")
            Path(argv[2]).write_bytes(descriptor)
            return json.dumps({"artifact_sha256": digest,
                               "descriptor_sha256": hashlib.sha256(descriptor).hexdigest()})
        with mock.patch.object(tool, "command", side_effect=describe):
            result = self.prepare(self.config(plugin_sources=[{"source": "plugin.so"}]))
        directory = self.root / "cache" / ("native-" + digest)
        self.assertIn(str(directory), result["artifacts"])
        self.assertEqual((directory / "lib/plugin.so").read_bytes(), library.read_bytes())
        record = json.loads((directory / "package.json").read_text())
        self.assertEqual(record["artifact_sha256"], digest)

    def test_remote_requires_content_identity(self):
        with self.assertRaisesRegex(ValueError, "pinned"):
            tool.source_path("https://example.test/a.so", self.root, self.root, {})
        with self.assertRaisesRegex(ValueError, "pinned"):
            tool.source_path("git+https://example.test/a.git", self.root, self.root, {})
        with self.assertRaisesRegex(ValueError, "unsupported"):
            tool.source_path("http://example.test/a.so", self.root, self.root, {})

    def test_cpp_strategy_requires_registration(self):
        source = self.root / "cpp"
        source.mkdir()
        with self.assertRaisesRegex(ValueError, "not registered"):
            tool.resolve({"source": "cpp", "builder": "cpp"}, self.root, self.root, {})

    def test_nix_strategy_uses_single_output_path(self):
        project = self.root / "crate"
        project.mkdir()
        (project / "Cargo.toml").write_text("[package]\nname='p'\nversion='0.1.0'\n")
        package = self.root / "nix-package"
        package.mkdir()
        (package / "package.json").write_text("{}")
        with mock.patch.object(tool, "command", return_value=str(package)) as run:
            result, prebuilt = tool.resolve({
                "source": "crate", "builder": "rust", "executor": "nix",
                "installable": ".#plugin"}, self.root, self.root, {})
        self.assertTrue(prebuilt)
        self.assertEqual(result, package)
        self.assertEqual(run.call_args.args[0], "nix")

    def test_builder_is_not_inferred_from_unknown_directory(self):
        (self.root / "unknown").mkdir()
        with self.assertRaisesRegex(ValueError, "recognized"):
            tool.resolve({"source": "unknown"}, self.root, self.root, {})


if __name__ == "__main__":
    unittest.main()
