#!/usr/bin/env python3
"""Prepare native plugin sources into the ONE phenix.composition.v3 format.

The Phenix kernel consumes only verified native package directory paths.
Source resolution and building run in the product host, never in the kernel.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit
from urllib.request import urlopen

COMPOSITION = "phenix.composition.v3"
LIBRARIES = {".so", ".dll", ".dylib"}
HEX_SHA = re.compile(r"[0-9a-f]{40}\Z")
HEX_DIGEST = re.compile(r"[0-9a-f]{64}\Z")


def command(*argv, cwd=None):
    return subprocess.check_output([str(arg) for arg in argv], cwd=cwd, text=True).strip()


def source_path(source, base, staging, spec):
    if not isinstance(source, str) or not source:
        raise ValueError("plugin source must be a nonempty path or HTTPS URL")
    if source.startswith(("https://", "git+https://")):
        address = source.removeprefix("git+")
        url = urlsplit(address)
        if url.scheme != "https" or not url.hostname or url.username or url.password:
            raise ValueError("remote plugin source must use plain HTTPS")
        if source.startswith("git+") or url.path.endswith(".git"):
            revision = spec.get("revision")
            if not isinstance(revision, str) or not HEX_SHA.fullmatch(revision):
                raise ValueError("Git plugin requires a pinned 40-character commit revision")
            path = staging / "checkout"
            command("git", "clone", "--no-checkout", "--", address, path)
            command("git", "-C", path, "checkout", "--detach", revision)
            if command("git", "-C", path, "rev-parse", "HEAD") != revision:
                raise ValueError("Git checkout does not match the pinned revision")
            return path
        checksum = spec.get("sha256")
        if not isinstance(checksum, str) or not HEX_DIGEST.fullmatch(checksum):
            raise ValueError("downloaded binary requires a pinned SHA-256 digest")
        suffix = Path(url.path).suffix
        if suffix not in LIBRARIES:
            raise ValueError("binary URL must end in a recognized native library extension")
        path = staging / ("download" + suffix)
        digest = hashlib.sha256()
        total = 0
        with urlopen(address, timeout=30) as response, path.open("wb") as sink:
            if urlsplit(response.geturl()).scheme != "https":
                raise ValueError("binary redirect must remain HTTPS")
            while block := response.read(1024 * 1024):
                total += len(block)
                if total > 128 * 1024 * 1024:
                    raise ValueError("remote native library exceeds 128 MiB")
                digest.update(block)
                sink.write(block)
        if digest.hexdigest() != checksum:
            raise ValueError("remote library SHA-256 mismatch")
        return path
    if "://" in source:
        raise ValueError("unsupported plugin source scheme")
    path = Path(source).expanduser()
    return (path if path.is_absolute() else base / path).resolve(strict=True)


def rust_build(source, spec, cache):
    manifest = source / "Cargo.toml" if source.is_dir() else source
    if manifest.name != "Cargo.toml" or not manifest.is_file():
        raise ValueError("Rust source needs Cargo.toml")
    info = json.loads(command("cargo", "metadata", "--locked", "--no-deps",
                              "--format-version", "1", "--manifest-path", manifest))
    packages = [p for p in info["packages"]
                if any("cdylib" in t.get("crate_types", []) for t in p["targets"])]
    chosen = spec.get("package")
    if chosen is not None:
        packages = [p for p in packages if p["name"] == chosen]
    if len(packages) != 1:
        raise ValueError("Rust source must select exactly one cdylib package")
    package = packages[0]
    features = spec.get("features", [])
    if not isinstance(features, list) or not all(isinstance(f, str) and f for f in features):
        raise ValueError("features must be nonempty strings")
    # Cargo fingerprinting already tracks file contents, features, profiles and
    # toolchains. Keep a stable target directory per workspace so an unchanged
    # local crate is not rebuilt on every Basic runtime startup. Different
    # local workspaces must never share target artifacts.
    workspace = Path(info["workspace_root"]).resolve()
    workspace_key = hashlib.sha256(os.fsencode(workspace)).hexdigest()
    target = cache / "cargo-target" / workspace_key
    target.mkdir(parents=True, exist_ok=True)
    argv = ["cargo", "build", "--locked", "--release", "--package", package["name"],
            "--manifest-path", str(manifest), "--target-dir", str(target),
            "--message-format=json-render-diagnostics"]
    for feature in features:
        argv.extend(["--features", feature])
    built = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if built.returncode != 0:
        raise ValueError("Cargo failed: " + built.stderr[-8000:])
    libs = []
    for line in built.stdout.splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (record.get("reason") == "compiler-artifact"
                and record.get("package_id") == package["id"]
                and "cdylib" in record.get("target", {}).get("crate_types", [])):
            libs.extend(Path(p) for p in record.get("filenames", [])
                        if Path(p).suffix in LIBRARIES)
    if len(libs) != 1 or not libs[0].is_file():
        raise ValueError("Cargo did not emit one unambiguous cdylib")
    return libs[0]


def resolve(spec, base, staging, builders, cache):
    source = source_path(spec.get("source"), base, staging, spec)
    if source.is_dir() and (source / "package.json").is_file():
        if spec.get("builder") not in (None, "auto"):
            raise ValueError("prebuilt package must not request a builder")
        return source, True
    if source.is_file() and source.suffix in LIBRARIES:
        if spec.get("builder") not in (None, "auto"):
            raise ValueError("precompiled library must not request a builder")
        return source, False
    builder = spec.get("builder", "auto")
    if builder == "auto":
        # Accept both a project directory and a direct Cargo.toml path.
        # rust_build validates the actual manifest and cdylib package.
        is_manifest = source.is_file() and source.name == "Cargo.toml"
        is_project = source.is_dir() and (source / "Cargo.toml").is_file()
        builder = "rust" if is_manifest or is_project else None
    if not isinstance(builder, str) or not builder:
        raise ValueError("source is neither a binary nor a recognized build project")
    executor = spec.get("executor", "local")
    if executor == "nix":
        installable = spec.get("installable")
        if not isinstance(installable, str) or not installable:
            raise ValueError("Nix executor requires an explicit installable")
        outputs = command("nix", "build", "--no-link", "--print-out-paths", installable,
                          cwd=source if source.is_dir() else source.parent).splitlines()
        if len(outputs) != 1:
            raise ValueError("Nix build must emit exactly one output")
        result = Path(outputs[0]).resolve(strict=True)
        if result.is_dir() and (result / "package.json").is_file():
            return result, True
        result = result / "lib" / "plugin.so" if result.is_dir() else result
        if not result.is_file():
            raise ValueError("Nix output has no native plugin library")
        return result, False
    if executor != "local":
        raise ValueError("unknown builder executor")
    if builder == "rust":
        return rust_build(source, spec, cache), False
    argv = builders.get(builder)
    if not isinstance(argv, list) or not argv or not all(isinstance(x, str) and x for x in argv):
        raise ValueError("builder strategy is not registered: " + builder)
    destination = staging / "output"
    destination.mkdir()
    lib = command(*(arg.replace("{source}", str(source)).replace("{output}", str(destination))
                    for arg in argv), cwd=source if source.is_dir() else source.parent)
    result = Path(lib)
    if (not result.is_absolute() or not result.is_file()
            or not result.resolve().is_relative_to(destination.resolve())
            or result.suffix not in LIBRARIES):
        raise ValueError("builder must return a shared library inside the output directory")
    return result, False


def package(binary, describe, staging, cache, store):
    # The current platform's artifact verifier is Linux ELF-only. The source
    # abstraction remains independent of the eventual .dll/.dylib validators.
    if binary.suffix != ".so":
        raise ValueError("this Phenix native loader currently accepts ELF .so only")
    contents = binary.read_bytes()
    sha = hashlib.sha256(contents).hexdigest()
    package_dir = staging / "package"
    (package_dir / "lib").mkdir(parents=True)
    (package_dir / "lib" / "plugin.so").write_bytes(contents)
    descriptor = package_dir / "descriptor.bin"
    info = json.loads(command(describe, package_dir / "lib" / "plugin.so", descriptor))
    if (info.get("artifact_sha256") != sha or
            info.get("descriptor_sha256") != hashlib.sha256(descriptor.read_bytes()).hexdigest()):
        raise ValueError("descriptor digests disagree with packaged library")
    manifest = {"format": "phenix.native-package.v2", "artifact": "lib/plugin.so",
                "descriptor": "descriptor.bin", "target": "x86_64-unknown-linux-gnu", **info}
    (package_dir / "package.json").write_text(json.dumps(manifest, sort_keys=True) + "\n")
    if store == "nix":
        result = Path(command("nix", "store", "add-path", package_dir)).resolve(strict=True)
        if not result.is_dir():
            raise ValueError("Nix did not create a package output path")
        return result
    if store != "local":
        raise ValueError("store must be local or nix")
    cache.mkdir(parents=True, exist_ok=True)
    final = cache / ("native-" + sha)
    if not final.exists():
        package_dir.rename(final)
        for path in final.rglob("*"):
            path.chmod(0o555 if path.is_dir() else 0o444)
        final.chmod(0o555)
    # The kernel verifies the manifest and embedded descriptors again before
    # loading. Local same-user files are trusted deployment inputs.
    return final.resolve(strict=True)


def prepare(config_file, output_file, describe, cache, store, extra_sources=None):
    spec = json.loads(config_file.read_text(encoding="utf-8"))
    if (not isinstance(spec, dict) or spec.get("format") != COMPOSITION or
            not isinstance(spec.get("artifacts"), list) or
            not all(isinstance(x, str) for x in spec["artifacts"])):
        raise ValueError("expected only phenix.composition.v3 with artifacts array")
    sources = spec.pop("plugin_sources", [])
    builders = spec.pop("builders", {})
    # CLI flags are a convenience for constructing the same canonical file.
    # They never synthesize authority, grants, or slot bindings.
    if extra_sources is not None:
        if not isinstance(extra_sources, list) or not all(
                isinstance(x, str) and x for x in extra_sources):
            raise ValueError("--plugin requires nonempty source paths")
        if not isinstance(sources, list):
            raise ValueError("plugin_sources must be an array")
        sources = sources + [{"source": value} for value in extra_sources]
    if not isinstance(sources, list) or not isinstance(builders, dict):
        raise ValueError("plugin_sources must be an array, builders must be an object")
    packages = []
    for source in sources:
        if not isinstance(source, dict):
            raise ValueError("plugin source must be an object")
        with tempfile.TemporaryDirectory(prefix="phenix-plugin-") as scratch:
            location, prebuilt = resolve(source, config_file.parent, Path(scratch), builders, cache)
            packages.append(str(location if prebuilt else
                                package(location, describe, Path(scratch), cache, store)))
    spec["artifacts"] = sorted(set(spec["artifacts"] + packages))
    output_file.parent.mkdir(parents=True, exist_ok=True)
    if output_file.resolve() == config_file.resolve():
        raise ValueError("refusing to overwrite the source configuration")
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8",
                                     dir=output_file.parent, delete=False) as tmp:
        json.dump(spec, tmp, sort_keys=True, indent=2)
        tmp.write("\n")
        temp_path = Path(tmp.name)
    temp_path.replace(output_file)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("composition", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--describe", default="phenix-describe")
    parser.add_argument("--plugin", action="append", default=[], metavar="SOURCE",
                        help="Append a plugin package, shared object, or source project; "
                             "repeatable. Never infers composition authority or bindings.")
    parser.add_argument("--store", choices=("local", "nix"), default="local")
    parser.add_argument("--cache-dir", type=Path,
                        default=Path(os.environ.get("PHENIX_PLUGIN_CACHE_DIR",
                                                   str(Path.home() / ".cache/phenix/plugins"))))
    args = parser.parse_args()
    try:
        prepare(args.composition.resolve(strict=True), args.output.resolve(),
                args.describe, args.cache_dir, args.store, args.plugin)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError,
            json.JSONDecodeError) as error:
        parser.exit(1, f"phenix-plugin-sources: {error}\n")


if __name__ == "__main__":
    main()
