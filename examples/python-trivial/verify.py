"""Run from repository root: PYTHONPATH=tools python3 examples/python-trivial/verify.py.

Requires cargo/rustc, a C linker and Python development library (PyO3 0.25).
All generated files and packages stay in the chosen scratch directory.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from phenix_python.__main__ import generate


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


root = Path(__file__).resolve().parents[2]
os.chdir(root)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("work", nargs="?", default="/tmp/opencode/python-trivial-artifacts")
parser.add_argument("--release", action="store_true")
parser.add_argument("--strip", action="store_true")
args = parser.parse_args()
work = Path(args.work).resolve()
profile = "release" if args.release else "debug"
build_flags = ["--release"] if args.release else []
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
run("cargo", "build", "--manifest-path", str(crate / "Cargo.toml"), *build_flags)
run("cargo", "build", "--manifest-path", "examples/python-trivial/rust-equivalent/Cargo.toml", *build_flags)
libraries = [
    ("package", crate / f"target/{profile}/libphenix_python_authored.so"),
    ("rust-package", root / f"examples/python-trivial/rust-equivalent/target/{profile}/libphenix_rust_greeter.so"),
]
for token in ("a", "b", "scalar"):
    source = root / "tools/phenix_python/testdata/scalar.py"
    if token != "scalar":
        source = work / f"concurrent-{token}.py"
        source.write_text((root / "tools/phenix_python/testdata/concurrent.py").read_text()
                          .replace('TOKEN = "A"', f'TOKEN = "{token.upper()}"')
                          .replace("example.concurrent-a", f"example.concurrent-{token}"))
    generated = work / f"plugin-{token}"
    generate(source, generated, public / "crates/phenix-sdk")
    run("cargo", "build", "--manifest-path", str(generated / "Cargo.toml"), *build_flags)
    libraries.append((f"package-{token}", generated / f"target/{profile}/libphenix_python_authored.so"))
for name, library in libraries:
    package = work / name
    (package / "lib").mkdir(parents=True, exist_ok=True)
    artifact = package / "lib/plugin.so"
    shutil.copy2(library, artifact)
    if args.strip:
        run("strip", "--strip-all", str(artifact))
    result = run("cargo", "run", "-q", "-p", "phenix-artifact", "--bin", "phenix-describe", "--",
                 str(artifact), str(package / "descriptor.bin"), capture_output=True)
    (package / "package.json").write_text(json.dumps({
        "format": "phenix.native-package.v2", "artifact": "lib/plugin.so",
        "descriptor": "descriptor.bin", "target": "x86_64-unknown-linux-gnu",
        **json.loads(result.stdout),
    }, sort_keys=True))
run("cargo", "run", "--manifest-path", "examples/python-trivial/runner/Cargo.toml",
    "--bin", "phenix-python-trivial-verification", "--",
    str(work / "package"), str(work / "rust-package"))
with tempfile.TemporaryDirectory(prefix="init-barrier-", dir=work) as barrier:
    run("cargo", "run", "--manifest-path", "examples/python-trivial/runner/Cargo.toml",
        "--bin", "regressions", "--", str(work / "package-a"), str(work / "package-b"),
        str(work / "package-scalar"), env={**os.environ, "PHENIX_INIT_BARRIER": barrier})
