from dataclasses import dataclass
from phenix_python import plugin, expose


@dataclass
class Text:
    text: str


@plugin(id="example.python-greeter", version="1.0.0", contract="example.greeter@1")
class Greeter:
    def __init__(self):
        self.calls = 0

    @expose
    def greet(self, request: Text) -> Text:
        self.calls += 1
        if request.text == "malformed":
            return Text(42)
        return Text(f"Hello {request.text} ({self.calls})")
