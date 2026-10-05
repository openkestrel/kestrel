import json,pathlib,subprocess,time,os
root=pathlib.Path(__file__).resolve().parents[3]
output=root/'docs/measurements/contributor-baseline'
results=[]
base=['mise','exec','--','env','CARGO_BUILD_JOBS=2','GIT_CONFIG_GLOBAL=/dev/null','GIT_CONFIG_NOSYSTEM=1','cargo']
test=['test','--locked','-p','kestrel','--test','work','a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance','--','--exact']
def run(name,args):
 started=time.time(); t=time.perf_counter()
 with (output/(name+'.log')).open('w') as f:
  p=subprocess.run(base+args,cwd=root,stdout=f,stderr=subprocess.STDOUT)
 result={'name':name,'command':base+args,'started_unix':started,'wall_seconds':round(time.perf_counter()-t,3),'exit_code':p.returncode}
 results.append(result); (output/'local-followups.json').write_text(json.dumps(results,indent=2)+'\n'); print(json.dumps(result),flush=True)
 if p.returncode:raise SystemExit(p.returncode)
run('first-test',test)
run('warm-test',test)
run('noop-build',['build','--locked','-p','kestrel'])
source=root/'crates/kestrel/src/lib.rs'; original=source.read_bytes()
try:
 source.write_bytes(original+b'\n')
 run('whitespace-edit-build',['build','--locked','-p','kestrel'])
finally:
 source.write_bytes(original)
