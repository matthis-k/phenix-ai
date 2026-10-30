#!/usr/bin/env python3
"""Single-schema plugin source preparation regressions (stdlib only)."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import subprocess
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

    def test_repeatable_cli_plugin_sources_generate_same_canonical_file(self):
        first = self.root / "alpha"
        second = self.root / "beta"
        for directory in (first, second):
            directory.mkdir()
            (directory / "package.json").write_text("{}")
        input_file = self.config(
            plugin_sources=[{"source": "alpha"}],
            slots=[{"caller": "explicit", "slot": "Catalog", "index": 0,
                    "provider": "chosen", "export": "Catalog"}],
            grants=[{"caller": "root", "interface": "i",
                     "operation": "o", "resources": []}],
        )
        output = self.root / "out.json"
        tool.prepare(input_file, output, "describe", self.root / "cache",
                     "local", ["beta"])
        result = json.loads(output.read_text())
        self.assertEqual(result["artifacts"],
                         sorted(["/nix/store/example-package", str(first), str(second)]))
        self.assertEqual(result["slots"],
                         json.loads(input_file.read_text())["slots"])
        self.assertEqual(result["grants"],
                         json.loads(input_file.read_text())["grants"])
        self.assertNotIn("plugin_sources", result)
        self.assertNotIn("builders", result)
        with self.assertRaisesRegex(ValueError, "nonempty"):
            tool.prepare(input_file, output, "describe", self.root / "cache",
                         "local", [""])

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
            tool.resolve({"source": "cpp", "builder": "cpp"}, self.root, self.root, {}, self.root / "cache")

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
                "installable": ".#plugin"}, self.root, self.root, {}, self.root / "cache")
        self.assertTrue(prebuilt)
        self.assertEqual(result, package)
        self.assertEqual(run.call_args.args[0], "nix")

    def test_auto_rust_builder_accepts_crate_directory_or_cargo_manifest(self):
        project = self.root / "crate"
        project.mkdir()
        manifest = project / "Cargo.toml"
        manifest.write_text("[package]\nname='fixture'\nversion='0.1.0'\n")
        library = self.root / "built.so"
        with mock.patch.object(tool, "rust_build", return_value=library) as build:
            for source in ("crate", "crate/Cargo.toml"):
                result, prebuilt = tool.resolve(
                    {"source": source}, self.root, self.root, {}, self.root / "cache")
                self.assertEqual(result, library)
                self.assertFalse(prebuilt)
            self.assertEqual([c.args[0] for c in build.call_args_list],
                             [project, manifest])

    def test_rust_builder_compiles_a_real_cdylib(self):
        crate = self.root / "smoke-crate"
        (crate / "src").mkdir(parents=True)
        (crate / "Cargo.toml").write_text(
            '[package]\nname = "phenix-source-smoke"\nversion = "0.1.0"\nedition = "2021"\n'
            '[lib]\ncrate-type = ["cdylib"]\n')
        (crate / "src" / "lib.rs").write_text(
            '#[no_mangle]\npub extern "C" fn smoke() -> u8 { 1 }\n')
        subprocess.run(["cargo", "generate-lockfile", "--offline",
                        "--manifest-path", str(crate / "Cargo.toml")], check=True)
        cache = self.root / "cache"
        library = tool.rust_build(crate, {}, cache)
        self.assertEqual(library.suffix, ".so")
        self.assertTrue(library.is_file())
        # A second preparation of the same source reuses the exact target
        # directory and Cargo's fingerprinted, compiled cdylib.
        first_sha = hashlib.sha256(library.read_bytes()).hexdigest()
        first_mtime = library.stat().st_mtime_ns
        repeated = tool.rust_build(crate, {}, cache)
        self.assertEqual(repeated, library)
        self.assertEqual(repeated.stat().st_mtime_ns, first_mtime)
        self.assertEqual(hashlib.sha256(repeated.read_bytes()).hexdigest(), first_sha)
        # Editing this one plugin triggers its compilation, not an unrelated
        # workspace's target build directory.
        (crate / "src" / "lib.rs").write_text(
            '#[no_mangle]\npub extern "C" fn smoke() -> u8 { 2 }\n')
        updated = tool.rust_build(crate, {}, cache)
        self.assertEqual(updated, library)
        self.assertNotEqual(hashlib.sha256(updated.read_bytes()).hexdigest(), first_sha)

    def test_workspace_source_edit_keeps_unrelated_cdylib_cached(self):
        # Two independent plugins in the same Cargo workspace must share
        # dependency fingerprints without recompiling the unchanged sibling.
        workspace = self.root / "workspace"
        workspace.mkdir()
        (workspace / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["a", "b"]\nresolver = "2"\n')
        names = {}
        for part in ("a", "b"):
            crate = workspace / part
            (crate / "src").mkdir(parents=True)
            name = "phenix-source-" + part
            names[part] = name
            (crate / "Cargo.toml").write_text(
                '[package]\nname = "' + name + '"\nversion = "0.1.0"\n'
                'edition = "2021"\n[lib]\ncrate-type = ["cdylib"]\n')
            (crate / "src" / "lib.rs").write_text(
                '#[no_mangle]\npub extern "C" fn smoke_' + part +
                '() -> u8 { 1 }\n')
        subprocess.run(["cargo", "generate-lockfile", "--offline",
                        "--manifest-path", str(workspace / "Cargo.toml")], check=True)
        cache = self.root / "cache"
        first_a = tool.rust_build(workspace / "a", {"package": names["a"]}, cache)
        first_b = tool.rust_build(workspace / "b", {"package": names["b"]}, cache)
        self.assertEqual(first_a.parent, first_b.parent,
                         "same Cargo workspace should share a target directory")
        a_sha = hashlib.sha256(first_a.read_bytes()).hexdigest()
        b_sha = hashlib.sha256(first_b.read_bytes()).hexdigest()
        b_mtime = first_b.stat().st_mtime_ns
        (workspace / "a" / "src" / "lib.rs").write_text(
            '#[no_mangle]\npub extern "C" fn smoke_a() -> u8 { 2 }\n')
        rebuilt_a = tool.rust_build(workspace / "a", {"package": names["a"]}, cache)
        self.assertNotEqual(hashlib.sha256(rebuilt_a.read_bytes()).hexdigest(), a_sha)
        rebuilt_b = tool.rust_build(workspace / "b", {"package": names["b"]}, cache)
        self.assertEqual(hashlib.sha256(rebuilt_b.read_bytes()).hexdigest(), b_sha)
        self.assertEqual(rebuilt_b.stat().st_mtime_ns, b_mtime,
                         "editing plugin A must not recompile plugin B")

    def test_builder_is_not_inferred_from_unknown_directory(self):
        (self.root / "unknown").mkdir()
        with self.assertRaisesRegex(ValueError, "recognized"):
            tool.resolve({"source": "unknown"}, self.root, self.root, {}, self.root / "cache")


if __name__ == "__main__":
    unittest.main()
