"""Independent canonical wire/hash regression, without executing a plugin."""
import copy, hashlib, json, pathlib, struct
DOMAIN = b"phenix.generation.v1\0"
def encode(v):
    n = lambda x: struct.pack('>I', x)
    if v is None: return b'\0'
    if isinstance(v, bool): return bytes([2 if v else 1])
    if isinstance(v, int): return b'\4' + struct.pack('>Q', v)
    if isinstance(v, str):
        b = v.encode(); return b'\5' + n(len(b)) + b
    if isinstance(v, list): return b'\7' + n(len(v)) + b''.join(encode(x) for x in v)
    if isinstance(v, dict): return b'\10' + n(len(v)) + b''.join(n(len(k.encode())) + k.encode() + encode(v[k]) for k in sorted(v))
    raise TypeError(v)
def digest(v): return hashlib.sha256(DOMAIN + encode(v)).hexdigest()
if __name__ == '__main__':
    import sys
    fixture = json.loads(pathlib.Path(sys.argv[1]).read_text())
    assert encode(fixture['tuple']).hex() == fixture['bytes_hex']
    assert digest(fixture['tuple']) == fixture['sha256']
    for key in fixture['tuple']:
        variant = copy.deepcopy(fixture['tuple'])
        variant[key] = ['changed']
        assert digest(variant) != fixture['sha256'], key
    assert digest(dict(reversed(list(fixture['tuple'].items())))) == fixture['sha256']
    print(json.dumps(dict(format='phenix.generation.identity.v1', sha256=fixture['sha256'], tested_fields=list(fixture['tuple']))))
