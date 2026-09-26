#!/usr/bin/env python3
"""Summarize stored three-process trials; does not run or time benchmarks."""
import json
import re
import statistics
import sys
from pathlib import Path

root = Path(sys.argv[1])
median = statistics.median
print("| Nodes | Owners / loops¹ | Case | Warm p50 / p95 (ms) | Release median [min–max] (ms) | Settle median (ms) | Warm RSS MiB |")
print("|---:|---:|---|---:|---:|---:|---:|")
matrix_path = root / "matrix.json"
matrix = json.loads(matrix_path.read_text()) if matrix_path.exists() else {
    "nodes": [5000, 20000], "owners": [1, 64], "trials": 3,
    "cases": ["paint", "pixel", "length", "moving", "mixed", "independent", "coupled", "upward"],
}
for nodes in matrix["nodes"]:
    for owners in matrix["owners"]:
        for case in matrix["cases"]:
            texts = [(root / f"{nodes}-{owners}-{case}-{trial}.txt").read_text()
                     for trial in range(1, matrix["trials"] + 1)]

            def values(key):
                return [float(re.search(rf"\b{key}=(\d+)", text)[1]) / 1000 for text in texts]

            release = values("release_us")
            settle = f"{median(values('settle_us')):.3f}" if "settle_us=" in texts[0] else "—"
            rss = median([int(re.search(r"warm_p50_us=.*VmRSS:\s+(\d+)", text)[1]) / 1024 for text in texts])
            print(f"| {nodes} | {owners} | {case} | {median(values('warm_p50_us')):.3f} / {median(values('warm_p95_us')):.3f} | {median(release):.3f} [{min(release):.3f}–{max(release):.3f}] | {settle} | {rss:.1f} |")
print("\n¹ `upward` counts looping children under one finite Content parent. Other cases count finite owners.")
print("Warm columns are medians of per-process quantiles, not pooled distributions. RSS is not exact live heap.")
