"""Black-box P2-0 composition guard, run against packaged executable presets.

The Advanced product in P2-0 is a *scaffold*, not the accepted P2 release.
Later P2 plugins extend the composition; the Basic product must never change.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

(
    kernel_exe, basic_exe, basic_test_exe, advanced_exe, advanced_test_exe,
    basic_path, basic_test_path, advanced_path, advanced_test_path,
    foundation_path, destination,
) = sys.argv[1:]
out = Path(destination)
out.mkdir(parents=True, exist_ok=True)

read = lambda path: json.loads(Path(path).read_text())
basic, basic_test, advanced, advanced_test = (
    read(path) for path in
    (basic_path, basic_test_path, advanced_path, advanced_test_path)
)
foundation = read(foundation_path)
assert foundation["format"] == "phenix.p2.composition-foundation.v1"
assert foundation["status"] == "scaffold"
assert foundation["p2_accepted"] is False

for binary in (kernel_exe, basic_exe, basic_test_exe, advanced_exe, advanced_test_exe):
    assert Path(binary).is_file(), binary
    assert os.access(binary, os.X_OK), binary

for spec in (basic, basic_test, advanced, advanced_test):
    assert spec["format"] == "phenix.composition.v2"
    assert spec["entry"] == basic["entry"]
    assert len(spec["artifacts"]) == len(set(spec["artifacts"]))
    assert len(spec["definitions"]) == len(set(spec["definitions"]))
    keys = [(b["caller"], b["slot"], b["index"]) for b in spec["slots"]]
    assert len(keys) == len(set(keys)), keys

def model(spec):
    matching = [
        b for b in spec["slots"]
        if (b["caller"], b["slot"]) == ("phenix.basic.agent", "Model")
    ]
    assert len(matching) == 1, matching
    return matching[0]["provider"]

assert model(basic) == "phenix.model.scripted"
assert model(basic_test) == "phenix.model.history-test"
assert model(advanced) == "phenix.advanced.model-adapter"
assert model(advanced_test) == "phenix.advanced.model-adapter"

def upstream(spec):
    xs = [b for b in spec["slots"]
          if (b["caller"], b["slot"]) == ("phenix.advanced.model-adapter", "Upstream")]
    assert len(xs) == 1, xs
    return xs[0]["provider"]

assert upstream(advanced) == model(basic)
assert upstream(advanced_test) == model(basic_test)

# The P2 composition extends P1; it does not secretly override the agent,
# storage, journal, session, grants, or the public frontend contracts.
for field in ("entry", "resource"):
    assert advanced[field] == basic[field], field
assert advanced["artifacts"][:len(basic["artifacts"])] == basic["artifacts"]
assert advanced["definitions"][:len(basic["definitions"])] == basic["definitions"]
assert advanced_test["artifacts"][:len(advanced["artifacts"])] == advanced["artifacts"]
assert advanced_test["definitions"] == advanced["definitions"]
assert len(advanced_test["artifacts"]) == len(advanced["artifacts"]) + 1
for key in ("grants", "authority", "resource", "entry", "excluded", "substitutions"):
    assert advanced_test[key] == advanced[key], key
assert [
    slot for slot in advanced_test["slots"]
    if (slot["caller"], slot["slot"]) != ("phenix.advanced.model-adapter", "Upstream")
] == [
    slot for slot in advanced["slots"]
    if (slot["caller"], slot["slot"]) != ("phenix.advanced.model-adapter", "Upstream")
]
assert advanced_test["grants"] == advanced["grants"]
assert not any("history-test" in artifact for artifact in advanced["artifacts"])
assert any("history-test" in artifact for artifact in advanced_test["artifacts"])

def run(binary, mode, payload, file_name):
    env = dict(os.environ)
    env["PHENIX_BASIC_SQLITE_PATH"] = str(out / file_name)
    result = subprocess.run(
        [binary, mode], input=payload, text=True, capture_output=True,
        env=env, timeout=45,
    )
    assert result.returncode == 0, (binary, result.stdout, result.stderr)
    return result.stdout

for name, binary in (("basic", basic_exe), ("advanced", advanced_exe)):
    text = run(binary, "terminal", "hello\n", name + ".sqlite")
    assert "answer: mock:hello" in text, text

payload = "".join(
    json.dumps({"type": "prompt", "session": "s1", "text": text}) + "\n"
    for text in ("remember:color=violet", "recall:color")
)
for name, binary in (("basic-test", basic_test_exe),
                     ("advanced-test", advanced_test_exe)):
    stdout = run(binary, "jsonl", payload, name + ".sqlite")
    events = [json.loads(line) for line in stdout.splitlines()]
    actual = [e["text"] for e in events if e["kind"] == "Answer"]
    assert actual == ["stored:color=violet", "recalled:color=violet"], actual
    restarted = run(
        binary, "jsonl",
        json.dumps({"type": "prompt", "session": "s1", "text": "recall:color"}) + "\n",
        name + ".sqlite",
    )
    resumed = [json.loads(line) for line in restarted.splitlines()]
    assert "recalled:color=violet" in [
        event["text"] for event in resumed if event["kind"] == "Answer"
    ], resumed

(out / "p2-composition-foundation.json").write_text(json.dumps({
    "format": "phenix.p2.composition-foundation-evidence.v1",
    "status": "scaffold",
    "p2_accepted": False,
    "cases": [
        "five-executable-presets",
        "immutable-composition-v2",
        "single-selected-model-per-product",
        "basic-composition-not-mutated",
        "advanced-extends-basic",
        "test-model-only-in-verification",
        "production-and-verification-grants-equal",
        "real-terminal-basic-and-advanced",
        "real-jsonl-history-and-restart",
    ],
    "accepted": True,
}, indent=2, sort_keys=True))
