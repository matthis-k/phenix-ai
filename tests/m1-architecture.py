"""Guard removal of active compatibility implementations; behavioral gates are separate."""
import json
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
production = sorted(p for p in (root / "crates").glob("*/src/**/*.rs")
                    if p.relative_to(root).parts[1] != "phenix-conformance")
source = "\n".join(p.read_text() for p in production)
obsolete = ["Schema", "SchemaSpec", "Descriptor", "Deployment", "StateComposition",
            "WireProfile", "LivePortable", "PluginTable", "PendingPluginTable",
            "HostApi", "PendingHostApi"]
for name in obsolete:
    assert not re.search(r"\b(?:struct|enum|trait|type)\s+" + name + r"\b", source), name
for name in ["normalize_metadata", "normalize_state", "empty_descriptor",
             "phenix_native_v1", "phenix_native_v2"]:
    assert not re.search(r"\bfn\s+" + name + r"\b", source), name
for kind, name in [("trait", "Portable"), ("struct", "ContractMetadata"),
                   ("enum", "ValueSchema"), ("struct", "Composition")]:
    assert len(re.findall(r"\bpub\s+" + kind + r"\s+" + name + r"\b", source)) == 1, name
assert len(re.findall(r"\bpub\s+fn\s+prepare\b", source)) == 1
assert len(re.findall(r"\bfn\s+effect_scope\b", source)) == 1
assert "SchemaSpec::Live" not in source
assert not (root / "crates/phenix-contract/src/descriptor/encode.rs").exists()
assert not (root / "crates/phenix-kernel/src/state_resolve.rs").exists()
public = [p for p in production if p.relative_to(root).parts[1] in
          {"phenix-contract", "phenix-sdk"}]
kernel = [p for p in production if p.relative_to(root).parts[1] == "phenix-kernel"]
print(json.dumps(dict(
    format="phenix.m1.architecture.v1", obsolete_declarations_absent=obsolete,
    canonical_contract_models=1, preparation_input_shapes=1,
    production_crates=len({p.relative_to(root).parts[1] for p in production}),
    production_source_lines=sum(len(p.read_text().splitlines()) for p in production),
    kernel_source_lines=sum(len(p.read_text().splitlines()) for p in kernel),
    public_contract_sdk_declarations=sum(len(re.findall(
        r"^pub (?:struct|enum|trait|type)\b", p.read_text(), re.M)) for p in public),
    semantic_guarantees_proven_here=False,
), sort_keys=True))
