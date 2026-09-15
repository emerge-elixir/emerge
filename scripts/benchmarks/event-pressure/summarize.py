#!/usr/bin/env python3
"""Summarize raw repeats without converting them into whole-runtime guarantees."""
import collections
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
rows = json.loads((root / "results.json").read_text())
groups = collections.defaultdict(list)
for row in rows:
    groups[(row["nodes"], row["kind"], row["rate"], row["count"], row["stall"])].append(row)


def span(values, precision=0):
    lo, hi = min(values), max(values)
    return f"{lo:.{precision}f}" if lo == hi else f"{lo:.{precision}f}–{hi:.{precision}f}"


lines = ["# Event pressure measurements", "", "Ranges across three separate release processes per case.", "",
         "| Nodes | Input | Offered/s (or burst) | Pause | Channel full | Incoming peak | Listener FIFO peak | Outbox peak | Settled within recovery window |",
         "|---:|---|---:|---|---:|---:|---:|---:|---:|"]
for (nodes, kind, rate, count, stall), runs in groups.items():
    assert len(runs) == 3
    values = [span([r[field] for r in runs]) for field in ("full", "event_queue_peak", "buffered_peak", "outbox_peak")]
    lines.append(f"| {nodes:,} | {kind} | {rate if rate else str(count) + ' burst'} | {stall} | " + " | ".join(values) + f" | {sum(r['settled'] for r in runs)}/3 |")
(root / "table.md").write_text("\n".join(lines) + "\n")
