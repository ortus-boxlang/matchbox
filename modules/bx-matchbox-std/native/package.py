#!/usr/bin/env python3
"""Author build: produce an installable module/ZIP with prebuilt, hash-pinned bytes."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--sdk-path", type=Path, default=root.parents[1] / "crates/matchbox-plugin-sdk")
args = parser.parse_args()
target = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
if target != "x86_64-unknown-linux-gnu":
    parser.error("This initial provider is validated only on x86_64-unknown-linux-gnu")
command = ["cargo", "build", "--release", "--manifest-path", str(root / "native/Cargo.toml"), "--target", target,
           "--config", f'patch.crates-io.matchbox-plugin-sdk.path="{args.sdk_path.resolve()}"']
subprocess.run(command, check=True)
package = root / "build/module"
package.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / "ModuleConfig.bx", package)
shutil.copytree(root / "bifs", package / "bifs", dirs_exist_ok=True)
shutil.copy2(root / "LICENSE", package / "LICENSE")
shutil.copy2(root / "README.md", package / "README.md")
library = Path("vendor") / target / "libbx_matchbox_std_native.so"
(package / library).parent.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / "native/target" / target / "release" / library.name, package / library)
metadata = json.loads((root / "box.json").read_text())
metadata["matchbox"] = {"artifacts": {target: {"path": library.as_posix(), "sha256": hashlib.sha256((package / library).read_bytes()).hexdigest(), "abi": 1}}}
(package / "box.json").write_text(json.dumps(metadata, indent=2) + "\n")
archive_path = root / "build/bx-matchbox-std.zip"
with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(package.rglob("*")):
        if path.is_file():
            archive.write(path, path.relative_to(package))
print(package)
print(archive_path)
