"""Bounded Python authoring frontend for the public Phenix SDK.

Author modules are trusted build inputs, not sandboxed code.
"""


def plugin(*, id, version, contract):
    def decorate(cls):
        cls.__phenix_plugin__ = (id, version, contract)
        return cls
    return decorate


def expose(method):
    method.__phenix_exposed__ = True
    return method
