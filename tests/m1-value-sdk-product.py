"""Typed SDK fixture through the real immutable-package CLI boundary."""
import copy,json,subprocess,sys
from pathlib import Path
kernel,native,output=map(Path,sys.argv[1:4]);output.mkdir(parents=True,exist_ok=True)
policy=json.loads((native/'composition.json').read_text())
def request(command):
 b=command.encode();return b'PV\x02\0\x05'+len(b).to_bytes(4,'big')+b
cases=[]
for command,expected in [('typed',[1,2,2,1]),('denied',[1])]:
 selected=copy.deepcopy(policy)
 if command=='denied':selected['composition']['grants']=[g for g in selected['composition']['grants'] if g['operation']!='set']
 deployment=output/'deployment.json';deployment.write_text(json.dumps(selected));input_path=output/'input.bin';input_path.write_bytes(request(command))
 result=json.loads(subprocess.check_output([str(kernel),'invoke',str(deployment),str(input_path)],text=True))
 assert result['outcome']=={'kind':'success','value':expected},result
 assert result['live_state']['cells']==0,result
 assert result['live_state']['subscriptions']==0,result
 cases.append({'case':command,'result':result})
(output/'product.json').write_text(json.dumps({'all_passed':True,'cases':cases},indent=2));print('S3a packaged typed SDK CLI passed')
