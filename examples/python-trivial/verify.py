"""Run from repository root: PYTHONPATH=tools python3 examples/python-trivial/verify.py.

Requires cargo/rustc, a C linker and Python development library (PyO3 0.25).
All generated files and packages stay in the chosen scratch directory.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from phenix_python.__main__ import generate


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


root = Path(__file__).resolve().parents[2]
os.chdir(root)
work = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/python-trivial-artifacts")
work.mkdir(parents=True, exist_ok=True)
# Physically exclude private kernel/artifact sources from the plugin's SDK tree.
public = work / "public-sdk"
public.mkdir(exist_ok=True)
shutil.copy2(root / "Cargo.toml", public / "Cargo.toml")
for path in ("crates/phenix-sdk", "crates/phenix-contract", "crates/phenix-macros", "include"):
    shutil.copytree(root / path, public / path, dirs_exist_ok=True)
crate = work / "plugin"
generate(root / "examples/python-trivial/greeter.py", crate, public / "crates/phenix-sdk")
os.environ["PYO3_PYTHON"] = sys.executable
run("cargo", "build", "--manifest-path", str(crate / "Cargo.toml"))
run("cargo", "build", "--manifest-path", "examples/python-trivial/rust-equivalent/Cargo.toml")
for name, library in (
    ("package", crate / "target/debug/libphenix_python_authored.so"),
    ("rust-package", root / "examples/python-trivial/rust-equivalent/target/debug/libphenix_rust_greeter.so"),
):
    package = work / name
    (package / "lib").mkdir(parents=True, exist_ok=True)
    artifact = package / "lib/plugin.so"
    shutil.copy2(library, artifact)
    result = run("cargo", "run", "-q", "-p", "phenix-artifact", "--bin", "phenix-describe", "--",
                 str(artifact), str(package / "descriptor.bin"), capture_output=True)
    (package / "package.json").write_text(json.dumps({
        "format": "phenix.native-package.v2", "artifact": "lib/plugin.so",
        "descriptor": "descriptor.bin", "target": "x86_64-unknown-linux-gnu",
        **json.loads(result.stdout),
    }, sort_keys=True))
run("cargo", "run", "--manifest-path", "examples/python-trivial/runner/Cargo.toml", "--",
    str(work / "package"), str(work / "rust-package"))
