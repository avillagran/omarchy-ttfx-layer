#!/usr/bin/env python3
"""Join deferred GPU timestamp results to submission windows; never infer FPS."""
import argparse
import json
import math
from pathlib import Path

def quantiles(values):
    values = sorted(values)
    return {f'p{p}': values[max(0, math.ceil(len(values)*p/100)-1)] for p in (50,95,99)} if values else None

def summarize(events,skip,seconds):
    stop=skip+seconds
    submits={e['gpu_timing_frame']:e for e in events if e.get('event')=='submit' and e.get('gpu_timing_frame') is not None and skip<=e.get('elapsed_ns',-1)/1e9<stop}
    measured=[e for e in events if e.get('event')=='gpu_elapsed' and e.get('gpu_timing_frame') in submits]
    ids={e['gpu_timing_frame'] for e in measured}
    return {
        'window_seconds':[skip,stop],
        'log_covers_submission_window':max((e.get('elapsed_ns',0)for e in events),default=0)/1e9>=stop,
        'capabilities':[e for e in events if e.get('event')=='gpu_timer_capability'],
        'unavailable':[e for e in events if e.get('event')=='gpu_timer_unavailable'],
        'query_tagged_submissions':len(submits),
        'valid_completed_submissions':len(ids),
        'missing_valid_results':len(set(submits)-ids),
        'stages_us':{k:quantiles([e[k] for e in measured])for k in ('gpu_upload_us','gpu_draw_us','gpu_total_us')},
        'disjoint_invalidated_frames_all_log':sum(e['invalidated_frames']for e in events if e.get('event')=='gpu_timer_disjoint'),
        'saturated_frames_all_log':sum(e.get('event')=='gpu_timer_saturated'for e in events),
        'invalid_timestamps_all_log':sum(e.get('event')=='gpu_timer_invalid'for e in events),
        'pending_unread_at_exit':[e['pending_unread_frames']for e in events if e.get('event')=='gpu_timer_end'],
        'note':'GPU timestamp differences linked to submission window, not readback arrival. No CPU/GPU clock subtraction, compositor time, watts or unique FPS. Missing/disjoint are excluded, never zero timings. Query overhead requires separate comparison; percentiles do not add.'
    }

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('log',type=Path);p.add_argument('--skip',type=float,default=10);p.add_argument('--seconds',type=float,default=20);a=p.parse_args()
    events=[json.loads(line)for line in a.log.read_text().splitlines()if line.strip()]
    print(json.dumps(summarize(events,a.skip,a.seconds),indent=2))
