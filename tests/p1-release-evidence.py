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
NATIVE_SELECTION_FACTS = {
    "selected_model_CAS", "atomic_witness", "restart_reselection",
    "second_client_observes_new_model", "stale_client_selection_rejected",
    "stale_client_typed_conflict", "fresh_menu_preconditions",
    "refreshed_client_successful_switch", "deleted_native_identity_denies_after_restart",
    "fallback_environment_ignored",
}
AUTH_LAUNCHER_FACTS = {
    "default_no_env_acp_initialized", "api_key_stored_in_private_xdg_state",
    "restart_survives", "explicit_opt_out_denied", "relative_xdg_denied",
    "symlink_vault_denied",
}
COMBINED_AUTH_FACTS = {
    "api_key_challenge_cancelled", "oauth_browser_pkce_challenge_cancelled",
    "cross_connection_attempt_denied", "oauth_only_provider_owned_logout_granted",
    "oauth_import_and_api_env_binding_denied", "unauthenticated_native_model_denied",
}
RESTART_CASES = ("untouched", "orphaned_witness", "orphaned_selection", "corrupt_pair")
SHIPPING_GATES = {
    "graph", "basic-agent", "basic-host-smoke", "basic-pending-http-product",
    "acp-codec", "public-sdk", "sqlite-restart", "imports", "native",
}


def require(flag, context):
    if not flag:
        raise AssertionError(context)


def load(path):
    return json.loads(path.read_text())


def verify(host, pending, shipping, launcher, combined):
    summary = load(host / "evidence.json")
    require(summary.get("format") == "phenix.basic.host-smoke.v1", "host evidence format")
    for flag in ("terminal", "jsonl", "acp_v1_session_reload"):
        require(summary.get(flag) is True, "missing host frontend: " + flag)
    require(summary.get("p1_accepted") is False, "premature P1 acceptance declaration")

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

    # Release-gate the native model and auth evidence produced by real ACP
    # product tests. None of these offline fixtures proves a live paid login.
    selected = load(host / "c9-selected/native-model-acceptance.json")
    require(selected.get("format") == "phenix.c9.selected-http.v1", "native selection format")
    for field in NATIVE_SELECTION_FACTS:
        require(selected.get(field) is True, "missing native model guarantee: " + field)
    require(selected.get("production_provider_login") is False,
            "native fixture must not claim production login")

    restarted = load(host / "native-routing-restart.json")
    require(restarted.get("format") == "phenix.native-routing-restart.v1",
            "native routing restart format")
    require(restarted.get("no_successful_provider_selection_claimed") is True,
            "restart fixture may not claim production selection")
    cases = restarted.get("cases", {})
    require(set(cases) == set(RESTART_CASES), "native restart scenarios are missing")
    for name in RESTART_CASES:
        evidence = cases[name]
        require(evidence.get("restarted") is True, "restart not exercised: " + name)
        denied = name != "untouched"
        require(evidence.get("load_denied") is denied
                and evidence.get("prompt_denied") is denied,
                "unsafe restart outcome: " + name)

    installed = load(launcher / "accepted.json")
    for field in AUTH_LAUNCHER_FACTS:
        require(installed.get(field) is True,
                "missing default auth launcher guarantee: " + field)

    coexistence = load(combined / "p1-combined-auth.json")
    require(coexistence.get("installed_connections") == ["openai/api", "openai/chatgpt"],
            "one-product API and ChatGPT connections are missing")
    for field in COMBINED_AUTH_FACTS:
        require(coexistence.get(field) is True,
                "missing combined auth property: " + field)
    require(coexistence.get("live_external_login_verified") is False,
            "offline auth product must not claim external OAuth success")

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
        "verified_native_selection_properties": sorted(NATIVE_SELECTION_FACTS),
        "verified_native_restart_cases": list(RESTART_CASES),
        "verified_auth_launcher_properties": sorted(AUTH_LAUNCHER_FACTS),
        "verified_combined_auth_properties": sorted(COMBINED_AUTH_FACTS),
        "external_required": [
            "official stable-v1 ACP SDK interoperability on final exact head",
            "all final exact-head canonical, flake and generated-DAG checks",
        ],
    }


def self_test():
    from tempfile import TemporaryDirectory

    with TemporaryDirectory() as d:
        base = Path(d)
        for dirname in ("host", "pending", "shipping", "launcher", "combined"):
            (base / dirname).mkdir()
        host, pending, shipping, launcher, combined = (
            base / x for x in ("host", "pending", "shipping", "launcher", "combined")
        )
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
            host / "c9-selected/native-model-acceptance.json": {
                "format": "phenix.c9.selected-http.v1",
                "production_provider_login": False,
                **{x: True for x in NATIVE_SELECTION_FACTS},
            },
            host / "native-routing-restart.json": {
                "format": "phenix.native-routing-restart.v1",
                "no_successful_provider_selection_claimed": True,
                "cases": {name: {
                    "restarted": True, "load_denied": name != "untouched",
                    "prompt_denied": name != "untouched",
                } for name in RESTART_CASES},
            },
            launcher / "accepted.json": {x: True for x in AUTH_LAUNCHER_FACTS},
            combined / "p1-combined-auth.json": {
                "installed_connections": ["openai/api", "openai/chatgpt"],
                "live_external_login_verified": False,
                **{x: True for x in COMBINED_AUTH_FACTS},
            },
            shipping / "evidence.json": {
                "format": "phenix.m1.shipping.v1",
                "gates": sorted(SHIPPING_GATES),
                "realized_gates": {x: x for x in SHIPPING_GATES - {"public-sdk"}},
            },
        }
        for filename, cases in HOST_CASES.items():
            sample[host / filename] = {"cases": sorted(cases)}
        (host / "c9-selected").mkdir()
        for path, value in sample.items():
            path.write_text(json.dumps(value))
        require(verify(host, pending, shipping, launcher, combined)["p1_accepted"] is False, "test baseline")
        for path, field in [
            (host / "evidence.json", "terminal"),
            (pending / "pending-basic-http.json", "no_interrupted_model_replay"),
            (pending / "acp-pending-http.json", "client_eof_cancels_active_graph"),
            (host / "c9-selected/native-model-acceptance.json",
             "stale_client_selection_rejected"),
            (launcher / "accepted.json", "default_no_env_acp_initialized"),
            (combined / "p1-combined-auth.json", "cross_connection_attempt_denied"),
        ]:
            original = copy.deepcopy(sample[path])
            tampered = dict(original)
            tampered[field] = False
            path.write_text(json.dumps(tampered))
            try:
                verify(host, pending, shipping, launcher, combined)
            except AssertionError:
                pass
            else:
                raise AssertionError("tampered evidence was admitted: " + field)
            path.write_text(json.dumps(original))
        restart_path = host / "native-routing-restart.json"
        original_restart = copy.deepcopy(sample[restart_path])
        forged_restart = copy.deepcopy(original_restart)
        forged_restart["cases"]["orphaned_witness"]["prompt_denied"] = False
        restart_path.write_text(json.dumps(forged_restart))
        try:
            verify(host, pending, shipping, launcher, combined)
        except AssertionError:
            pass
        else:
            raise AssertionError("unsafe routing restart was admitted")
        restart_path.write_text(json.dumps(original_restart))

        # An artifact may not declare P1 accepted before external CI
        # and a verified main integration independently establish it.
        declaration = host / "evidence.json"
        forged = dict(sample[declaration], p1_accepted=True)
        declaration.write_text(json.dumps(forged))
        try:
            verify(host, pending, shipping, launcher, combined)
        except AssertionError:
            pass
        else:
            raise AssertionError("host evidence forged premature P1 acceptance")
        declaration.write_text(json.dumps(sample[declaration]))

        missing = host / "jsonl-multisession.json"
        broken = {"cases": list(HOST_CASES[missing.name] - {"caller-tool-limit-no-effect"})}
        missing.write_text(json.dumps(broken))
        try:
            verify(host, pending, shipping, launcher, combined)
        except AssertionError:
            pass
        else:
            raise AssertionError("missing limit case was admitted")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        if len(sys.argv) != 7:
            raise SystemExit("usage: p1-release-evidence.py HOST HTTP SHIPPING AUTH_LAUNCHER COMBINED_AUTH OUTPUT")
        host, pending, shipping, launcher, combined, output = map(Path, sys.argv[1:])
        result = verify(host, pending, shipping, launcher, combined)
        output.mkdir(parents=True, exist_ok=True)
        (output / "evidence.json").write_text(json.dumps(result, sort_keys=True, indent=2))
        print("P1 product release preflight passed; official ACP SDK CI still required")
