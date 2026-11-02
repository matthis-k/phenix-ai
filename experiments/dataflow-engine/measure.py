"""Reproduce bounded measurements; no engine-equivalence claim."""
import json, os, pathlib, platform, subprocess, time
ROOT=pathlib.Path(__file__).resolve().parents[2]
HERE=pathlib.Path(__file__).resolve().parent
CARGO=os.environ.get('CARGO',str(pathlib.Path.home()/'.cargo/bin/cargo'))
TOOLCHAIN=os.environ.get('PHENIX_TOOLCHAIN','+1.96.1')
cmd=[CARGO,TOOLCHAIN]
def run(args):
    return subprocess.check_output(args,cwd=ROOT,text=True,stderr=subprocess.DEVNULL)
def closure(meta,name):
    nodes={n['id']:n for n in meta['resolve']['nodes']}; pkgs={p['id']:p for p in meta['packages']}
    ids=[p['id'] for p in meta['packages'] if p['name']==name];todo=ids[:];seen=set()
    while todo:
        i=todo.pop()
        if i in seen:continue
        seen.add(i);todo.extend(nodes[i]['dependencies'])
    return {'version':[pkgs[i]['version'] for i in ids], 'external_packages_including_self':sum(pkgs[i]['source'] is not None for i in seen),'total_packages_including_self':len(seen)}
meta=json.loads(run(cmd+['metadata','--offline','--locked','--format-version','1','--filter-platform','x86_64-unknown-linux-gnu','--manifest-path',str(HERE/'Cargo.toml')]))
components=json.loads(run(cmd+['metadata','--offline','--locked','--format-version','1','--filter-platform','x86_64-unknown-linux-gnu','--manifest-path',str(HERE.parent/'dataflow-components/Cargo.toml')]))
result={'toolchain':run(cmd+['--version']).strip(),'rustc':run([str(pathlib.Path(CARGO).with_name('rustc')),TOOLCHAIN,'--version']).strip(),'platform':platform.platform(),'cpu':next(l.split(':',1)[1].strip() for l in pathlib.Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name')),'profile':{'release':True,'lto':'thin','codegen_units':1},'n':2000,'dependency_closures':{},'rust_source_lines':{},'measurement_limits':['Candidates: caller-issued identity, Rust in-process dispatch and eager main fixture; native: actual root admission, structural ABI and terminal merge. Not equivalent engine semantics.','Tracing and retained per-occurrence state enabled. Allocations are cumulative calls/bytes; not peak or per-node allocation guarantees.','Timely uses 32 worker steps/sample; DFIR run_available_sync; driver overhead included.','No independent repetitions, CPU pinning or statistical confidence interval. No performance ranking justified.']}
for name in ['timely','dfir_rs','tokio-util','phenix-kernel']:result['dependency_closures'][name]=closure(meta,name)
for name in ['bevy_ecs','petgraph']:result['dependency_closures'][name]=closure(components,name)
for p in [*HERE.rglob('*.rs'),* (HERE.parent/'dataflow-components/src').rglob('*.rs'),ROOT/'crates/phenix-kernel/src/graph/v2/execute.rs',ROOT/'crates/phenix-kernel/src/graph/v2.rs',ROOT/'crates/phenix-kernel/src/dag.rs']:
    if 'target' in p.parts:continue
    result['rust_source_lines'][str(p.relative_to(ROOT))]=len(p.read_text().splitlines())
env=dict(os.environ,PHENIX_BENCH_N='2000')
result['candidate_samples']=[json.loads(l) for l in subprocess.check_output([str(HERE/'target/release/benchmark')],env=env,text=True).splitlines()]
result['native_smoke']=subprocess.check_output([str(HERE/'target/release/dataflow-native-host'),'--package',str(HERE/'target/release/libdataflow_native_plugin.so'),'/tmp/phenix-measured-package'],env=env,text=True).splitlines()
subprocess.run(cmd+['build','--offline','--locked','--release','--manifest-path',str(HERE/'Cargo.toml'),'--workspace'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
start=time.perf_counter();subprocess.run(cmd+['build','--offline','--locked','--release','--manifest-path',str(HERE/'Cargo.toml'),'--workspace'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);result['noop_release_build_seconds']=time.perf_counter()-start
# An mtime-only touch measures Cargo's rebuild response, without modifying code content.
os.utime(HERE/'fixture/src/lib.rs',None)
start=time.perf_counter();subprocess.run(cmd+['build','--offline','--locked','--release','--manifest-path',str(HERE/'Cargo.toml'),'--workspace'],cwd=ROOT,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);result['fixture_touch_release_rebuild_seconds']=time.perf_counter()-start
(HERE/'measurements.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
