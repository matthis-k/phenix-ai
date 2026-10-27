"""Ordinary Python domain code; no native ABI or kernel implementation."""

from dataclasses import dataclass

def plugin(identity):
    def decorate(cls):
        cls.__phenix_plugin__ = identity
        return cls
    return decorate

def expose(method):
    method.__phenix_exposed__ = True
    return method

@dataclass
class Query:
    text: str
    limit: int = 8

@dataclass
class SearchResults:
    matches: list[str]
    calls: int

class Host:
    """Annotation-only; the native adapter supplies the call-scoped facade."""
    pass

@plugin("example.python-host")
class Search:
    def __init__(self):
        self.calls = 0
        self.previous_host = None

    @expose
    def query(self, request: Query) -> SearchResults:
        if request.text == "raise":
            raise ValueError("intentional Python exception")
        if request.text == "wrong-result":
            return 123
        self.calls += 1
        return SearchResults(request.text.split()[:request.limit], self.calls)

    @expose
    def enriched_query(self, request: Query, host: Host) -> SearchResults:
        if request.text == "stale":
            # Deliberately use a host handle from an earlier completed call.
            return SearchResults([self.previous_host.invoke(
                "example.transform.apply@1/apply", {"query": "stale"}
            )["text"]], self.calls)
        self.previous_host = host
        result = host.invoke(
            "example.transform.apply@1/apply",
            {"query": request.text},
        )
        self.calls += 1
        return SearchResults([result["text"]], self.calls)

    def private(self, request: Query):
        return "must not be callable through the public adapter"
