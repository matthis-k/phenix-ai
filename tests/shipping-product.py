"""Acceptance over one pinned product and all prior real-artifact gates."""
import json, re, sys
from pathlib import Path
kernel, sdk_check, conformance, closure, examples, output = map(Path, sys.argv[1:])
output.mkdir(parents=True)
required = {"contract", "descriptor", "native", "product", "composition", "composition-product", "lifecycle", "lifecycle-product", "pending", "pending-product", "imports", "generations", "wire", "metadata-state", "authoring", "live-state", "live-state-product", "value-sdk", "value-sdk-product", "agent", "agent-product", "graph", "basic-contracts", "scripted-model", "http-model", "basic-tools", "event-journal", "basic-session", "sqlite-restart", "basic-memory", "basic-agent", "frontends", "basic-host-smoke", "basic-pending-http-product", "acp-codec"}
assert {p.name for p in conformance.iterdir()} == required
assert (conformance / "contract" / "contract").read_text().strip() == "passed"
assert (sdk_check / "public-sdk").read_text().strip() == "passed"
reports = []
for name in sorted(required - {"contract"}):
    files = sorted((conformance / name).rglob("*.json"))
    assert files, name
    for path in files:
        parsed = json.loads(path.read_text())
        assert isinstance(parsed, (dict, list)), path
        reports.append(dict(gate=name, file=str(path.relative_to(conformance)), bytes=path.stat().st_size))
closure_paths = (closure / "store-paths").read_text().splitlines()
assert str(kernel) in closure_paths
for path in closure_paths:
    assert not any(name in Path(path).name for name in ["example-", "phenix-conformance", "phenix-external-plugin", "phenix-sdk", "opencode", "phenix-plugin"]), path
production_files = sorted(str(p.relative_to(kernel)) for p in kernel.rglob("*") if p.is_file())
assert "bin/phenix-kernel" in production_files
assert not any(p.endswith("plugin.so") for p in production_files)
# Protocol and adversarial fixtures supply independent ABI 3 producers.
# Ordinary M1/A0 authors must use generated native glue.
independent_abi_fixtures = {"reactive-client", "reactive-provider", "value-client", "value-provider", "pending-security"}
raw_import_cases = {"forged slot", "forged operation", "forged cancel zero effects",
                    "valid child survives forgeries", "cross-thread scope rejection"}
import_report = json.loads((conformance / "imports" / "evidence.json").read_text())
assert raw_import_cases <= set(import_report["cases"])
assert (examples / "pending-security" / "src" / "lib.rs").is_file()
fixture_files = sorted(p for p in examples.rglob("*.rs")
                       if p.relative_to(examples).parts[0] not in independent_abi_fixtures)
assert fixture_files
for path in fixture_files:
    source = path.read_text()
    assert not re.search(r"phenix_native_v[123]|descriptor_pending\s*\(|descriptor_with_policy\s*\(|link_section\s*=\s*\"\.phenix\"", source), path
result = dict(format="phenix.m1.shipping.v1", gates=sorted(required | {"public-sdk"}), kernel=str(kernel), sdk_check=str(sdk_check), realized_gates={name:str((conformance / name).resolve()) for name in sorted(required)}, production_files=production_files, production_closure=closure_paths, parsed_reports=reports, independently_authored_fixture_files=len(fixture_files))
result["independent_abi_fixtures"] = sorted(independent_abi_fixtures)
result["adversarial_import_cases"] = sorted(raw_import_cases)
(output / "evidence.json").write_text(json.dumps(result, sort_keys=True, indent=2))
print("M1 integrated shipping conformance passed")
