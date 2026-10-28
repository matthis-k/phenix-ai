"""Built twice with different identities/tokens by verify.py."""
from dataclasses import dataclass
from phenix_python import plugin, expose
import os
import sys
from pathlib import Path
import time

TOKEN = "A"

# Both independent images must be executing module initialization simultaneously.
# sleep releases the GIL; a timeout fails instead of silently skipping overlap.
if __name__.startswith("_phenix_instance_"):
    barrier = Path(os.environ["PHENIX_INIT_BARRIER"])
    (barrier / TOKEN).touch()
    deadline = time.monotonic() + 10
    while not all((barrier / name).exists() for name in ("A", "B")):
        if time.monotonic() > deadline:
            raise RuntimeError("concurrent initialization did not overlap")
        time.sleep(0.01)


@dataclass
class Operation:
    text: str


@dataclass
class ValueSchema:
    text: str


@plugin(id="example.concurrent-a", version="1.0.0", contract="example.concurrent@1")
class Greeter:
    def __init__(self):
        self.calls = 0

    @expose
    def greet(self, request: Operation) -> ValueSchema:
        if sys.modules.get(__name__) is not REPLACEMENT:
            raise RuntimeError("generated cleanup removed another module entry")
        self.calls += 1
        if self.calls == 2:
            del sys.modules[__name__]
        return ValueSchema(f"{TOKEN}:{request.text}:{self.calls}")


if __name__.startswith("_phenix_instance_"):
    REPLACEMENT = object()
    sys.modules[__name__] = REPLACEMENT
