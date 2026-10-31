#!/usr/bin/env python3
"""Author regression: serialized feature switches must not embed a foreign runner."""
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
base = "bif-io,bif-cli,bif-crypto,bif-http"
env = dict(os.environ, CARGO_BUILD_JOBS="4")
for jit in [True, False, True]:
    target = root / "target" / ("lint-jit" if jit else "lint-dev")
    subprocess.run(["cargo", "build", "--no-default-features", "--features",
                    base + (",jit" if jit else ""), "--target-dir", str(target)],
                   cwd=root, env=env, check=True)
    if jit:
        subprocess.run([sys.executable, str(root / "tests/jit_runner.py"),
                        str(target / "debug/matchbox")], env=env, check=True)
print("Serialized JIT / non-JIT / JIT runner feature freshness: OK")
