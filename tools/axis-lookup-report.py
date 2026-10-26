#!/usr/bin/env python3
"""Axis lookup payload/rebuild observations, independently of feedback FPS."""
import argparse,json,math
from pathlib import Path

def quantiles(values):
    v=sorted(values)
    return {f'p{p}':v[max(0,math.ceil(len(v)*p/100)-1)]for p in(50,95,99)}if v else None

def summarize(events,skip,seconds):
    stop=skip+seconds
    submits=[e for e in events if e.get('event')=='submit'and skip<=e.get('elapsed_ns',-1)/1e9<stop]
    observed=[e['axis_lookup']for e in submits if e.get('axis_lookup')is not None]
    return {'window_seconds':[skip,stop],'full_log_window':max((e.get('elapsed_ns',0)for e in events),default=0)/1e9>=stop,'axis_observed_submissions':len(observed)if observed else None,'axis_active_submissions':sum(e['active']for e in observed)if observed else None,'axis_uploaded_bytes':sum(e['uploaded_bytes']for e in observed)if observed else None,'axis_rebuilt_axes':sum(e['rebuild_axes']for e in observed)if observed else None,'axis_texture_payload_bytes_peak':max((e['payload_bytes']for e in observed),default=None),'resize_thread_cpu_us':quantiles([e['resize_cpu_ns']/1000 for e in observed if e.get('resize_cpu_ns')is not None]),'resize_client_wall_us':quantiles([e['resize_wall_ns']/1000 for e in observed if e.get('resize_wall_ns')is not None]),'note':'Payload covers the two current lookup texture data arrays only, not driver/RSS/allocator. Resize timings only when observed; stable-window zero rebuild is measured, missing events/stages are null. Upload bytes include per-axis rebuild, not total graphics traffic. Use existing feedback reader for observed FPS and GPU timer reader separately.'}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('log',type=Path);p.add_argument('--skip',type=float,default=10);p.add_argument('--seconds',type=float,default=20);a=p.parse_args();print(json.dumps(summarize([json.loads(l)for l in a.log.read_text().splitlines()if l.strip()],a.skip,a.seconds),indent=2))
