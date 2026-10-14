"""Check packaged P1 evidence without conflating it with external ACP SDK CI.

Run against realized Nix output paths. This is a release *preflight*:
only the combined exact-head workflow gate may declare P1 accepted.
"""
import copy
import json
from pathlib import Path
import sys

HOST_CASES = {
    "jsonl-multisession.json": {
        "interleaved-session-prompts",
        "independent-observer-checkpoints",
        "process-restart-continuation",
        "failed-tool-model-continuation",
        "same-process-model-failure-continuation",
        "caller-tool-limit-no-effect",
        "caller-model-turn-limit-continuation",
        "limit-recovery-session-isolation",
    },
    "terminal-sessions.json": {
        "explicit-session-selection",
        "independent-session-restart",
        "failed-tool-model-continuation",
        "same-process-model-failure-continuation",
    },
    "acp-product.json": {
        "process-restart-load",
        "two-session-isolation",
        "two-tool-acp-lifecycle-ordered",
        "oversized-stdio-frame-rejected",
        "v1-resource-link-preserved-without-dereference",
        "invalid-prompt-params-rejected-before-admission",
    },
}
HTTP_FACTS = {
    "streamed_text_exactly_once",
    "pending_basic_agent",
    "active_cancellation_tested",
    "durable_resume_after_cancel",
    "queued_followup_survives_cancel",
    "queued_other_session_isolated",
    "session_file_lock_enforced",
    "durable_owner_marker_matched",
    "no_interrupted_model_replay",
    "tampered_marker_recovery_rejected",
    "interrupted_recovery_journal_write_repaired",
    "post_journal_marker_replay_deduplicated",
}
ACP_HTTP_FACTS = {
    "live_delta",
    "active_cancellation",
    "same_session_continuation",
    "active_cancel_requests_rejected_immediately",
    "client_eof_cancels_active_graph",
}
SHIPPING_GATES = {
    "graph", "basic-agent", "basic-host-smoke", "basic-pending-http-product",
    "acp-codec", "public-sdk", "sqlite-restart", "imports", "native",
}


def require(flag, context):
    if not flag:
        raise AssertionError(context)


def load(path):
    return json.loads(path.read_text())


def verify(host, pending, shipping):
    summary = load(host / "evidence.json")
    require(summary.get("format") == "phenix.basic.host-smoke.v1", "host evidence format")
    for flag in ("terminal", "jsonl", "acp_v1_session_reload"):
        require(summary.get(flag) is True, "missing host frontend: " + flag)
    require(isinstance(summary.get("p1_accepted"), bool), "missing explicit P1 flag")

    for filename, cases in HOST_CASES.items():
        result = load(host / filename)
        require(cases <= set(result.get("cases", [])), "missing product cases: " + filename)
    observer = load(host / "observer-product.json")
    require(observer.get("delivery") == "at-least-once", "observer delivery guarantee")
    require("failed-pipe-write-replay" in observer.get("cases", []), "failed delivery replay")

    model = load(pending / "pending-basic-http.json")
    require(model.get("format") == "phenix.basic.pending-http-product.v1", "HTTP evidence format")
    for field in HTTP_FACTS:
        require(model.get(field) is True, "missing HTTP property: " + field)
    require(model.get("tool_calls") == 2, "missing multi-tool HTTP continuation")

    acp = load(pending / "acp-pending-http.json")
    require(acp.get("format") == "phenix.acp.pending-http-product.v1", "ACP HTTP evidence format")
    for field in ACP_HTTP_FACTS:
        require(acp.get(field) is True, "missing ACP property: " + field)

    m1 = load(shipping / "evidence.json")
    require(m1.get("format") == "phenix.m1.shipping.v1", "M1 shipping evidence format")
    require(SHIPPING_GATES <= set(m1.get("gates", [])), "missing M1 shipping gates")
    require(
        SHIPPING_GATES - {"public-sdk"} <= set(m1.get("realized_gates", {})),
        "missing realized M1 evidence",
    )
    return {
        "format": "phenix.p1.release-preflight.v1",
        "p1_accepted": False,
        "basic_frontends": ["terminal", "jsonl", "acp-v1"],
        "verified_host_cases": {k: sorted(v) for k, v in sorted(HOST_CASES.items())},
        "verified_http_properties": sorted(HTTP_FACTS),
        "verified_acp_http_properties": sorted(ACP_HTTP_FACTS),
        "verified_shipping_gates": sorted(SHIPPING_GATES),
        "external_required": [
            "official stable-v1 ACP SDK interoperability on final exact head",
            "all final exact-head canonical, flake and generated-DAG checks",
        ],
    }


def self_test():
    from tempfile import TemporaryDirectory

    with TemporaryDirectory() as d:
        base = Path(d)
        for dirname in ("host", "pending", "shipping"):
            (base / dirname).mkdir()
        host, pending, shipping = (base / x for x in ("host", "pending", "shipping"))
        sample = {
            host / "evidence.json": {
                "format": "phenix.basic.host-smoke.v1", "terminal": True,
                "jsonl": True, "acp_v1_session_reload": True, "p1_accepted": False,
            },
            host / "observer-product.json": {
                "delivery": "at-least-once", "cases": ["failed-pipe-write-replay"],
            },
            pending / "pending-basic-http.json": {
                "format": "phenix.basic.pending-http-product.v1",
                "tool_calls": 2, **{x: True for x in HTTP_FACTS},
            },
            pending / "acp-pending-http.json": {
                "format": "phenix.acp.pending-http-product.v1",
                **{x: True for x in ACP_HTTP_FACTS},
            },
            shipping / "evidence.json": {
                "format": "phenix.m1.shipping.v1",
                "gates": sorted(SHIPPING_GATES),
                "realized_gates": {x: x for x in SHIPPING_GATES - {"public-sdk"}},
            },
        }
        for filename, cases in HOST_CASES.items():
            sample[host / filename] = {"cases": sorted(cases)}
        for path, value in sample.items():
            path.write_text(json.dumps(value))
        require(verify(host, pending, shipping)["p1_accepted"] is False, "test baseline")
        for path, field in [
            (host / "evidence.json", "terminal"),
            (pending / "pending-basic-http.json", "no_interrupted_model_replay"),
            (pending / "acp-pending-http.json", "client_eof_cancels_active_graph"),
        ]:
            original = copy.deepcopy(sample[path])
            tampered = dict(original)
            tampered[field] = False
            path.write_text(json.dumps(tampered))
            try:
                verify(host, pending, shipping)
            except AssertionError:
                pass
            else:
                raise AssertionError("tampered evidence was admitted: " + field)
            path.write_text(json.dumps(original))
        missing = host / "jsonl-multisession.json"
        broken = {"cases": list(HOST_CASES[missing.name] - {"caller-tool-limit-no-effect"})}
        missing.write_text(json.dumps(broken))
        try:
            verify(host, pending, shipping)
        except AssertionError:
            pass
        else:
            raise AssertionError("missing limit case was admitted")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        if len(sys.argv) != 5:
            raise SystemExit("usage: p1-release-evidence.py HOST HTTP SHIPPING OUTPUT")
        host, pending, shipping, output = map(Path, sys.argv[1:])
        result = verify(host, pending, shipping)
        output.mkdir(parents=True, exist_ok=True)
        (output / "evidence.json").write_text(json.dumps(result, sort_keys=True, indent=2))
        print("P1 product release preflight passed; official ACP SDK CI still required")
