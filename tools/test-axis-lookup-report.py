#!/usr/bin/env python3
import importlib.util
from pathlib import Path
spec=importlib.util.spec_from_file_location('axis',Path(__file__).with_name('axis-lookup-report.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
a={'active':True,'uploaded_bytes':0,'rebuild_axes':0,'payload_bytes':33280,'resize_cpu_ns':None,'resize_wall_ns':None}
r=m.summarize([{'event':'submit','elapsed_ns':11_000_000_000,'axis_lookup':a}],10,20)
assert r['axis_rebuilt_axes']==0 and r['axis_uploaded_bytes']==0 and r['resize_thread_cpu_us']is None
r=m.summarize([],10,20);assert r['axis_rebuilt_axes']is None and r['axis_texture_payload_bytes_peak']is None
b={**a,'uploaded_bytes':2712,'rebuild_axes':1,'resize_cpu_ns':1000,'resize_wall_ns':2000}
r=m.summarize([{'event':'submit','elapsed_ns':9_000_000_000,'axis_lookup':a},{'event':'submit','elapsed_ns':12_000_000_000,'axis_lookup':b}],10,20)
assert r['axis_rebuilt_axes']==1 and r['axis_uploaded_bytes']==2712 and r['resize_thread_cpu_us']['p50']==1
print('axis report stable-zero/missing-null/resize/window gates passed')
