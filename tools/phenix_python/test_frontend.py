import tempfile
from pathlib import Path
import unittest

from phenix_python.__main__ import generate


PREFIX = '''from dataclasses import dataclass
from phenix_python import plugin, expose
@dataclass
class Text:
    text: str
@plugin(id="test.python", version="1.0.0", contract="test.python@1")
class Greeter:
'''


class FrontendTests(unittest.TestCase):
    def compile(self, method, prefix=PREFIX):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "plugin.py"
            source.write_text(prefix + method)
            generate(source, root / "out", root / "sdk")
            return {p.name: p.read_text() for p in (root / "out").rglob("*") if p.is_file()}

    def test_canonical_macros_own_metadata(self):
        files = self.compile("    @expose\n    def greet(self, request: Text) -> Text: return request\n")
        self.assertIn("derive(phenix_sdk::Portable)", files["contract.rs"])
        self.assertIn("phenix_sdk::contract", files["contract.rs"])
        self.assertIn("phenix_sdk::plugin", files["lib.rs"])
        self.assertIn("graph.native_section()", files["build.rs"])
        self.assertNotIn("phenix_python", files["runtime.py"])

    def test_scalar(self):
        self.assertIn("request: String", self.compile(
            "    @expose\n    def greet(self, request: str) -> str: return request\n")["contract.rs"])

    def test_image_independent_module_identity(self):
        files = self.compile("    @expose\n    def greet(self, request: Text) -> Text: return request\n")
        self.assertIn('getattr("uuid4")', files["lib.rs"])
        self.assertIn("current.is(&module)", files["lib.rs"])
        self.assertNotIn("AtomicU64", files["lib.rs"])

    def test_build_imports_do_not_collide_with_dataclasses(self):
        for name in ("Operation", "ValueSchema"):
            files = self.compile(
                f"    @expose\n    def greet(self, request: {name}) -> {name}: return request\n",
                PREFIX.replace("Text", name))
            self.assertIn(f"pub struct {name}", files["contract.rs"])
            self.assertIn("fn main() {\n    use phenix_sdk::", files["build.rs"])

    def test_fail_closed(self):
        for method in [
            "    @expose\n    async def greet(self, request: Text) -> Text: return request\n",
            "    @expose\n    async def greet(self, request: Text) -> Text: yield request\n",
            "    @expose\n    def greet(self, request: Text) -> Text: yield request\n",
            "    @expose\n    def greet(self, request: int) -> Text: return Text('x')\n",
            "    @expose\n    def greet(self, request: list[str]) -> Text: return Text('x')\n",
            "    @expose\n    def greet(self, request: Text, host: str) -> Text: return request\n",
            "    @expose\n    def greet(self, request) -> Text: return request\n",
        ]:
            with self.subTest(method=method), self.assertRaises(ValueError):
                self.compile(method)

    def test_unsupported_dataclass_fields(self):
        for annotation in ("int", "list[str]", "str | None"):
            with self.subTest(annotation=annotation), self.assertRaises(ValueError):
                self.compile("    @expose\n    def greet(self, request: Text) -> Text: return request\n",
                             PREFIX.replace("text: str", f"text: {annotation}"))

    def test_extra_operations_reject(self):
        with self.assertRaisesRegex(ValueError, "exactly one @expose"):
            self.compile("    @expose\n    def greet(self, request: Text) -> Text: return request\n"
                         "    @expose\n    def other(self, request: Text) -> Text: return request\n")

    def test_runtime_source_not_rewritten_as_rust(self):
        files = self.compile("    @expose\n    def greet(self, request: Text) -> Text: return Text('r#unchanged')\n")
        self.assertIn("r#unchanged", files["runtime.py"])


if __name__ == "__main__":
    unittest.main()
