#!/usr/bin/env python3
"""Run under scripts/performance-lock.sh exclusive; never alongside builds/tests."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import time

ROOT = Path(__file__).resolve().parents[4]
os.chdir(ROOT)
parent_command = Path(f"/proc/{os.getppid()}/cmdline").read_bytes().split(b"\0")
if Path(os.fsdecode(parent_command[0])).name != "flock" or b"-x" not in parent_command:
    raise SystemExit("Invoke directly through scripts/performance-lock.sh exclusive")
out = Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=False)


def capture(*args):
    return subprocess.check_output(args, text=True).strip()


def sources():
    names = subprocess.check_output([
        "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--",
        "native/emerge_skia", "lib", "priv/test_assets", "config", ".cargo",
        "mix.exs", "mix.lock", "rust-toolchain.toml", "ci-tests.sh",
        str(Path(__file__).resolve().relative_to(ROOT)),
    ]).split(b"\0")
    return sorted({Path(os.fsdecode(name)) for name in names if name and Path(os.fsdecode(name)).is_file()})


def manifest(paths):
    return "".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path}\n" for path in paths)


paths = sources()
original = manifest(paths)
identity = "source-sha256:" + hashlib.sha256(original.encode()).hexdigest()
os.environ["EMERGE_SOURCE_REVISION"] = identity
(out / "source.sha256").write_text(original)
with tarfile.open(out / "source.tar.gz", "w:gz") as archive:
    for path in paths:
        archive.add(path, arcname=str(path), recursive=False)
(out / "identity.json").write_text(json.dumps({
    "source": identity,
    "head_for_reference_only": capture("git", "rev-parse", "HEAD"),
    "rustc": capture("rustc", "-Vv"),
    "cargo": capture("cargo", "-V"),
    "host": capture("uname", "-a"),
    "cpu": capture("lscpu"),
    "affinity": sorted(os.sched_getaffinity(0)),
    "environment": {key: os.environ.get(key) for key in [
        "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_DIR", "CC", "CXX",
        "MALLOC_ARENA_MAX", "LD_PRELOAD", "SKIA_BINARIES_URL", "EMERGE_PERFORMANCE_LOCK",
    ]},
    "features": "default + bench-diagnostics",
    "scope": "native publication, not raster/GPU/BEAM/device qualification",
}, indent=2) + "\n")
(out / "worktree-status.txt").write_text(capture("git", "status", "--short") + "\n")
command = ["cargo", "build", "--release", "--manifest-path", "native/emerge_skia/Cargo.toml",
           "--bench", "shared_animation", "--features", "bench-diagnostics", "--message-format=json"]
with (out / "build.jsonl").open("w") as stdout, (out / "build.log").open("w") as stderr:
    subprocess.run(command, stdout=stdout, stderr=stderr, check=True)
artifacts = [json.loads(line) for line in (out / "build.jsonl").read_text().splitlines()]
binary = Path(next(item["executable"] for item in artifacts
                   if item.get("reason") == "compiler-artifact"
                   and item["target"]["name"] == "shared_animation" and item.get("executable")))
(out / "binary.sha256").write_text(hashlib.sha256(binary.read_bytes()).hexdigest() + "  " + str(binary) + "\n")
(out / "command.json").write_text(json.dumps(command) + "\n")

# Separate processes/trials; no builds between measurements. Rotate scenario
# order between trials to avoid giving one scenario the same position every time.
cases = ["paint", "pixel", "length", "moving", "mixed", "independent", "coupled", "upward"]
for trial in range(3):
    for nodes in [5000, 20000]:
        for owners in [1, 64]:
            for case in cases[trial:] + cases[:trial]:
                filename = out / f"{nodes}-{owners}-{case}-{trial + 1}.txt"
                with filename.open("w") as log:
                    log.write(f"source={identity} trial={trial + 1} wall_time_ns={time.time_ns()} loadavg={os.getloadavg()}\n")
                    log.flush()
                    subprocess.run([str(binary), str(nodes), str(owners), case], stdout=log, stderr=subprocess.STDOUT, check=True)
                print(filename.name, flush=True)
assert paths == sources() and original == manifest(paths), "Source changed during measurement"
print(identity, flush=True)
