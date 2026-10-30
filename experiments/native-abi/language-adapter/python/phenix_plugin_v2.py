"""Updated Python implementation imported under a new module name.

Old instances continue to use v1; new instances can choose v2 without
reloading an active native image or changing the public ABI.
"""

class Plugin:
    def __init__(self):
        self.calls = 0

    def invoke(self, query: str, multiplier: int, maximum: int, mode: int):
        if query == "raise":
            raise RuntimeError("intentional v2 Python failure")
        words = min(len(query.split()), maximum)
        self.calls += 1
        return (words * multiplier + 100 + self.calls, self.calls)
