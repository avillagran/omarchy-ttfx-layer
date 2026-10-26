#!/usr/bin/env python3
"""Reader regression: attach late results to submit time, missing stays missing."""
import importlib.util
from pathlib import Path
spec=importlib.util.spec_from_file_location('gpu_report',Path(__file__).with_name('gpu-timer-report.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
e=[{'event':'submit','gpu_timing_frame':1,'elapsed_ns':11_000_000_000},
   {'event':'gpu_elapsed','gpu_timing_frame':1,'elapsed_ns':31_000_000_000,'gpu_upload_us':2,'gpu_draw_us':8,'gpu_total_us':10},
   {'event':'submit','gpu_timing_frame':2,'elapsed_ns':12_000_000_000},
   {'event':'submit','gpu_timing_frame':3,'elapsed_ns':31_000_000_000},
   {'event':'gpu_elapsed','gpu_timing_frame':3,'elapsed_ns':31_000_000_001,'gpu_upload_us':20,'gpu_draw_us':80,'gpu_total_us':100}]
r=m.summarize(e,10,20)
assert r['valid_completed_submissions']==1 and r['missing_valid_results']==1
assert r['stages_us']['gpu_draw_us']['p95']==8
assert m.summarize([],10,20)['stages_us']['gpu_total_us'] is None
assert not m.summarize([],10,20)['log_covers_submission_window']
print('GPU report late arrival/window/missing/null gates passed')
