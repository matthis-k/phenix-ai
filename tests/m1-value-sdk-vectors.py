"""Independent verification of compile-time schema bytes and hash identities."""
import hashlib,json,subprocess,sys
from pathlib import Path
probe,fixtures,output=map(Path,sys.argv[1:4])
actual=json.loads(subprocess.check_output([str(probe)],text=True))
expected={v["name"]:v for v in json.loads(fixtures.read_text())["schemas"]}
def encode(v):
 if isinstance(v,str):
  b=v.encode();return b'\x05'+len(b).to_bytes(4,'big')+b
 if isinstance(v,bytes):return b'\x06'+len(v).to_bytes(4,'big')+v
 if isinstance(v,list):return b'\x07'+len(v).to_bytes(4,'big')+b''.join(map(encode,v))
 return b'\x08'+len(v).to_bytes(4,'big')+b''.join(len(k.encode()).to_bytes(4,'big')+k.encode()+encode(x) for k,x in sorted(v.items()))
def ref(schema):return ["Ref",hashlib.sha256(b'phenix.schema.v2\0'+encode(schema)).digest()]
long=["Struct",{"a_abcdefghijklmnopqrstuvwxyz":["List","String"],"z_abcdefghijklmnopqrstuvwxyz":["Option","U64"]}]
extra={"i64":"I64","bytes":"Bytes","empty-record":["Struct",{}],"nested-ref":ref(["List",ref(["Option","String"])]),"long-record":long,"ref-long-record":ref(long)}
for name,schema in extra.items():
 b=encode(schema);expected[name]={"hex":b.hex(),"id":hashlib.sha256(b'phenix.schema.v2\0'+b).hexdigest()}
assert len(actual)==len(expected)==15
for item in actual:
 e=expected.pop(item['name']);assert item['hex']==e['hex'],item;assert item['id']==e['id'],item
assert not expected
output.mkdir(parents=True,exist_ok=True);(output/'static-schemas.json').write_text(json.dumps({'all_passed':True,'cases':actual},indent=2))
print('S3a 15 independent constant schema vectors passed')
