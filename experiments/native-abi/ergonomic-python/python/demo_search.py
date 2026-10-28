"""Author-side example. This file contains no native ABI code."""
from dataclasses import dataclass
from phenix import plugin, expose, Host


@dataclass(frozen=True)
class Query:
    text: str
    limit: int


@dataclass(frozen=True)
class SearchResults:
    matches: list[str]
    calls: int


@plugin('example.search')
class Search:
    def __init__(self):
        self.calls = 0

    @expose
    def query(self, request: Query) -> SearchResults:
        self.calls += 1
        words = request.text.split()[:request.limit]
        return SearchResults(words, self.calls)

    @expose
    def enriched_query(self, request: Query, host: Host) -> SearchResults:
        response = host.invoke('example.index/search', {'query': request.text})
        self.calls += 1
        return SearchResults(response['matches'][:request.limit], self.calls)

    @expose
    def broken_result(self, request: Query) -> SearchResults:
        return 123  # Demonstrate runtime result validation.

    @expose
    def fail(self, request: Query) -> SearchResults:
        raise RuntimeError('intentional Python exception')

    def private_method(self):
        return 'not exported'

    @expose
    def capture_host(self, request: Query, host: Host) -> SearchResults:
        self.saved_host = host
        return SearchResults(['saved'], self.calls)

    @expose
    def expired_host(self, request: Query, host: Host) -> SearchResults:
        # Calling a host saved from an earlier request must be denied.
        self.saved_host.invoke('example.index/search', {'query': request.text})
        return SearchResults([], self.calls)
