from phenix_python import plugin, expose


@plugin(id="example.scalar", version="1.0.0", contract="example.scalar@1")
class Scalar:
    def __init__(self):
        self.calls = 0

    @expose
    def greet(self, request: str) -> str:
        self.calls += 1
        return f"scalar:{request}:{self.calls}"
