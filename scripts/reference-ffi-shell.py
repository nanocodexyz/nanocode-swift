import ctypes,json,pathlib,sys,threading,time
root=pathlib.Path(sys.argv[3]).resolve()
library=ctypes.CDLL(sys.argv[1])
Callback=ctypes.CFUNCTYPE(None,ctypes.c_void_p,ctypes.POINTER(ctypes.c_uint8),ctypes.c_size_t)
library.nanocode_start.restype=ctypes.c_void_p
library.nanocode_start.argtypes=[ctypes.c_char_p,ctypes.c_size_t,Callback,ctypes.c_void_p]
library.nanocode_send.argtypes=[ctypes.c_void_p,ctypes.c_char_p,ctypes.c_size_t]
library.nanocode_stop.argtypes=[ctypes.c_void_p]
ready=threading.Event(); replies={}; records=[]
@Callback
def receive(context,data,size):
    value=json.loads(ctypes.string_at(data,size))
    if 'reply' in value: replies[value['reply']]=value;ready.set()
    if 'fatal' in value: print(json.dumps(value),flush=True);ready.set()
workspace=root/'ffi-probe-workspace';workspace.mkdir(exist_ok=True)
config=json.dumps({'data_dir':str(root/'ffi-probe-data'),'shell':sys.argv[2]}).encode()
runtime=library.nanocode_start(config,len(config),receive,None)
if not runtime: raise RuntimeError('nanocode_start returned null')
cases=[('shell',"printf 'b\na\n' | sort"),('python',"python3 -c 'import sys; print(sys.version.split()[0], sys.platform)'"),('numpy',"python3 -c 'import numpy as np; print(int(np.arange(10).sum()))'"),('plot',"python3 -c 'import matplotlib; matplotlib.use(\"Agg\"); import matplotlib.pyplot as plt; plt.plot([1,2,3],[1,4,9]); plt.savefig(\"probe.png\"); plt.close(); import os; print(os.path.getsize(\"probe.png\") > 100)'"),('subprocess',"python3 -c 'import subprocess; print(subprocess.check_output([\"echo\",\"native-child\"], text=True).strip())'")]
try:
    for i,(name,script) in enumerate(cases,1):
        ready.clear();started=time.monotonic()
        body=json.dumps({'id':i,'command':{'shell_run':{'script':script,'cwd':str(workspace)}}}).encode()
        library.nanocode_send(runtime,body,len(body))
        if not ready.wait(120): raise TimeoutError(name)
        reply=replies.get(i,{})
        records.append({'name':name,'seconds':time.monotonic()-started,'reply':reply})
        print(json.dumps(records[-1]),flush=True)
finally: library.nanocode_stop(runtime)
(root/'ffi-shell-probe-result.json').write_text(json.dumps(records,indent=2))
expected={'shell':'a\nb\n','numpy':'45\n','plot':'True\n','subprocess':'native-child\n'}
for item in records:
    result=item['reply'].get('ok',{})
    if result.get('exit_code') != 0: raise AssertionError(item)
    if item['name'] in expected and result.get('stdout') != expected[item['name']]: raise AssertionError(item)
    if item['name']=='python' and not result.get('stdout','').strip(): raise AssertionError(item)
