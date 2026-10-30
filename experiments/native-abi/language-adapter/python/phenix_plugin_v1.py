"""A plain Python domain plugin. No ABI declarations or native code."""

class Plugin:
    def __init__(self):
        self.calls = 0

    def invoke(self, query: str, multiplier: int, maximum: int, mode: int):
        if query == "raise":
            raise RuntimeError("intentional Python domain failure")
        if query == "wrong-type":
            return ("not-an-int", 0)
        if query == "overflow":
            return (2 ** 100, 1)
        words = min(len(query.split()), maximum)
        self.calls += 1
        return (words * multiplier + self.calls, self.calls)
