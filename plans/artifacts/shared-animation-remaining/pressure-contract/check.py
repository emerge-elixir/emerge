"""Reproducible bounded model exploration; no timings or native qualification."""
import json
from model import explore

if __name__ == "__main__":
    print(json.dumps([
        explore(2, 1, max_tokens=2, depth=12),
        explore(3, 2, max_tokens=2, depth=12),
        explore(5, 3, max_tokens=3, depth=12),
    ], indent=2))
