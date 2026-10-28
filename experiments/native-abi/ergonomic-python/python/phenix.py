"""Tiny language SDK proof of concept; domain authors see only ordinary Python."""
from dataclasses import fields, is_dataclass
from inspect import signature
from types import UnionType
from typing import Any, Union, get_args, get_origin, get_type_hints
import json


class ValidationError(ValueError):
    pass


class Host:
    """Scoped, call-local handle. The native adapter supplies the callback."""
    def __init__(self, invoke_callback):
        self._invoke_callback = invoke_callback

    def invoke(self, operation: str, request: dict) -> dict:
        return self._invoke_callback(operation, request)


def expose(function):
    function.__phenix_expose__ = True
    return function


def plugin(identifier: str):
    if not isinstance(identifier, str) or not identifier or '/' in identifier:
        raise ValidationError('invalid plugin identifier')

    def decorate(cls):
        exports = {}
        for name, raw in vars(cls).items():
            if getattr(raw, '__phenix_expose__', False):
                if name.startswith('_'):
                    raise ValidationError('private methods cannot be exported')
                hints = get_type_hints(raw)
                sig = signature(raw)
                params = list(sig.parameters.values())
                if not params or params[0].name != 'self' or 'return' not in hints:
                    raise ValidationError(f'{name} needs self and a return type')
                nonhost = [p for p in params[1:] if hints.get(p.name) is not Host]
                if len(nonhost) != 1 or nonhost[0].name not in hints:
                    raise ValidationError(f'{name} requires exactly one typed request parameter')
                for p in params[1:]:
                    if p.name not in hints:
                        raise ValidationError(f'unknown parameter type: {p.name}')
                exports[name] = (hints, sig, nonhost[0].name)
        if not exports:
            raise ValidationError('plugin exports no methods')
        cls.__phenix_id__ = identifier
        cls.__phenix_exports__ = exports
        return cls
    return decorate


def check(value, typ, path='value'):
    origin = get_origin(typ)
    args = get_args(typ)
    if typ is Any:
        raise ValidationError(f'{path}: Any is not a portable contract type')
    if typ is type(None):
        if value is not None:
            raise ValidationError(f'{path}: expected null')
        return None
    if typ in (str, int, bool, float):
        if type(value) is not typ:
            raise ValidationError(f'{path}: expected {typ.__name__}')
        if typ is int and not (-2**63 <= value < 2**63):
            raise ValidationError(f'{path}: signed integer exceeds ABI range')
        return value
    if origin is list:
        if not isinstance(value, list) or len(args) != 1:
            raise ValidationError(f'{path}: expected typed list')
        return [check(x, args[0], f'{path}[{i}]') for i, x in enumerate(value)]
    if origin is dict:
        if not isinstance(value, dict) or len(args) != 2 or args[0] is not str:
            raise ValidationError(f'{path}: expected string-keyed dictionary')
        if not all(isinstance(k, str) for k in value):
            raise ValidationError(f'{path}: dictionary keys must be strings')
        return {k: check(v, args[1], f'{path}.{k}') for k, v in value.items()}
    if origin in (UnionType, Union):
        for branch in args:
            try:
                return check(value, branch, path)
            except ValidationError:
                continue
        raise ValidationError(f'{path}: value does not match any variant')
    if isinstance(typ, type) and is_dataclass(typ):
        if not isinstance(value, dict):
            raise ValidationError(f'{path}: expected record')
        flds = fields(typ)
        names = {f.name for f in flds}
        if names != value.keys():
            raise ValidationError(f'{path}: missing or unknown fields')
        hints = get_type_hints(typ)
        return typ(**{f.name: check(value[f.name], hints[f.name], f'{path}.{f.name}') for f in flds})
    raise ValidationError(f'{path}: unsupported contract type {typ}')


def outgoing(value, typ):
    if isinstance(typ, type) and is_dataclass(typ):
        if not isinstance(value, typ):
            raise ValidationError('result: incorrect record type')
        data = {f.name: getattr(value, f.name) for f in fields(value)}
        hints = get_type_hints(typ)
        return {k: outgoing(v, hints[k]) for k, v in data.items()}
    origin = get_origin(typ)
    args = get_args(typ)
    if origin is list:
        if not isinstance(value, list):
            raise ValidationError('result: expected list')
        return [outgoing(v, args[0]) for v in value]
    if origin is dict:
        if not isinstance(value, dict) or args[0] is not str:
            raise ValidationError('result: expected string-keyed map')
        return {k: outgoing(v, args[1]) for k, v in value.items()}
    if origin in (UnionType, Union):
        for branch in args:
            try:
                return outgoing(value, branch)
            except ValidationError:
                continue
        raise ValidationError('result: invalid union')
    return check(value, typ, 'result')


def dispatch(instance, method_name, json_request: str, host: Host):
    """Called only by native adapter; return (status, JSON body)."""
    try:
        exported = instance.__class__.__phenix_exports__
        if method_name not in exported:
            raise ValidationError('method not publicly exported')
        hints, sig, param = exported[method_name]
        request = json.loads(json_request)
        typed_request = check(request, hints[param], 'request')
        kwargs = {param: typed_request}
        for p in list(sig.parameters.values())[1:]:
            if hints[p.name] is Host:
                kwargs[p.name] = host
        result = getattr(instance, method_name)(**kwargs)
        return (0, json.dumps(outgoing(result, hints['return']), separators=(',', ':')))
    except (ValidationError, json.JSONDecodeError) as error:
        return (1, json.dumps({'code': 'schema_rejected', 'message': str(error)}))
    except PermissionError as error:
        return (3, json.dumps({'code': 'denied', 'message': str(error)}))
    except Exception as error:
        return (2, json.dumps({'code': 'domain_failure', 'message': str(error)}))


def schema_of(typ):
    """POC structured contract export; production would emit ValueSchema."""
    if typ in (str, int, bool, float):
        return typ.__name__
    origin = get_origin(typ)
    args = get_args(typ)
    if origin is list and len(args) == 1:
        return {'list': schema_of(args[0])}
    if origin is dict and len(args) == 2 and args[0] is str:
        return {'map': schema_of(args[1])}
    if origin in (UnionType, Union):
        return {'union': [schema_of(t) for t in args]}
    if isinstance(typ, type) and is_dataclass(typ):
        hints = get_type_hints(typ)
        return {'record': {f.name: schema_of(hints[f.name]) for f in fields(typ)}}
    raise ValidationError(f'unsupported public signature {typ}')


def manifest(cls):
    result = {'plugin': cls.__phenix_id__, 'operations': {}}
    for name, (hints, sig, param) in sorted(cls.__phenix_exports__.items()):
        result['operations'][name] = {'request': schema_of(hints[param]), 'response': schema_of(hints['return'])}
    return json.dumps(result, sort_keys=True, separators=(',', ':'))
