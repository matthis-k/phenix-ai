"""Reject test-only plugins in shipping mixes and verify exact test deltas.

Run by the independent Nix agent-mix-boundaries check; no test plugin may
implicitly enter a Basic or Advanced composition or its binary closure.
"""
import json
from pathlib import Path
import sys


def read(path):
    return json.loads(Path(path).read_text())


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def is_test_artifact(path):
    return any(
        marker in str(path)
        for marker in (
            "phenix-python-poc",
            "phenix-python-authored",
            "phenix-python-host-fixture",
            "python-trivial",
            "phenix-plugin-model-history-test",
            "phenix-plugin-provider-loopback-fixture",
        )
    )


def check_history(name, production, testing, caller, slot):
    expected = dict(production)
    additions = [
        artifact for artifact in testing["artifacts"]
        if artifact not in production["artifacts"]
    ]
    require(len(additions) == 1 and "history-test" in additions[0],
            f"{name}: expected exactly one history-test Model artifact")
    expected["artifacts"] = production["artifacts"] + additions
    matching = [
        binding for binding in production["slots"]
        if binding["caller"] == caller and binding["slot"] == slot
    ]
    require(len(matching) == 1, f"{name}: expected one selected model slot")
    expected["slots"] = [
        {**binding, "provider": "phenix.model.history-test"}
        if binding["caller"] == caller and binding["slot"] == slot else binding
        for binding in production["slots"]
    ]
    require(testing == expected,
            f"{name}: history test mix changed more than its artifact and selected slot")
    return additions[0]


def main():
    (basic_path, advanced_path, basic_test_path, advanced_test_path,
     provider_path, provider_test_path, catalog_path, sdk_path,
     basic_closure_path, advanced_closure_path, output_path) = sys.argv[1:]
    basic, advanced = read(basic_path), read(advanced_path)
    basic_test, advanced_test = read(basic_test_path), read(advanced_test_path)
    provider, provider_test = read(provider_path), read(provider_test_path)
    catalog = read(catalog_path)
    require(catalog["format"] == "phenix.domain-testing-mixes.v1",
            "unexpected domain testing mix format")
    domains = catalog["domains"]
    require(set(domains) == {
        "model-history", "provider-loopback", "graph-v2-python-poc",
        "graph", "storage", "frontend", "advanced",
    }, "testing domains must be explicitly enumerated")
    expected_checks = {
        "graph": {"m1-graph"},
        "storage": {
            "b2-sqlite", "p2-project-storage-product",
            "p2-project-membership-product", "p2-project-source-product",
        },
        "frontend": {"f1-frontends", "f2-acp", "basic-host-smoke"},
        "advanced": {
            "p2-model-adapter-product", "p2-compaction-checkpoints-product",
            "p2-evidence-store-product",
        },
    }
    for domain, checks in expected_checks.items():
        item = domains[domain]
        require(item["kind"] == "native-check-domain"
                and set(item["checks"]) == checks
                and len(item["checks"]) == len(checks),
                f"{domain}: invalid domain check selection")
    require(domains["model-history"] == {
        "basic": basic_test_path,
        "advanced": advanced_test_path,
    }, "model history mix references do not match selected test compositions")
    require(domains["provider-loopback"] == {"management": provider_test_path},
            "provider testing mix must use its own composition")

    for name, mix in (("Basic", basic), ("Advanced", advanced)):
        require(mix["format"] == "phenix.composition.v3",
                f"{name}: wrong composition format")
        require(not any(is_test_artifact(path) for path in mix["artifacts"]),
                f"{name}: test-only artifact is part of the product composition")

    history = {
        "basic": check_history("Basic", basic, basic_test,
                               "phenix.basic.agent", "Model"),
        "advanced": check_history("Advanced", advanced, advanced_test,
                                  "phenix.advanced.model-adapter", "Upstream"),
    }

    expected_provider = dict(provider)
    expected_provider["entry"] = {**provider["entry"],
                                  "provider": "phenix.provider.loopback-fixture"}
    require(len(provider_test["artifacts"]) == 1
            and "loopback-fixture" in provider_test["artifacts"][0],
            "provider mix must use only the loopback test artifact")
    expected_provider["artifacts"] = provider_test["artifacts"]
    require(provider_test == expected_provider,
            "provider mix unexpectedly modified grants, definitions or bindings")

    python_mix = domains["graph-v2-python-poc"]
    require(python_mix["production"] is False
            and python_mix["kind"] == "standalone-experiment",
            "Python must remain a standalone non-product experiment")
    python_source = Path(python_mix["source"])
    require((python_source / "examples/python-trivial/greeter.py").is_file(),
            "missing sole public Python example")
    require((python_source / "tools/phenix_python/testdata/concurrent.py").is_file(),
            "missing internal concurrent Python regression")
    require((python_source / "tools/phenix_python/testdata/scalar.py").is_file(),
            "missing internal scalar Python regression")
    require(not (Path(sdk_path) / "tools/phenix_python").exists(),
            "experimental Python frontend leaked into public production Rust SDK")

    for name, closure in (("Basic", basic_closure_path),
                          ("Advanced", advanced_closure_path)):
        paths = Path(closure).read_text().splitlines()
        require(not any(is_test_artifact(Path(path).name) for path in paths),
                f"{name}: test-only artifact reached product binary closure")

    evidence = {
        "format": "phenix.agent-mix-boundaries.v1",
        "production": ["basic", "advanced"],
        "domain_tests": sorted(domains),
        "history_test_artifacts": history,
        "python_source_isolated": True,
        "closure_checked": True,
    }
    Path(output_path).write_text(json.dumps(evidence, sort_keys=True, indent=2))
    print("Agent product/test mix separation verified")


if __name__ == "__main__":
    main()
