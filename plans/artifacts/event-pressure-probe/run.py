#!/usr/bin/env python3
"""Exclusive, immutable, separate-process native event pressure experiment."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[3]
os.chdir(ROOT)
parent = Path(f"/proc/{os.getppid()}/cmdline").read_bytes().split(b"\0")
if Path(os.fsdecode(parent[0])).name != "flock" or b"-x" not in parent:
    raise SystemExit("Run directly through scripts/performance-lock.sh exclusive")
out = Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=False)


def capture(*args):
    return subprocess.check_output(args, text=True).strip()


names = subprocess.check_output([
    "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--",
    "native/emerge_skia", "lib", "priv/test_assets", "config", ".cargo", "mix.exs",
    "mix.lock", "rust-toolchain.toml", "scripts/performance-lock.sh",
    "plans/artifacts/event-pressure-probe/README.md", str(Path(__file__).resolve().relative_to(ROOT)),
]).split(b"\0")
paths = sorted({Path(os.fsdecode(name)) for name in names if name and Path(os.fsdecode(name)).is_file()})


def manifest():
    return "".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path}\n" for path in paths)


original = manifest()
identity = "source-sha256:" + hashlib.sha256(original.encode()).hexdigest()
os.environ["EMERGE_SOURCE_REVISION"] = identity
(out / "source.sha256").write_text(original)
with tarfile.open(out / "source.tar.gz", "w:gz", dereference=True) as archive:
    for path in paths:
        archive.add(path, arcname=str(path), recursive=False)
(out / "identity.json").write_text(json.dumps({
    "source": identity, "head_for_reference": capture("git", "rev-parse", "HEAD"),
    "rustc": capture("rustc", "-Vv"), "host": capture("uname", "-a"),
    "cpu": capture("lscpu"), "affinity": sorted(os.sched_getaffinity(0)),
    "features": "release lib tests, default + bench-diagnostics",
    "environment": {key: os.environ.get(key) for key in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_DIR", "LD_PRELOAD", "MALLOC_ARENA_MAX"]},
    "scope": "instrumented native event/tree processing, not GPU/BEAM/physical device input",
}, indent=2) + "\n")
with (out / "build.jsonl").open("w") as stdout, (out / "build.log").open("w") as stderr:
    subprocess.run(["cargo", "test", "--locked", "--release", "--no-run", "--lib",
                    "--manifest-path", "native/emerge_skia/Cargo.toml", "--features",
                    "bench-diagnostics", "--message-format=json"], stdout=stdout, stderr=stderr, check=True)
artifacts = [json.loads(line) for line in (out / "build.jsonl").read_text().splitlines()]
binary = Path(next(item["executable"] for item in artifacts if item.get("reason") == "compiler-artifact" and item.get("executable") and item["target"]["name"] == "emerge_skia"))
immutable_binary = Path(tempfile.mkdtemp(prefix="event-pressure-binary-")) / "probe.bin"
shutil.copy2(binary, immutable_binary)
(out / "binary.sha256").write_text(hashlib.sha256(immutable_binary.read_bytes()).hexdigest() + f"  {immutable_binary}\n")
if manifest() != original:
    raise SystemExit("source changed during build")
cases = [(nodes, kind, rate, rate, "none", 0)
         for nodes in (1, 20000)
         for kind, rates in (("edit", (30, 125, 1000, 8000)), ("pointer", (1000, 8000)), ("ime", (30, 1000)))
         for rate in rates]
cases += [(nodes, "edit", rate, rate, "tree", 1000) for nodes in (1, 20000) for rate in (30, 1000)]
cases += [(1, "pointer", 8000, 8000, "tree", 1000), (1, "ime", 1000, 1000, "tree", 1000)]
cases += [(1, kind, rate, rate, "event", 1000) for kind, rate in (("edit", 30), ("edit", 1000), ("edit", 8000), ("pointer", 8000))]
cases += [(1, "edit", 0, 20000, "none", 0), (1, "pointer", 0, 100000, "none", 0), (1, "ime", 0, 20000, "none", 0), (20000, "edit", 0, 20000, "none", 0)]
(out / "cases.json").write_text(json.dumps(cases, indent=2) + "\n")
rows = []
for repeat in range(3):
    # Rotate scenario order in separate processes; never mix benchmark and build.
    order = cases[repeat * 7:] + cases[:repeat * 7]
    for case in order:
        label = "-".join(map(str, case)) + f"-r{repeat + 1}"
        env = dict(os.environ, EMERGE_EVENT_PROBE=",".join(map(str, case)))
        with (out / f"{label}.log").open("w") as log:
            subprocess.run([str(immutable_binary), "isolated_event_pressure_probe",
                            "--ignored", "--nocapture", "--test-threads=1"], env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90)
        line = next(line for line in (out / f"{label}.log").read_text().splitlines() if "PROBE {" in line)
        row = json.loads(line[line.index("PROBE ") + 6:])
        row["repeat"] = repeat + 1
        rows.append(row)
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(label, "full", row["full"], "buffer", row["buffered_peak"], "outbox", row["outbox_peak"], "settled", row["settled"], flush=True)
if manifest() != original:
    raise SystemExit("source changed during measurement")
print(identity, "processes", len(rows), flush=True)
