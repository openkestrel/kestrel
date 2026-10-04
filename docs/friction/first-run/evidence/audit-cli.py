import subprocess,json,re,pathlib
root=pathlib.Path('/private/tmp/kestrel-friction-488')
binary='/Users/jackmoore/orca/workspaces/kestrel/0.4/target/debug/kestrel'
records=[]
def run(args,label=None,data='',endpoint='http://127.0.0.1:17818'):
    p=subprocess.run([binary,'--control-plane',endpoint,*args],input=data,text=True,capture_output=True,cwd=root/'source',timeout=15)
    records.append(dict(label=label or ' '.join(args),args=args,exit=p.returncode,stdout=p.stdout,stderr=p.stderr))
    (root/'evidence'/'cli.json').write_text(json.dumps(records,indent=2))
    return p
leaves=[]
def help(path):
    p=run([*path,'--help'])
    lines=p.stdout.splitlines(); inside=False; children=[]
    for line in lines:
        if line=='Commands:': inside=True;continue
        if inside and line and not line.startswith(' '):break
        if inside and (m:=re.match(r'^  ([a-z][a-z-]+)\s',line)):children.append(m[1])
    if children:
        for child in children:help([*path,child])
    else:leaves.append(path)
help([])
for path in leaves:
    run([*path,'--definitely-invalid'],label='invalid flag '+' '.join(path))
    run(path,label='missing/default '+' '.join(path))
run([],label='bare')
run(['status'],label='offline',endpoint='http://127.0.0.1:17999')
run(['status','--json'],label='fresh-status-json')
run(['organization','declare','audit'])
for args in [ ['project','list'],['agent','list'],['credential','list'],['profile','list'],['integration','list'],['event','list'],['trigger','list'],['workspace','list'],['session','list'],['instance','list'],['queue'] ]:run(args,label='empty '+' '.join(args))
for args in [['workspace','show','missing'],['session','show','missing'],['trigger','show','missing'],['instance','release','missing'],['event','show','missing'],['agent','model','missing','unknown'],['profile','set','missing','--variable','AUDIT_LOGIN']]:
    run(args,label='not-found '+' '.join(args),data='fake-audit-value\n')
run(['agent','declare','audit-agent','--harness','opencode'])
run(['project','declare','audit-project','--repository','https://github.com/octocat/Hello-World.git','--branch','master'])
run(['start','--organization','audit','--project','audit-project','--agent','audit-agent','--repository','https://github.com/octocat/Hello-World.git','--branch','master','--brief','Reply with audit complete. Do not edit files.','--yes','--json'],label='cli-session-start')
print(json.dumps({'help_leaf_count':len(leaves),'invocations':len(records),'start':records[-1]},indent=2))
