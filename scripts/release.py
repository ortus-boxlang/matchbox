#!/usr/bin/env python3
"""MatchBox release versions, download metadata, and next-release preparation."""
import argparse
from datetime import date, datetime, timezone
from difflib import SequenceMatcher
import hashlib
import json
from pathlib import Path
import re
import tomllib

CLI_FILES = {
    "matchbox-linux-x64", "matchbox-linux-x86", "matchbox-linux-arm64",
    "matchbox-linux-armv7", "matchbox-macos-x64", "matchbox-macos-arm64",
    "matchbox-windows-x64.exe", "matchbox-windows-arm64.exe",
}


def read_version(root):
    with (root / "Cargo.toml").open("rb") as file:
        version = tomllib.load(file)["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Cargo.toml must contain a base major.minor.patch version")
    return version


def metadata(root, snapshot, build_id, revision):
    base = read_version(root)
    if not re.fullmatch(r"[0-9]+", build_id):
        raise ValueError("Build ID must be numeric")
    if not re.fullmatch(r"[a-f0-9]{40}", revision):
        raise ValueError("Revision must be a full Git commit SHA")
    version = base + ("-snapshot" if snapshot else "")
    return {
        "base_version": base,
        "version": version,
        "build_version": f"{version}+{build_id}",
        "build_id": build_id,
        "revision": revision,
        "channel": "snapshot" if snapshot else "latest",
        "tag": "snapshot" if snapshot else f"v{base}",
        "next_version": f"{base.split('.')[0]}.{int(base.split('.')[1]) + 1}.0",
    }


def package(directory, info):
    files = {path.name for path in directory.iterdir() if path.is_file()}
    properties_name = f"version-{info['channel']}.properties"
    if files - {"version.json", "SHA256SUMS", properties_name} != CLI_FILES:
        raise ValueError("Expected exactly the eight full CLI binaries")
    artifacts = []
    for name in sorted(CLI_FILES):
        path = directory / name
        if path.stat().st_size == 0:
            raise ValueError(f"Empty CLI binary: {name}")
        with path.open("rb") as file:
            checksum = hashlib.file_digest(file, "sha256").hexdigest()
        artifacts.append({"file": name, "sha256": checksum})
    (directory / "version.json").write_text(json.dumps({**info, "artifacts": artifacts}, indent=2) + "\n")
    (directory / properties_name).write_text(
        f"version={info['build_version']}\n"
        f"buildDate={datetime.now(timezone.utc):%Y-%m-%d %H:%M:%S}\n"
    )
    for name in ("version.json", properties_name):
        artifacts.append({"file": name, "sha256": hashlib.sha256((directory / name).read_bytes()).hexdigest()})
    (directory / "SHA256SUMS").write_text("".join(f"{item['sha256']}  {item['file']}\n" for item in artifacts))


def unreleased_section(text):
    section = re.search(r"^## \[Unreleased\][^\n]*\n(.*?)(?=^## |\Z)", text, re.MULTILINE | re.DOTALL)
    if section is None:
        raise ValueError("Changelog must contain an [Unreleased] section")
    return section


def prepare_changelog(development, original, released, version):
    """Move only published notes out of Unreleased, preserving newer dev work."""
    current = unreleased_section(development)
    baseline = unreleased_section(original)
    old_lines = baseline.group(1).splitlines()
    new_lines = current.group(1).splitlines()
    headings = []
    heading = ""
    for line in new_lines:
        if line.startswith("### "):
            heading = line
        headings.append(heading)
    pending = {}
    for operation, _, _, start, end in SequenceMatcher(None, old_lines, new_lines, autojunk=False).get_opcodes():
        if operation not in ("insert", "replace"):
            continue
        for index in range(start, end):
            line = new_lines[index]
            if not line.startswith("### "):
                pending.setdefault(headings[index], []).append(line)
    notes = []
    for heading, lines in pending.items():
        body = "\n".join(lines).strip()
        if body:
            notes.append((heading + "\n" if heading else "") + body)
    release = re.search(rf"^## \[{re.escape(version)}\][^\n]*\n.*?(?=^## |\Z)", released, re.MULTILINE | re.DOTALL)
    if release is None or re.search(rf"^## \[{re.escape(version)}\]", development, re.MULTILINE):
        raise ValueError("Expected one new finalized changelog release")
    section = "## [Unreleased]\n\n" + ("\n\n".join(notes) + "\n\n" if notes else "")
    return development[:current.start()] + section + release.group().rstrip() + "\n\n" + development[current.end():]


def finalize_changelog(text, version, released_on):
    if re.search(rf"^## \[v?{re.escape(version)}\]", text, re.MULTILINE):
        raise ValueError(f"Changelog already contains release {version}")
    section = unreleased_section(text)
    notes = section.group(1).strip()
    replacement = f"## [Unreleased]\n\n## [{version}] - {released_on}\n\n{notes}\n\n"
    return text[:section.start()] + replacement + text[section.end():], notes


def bump(root, released_version):
    current = read_version(root)
    if current != released_version:
        raise ValueError(f"develop is at {current}, not released version {released_version}; refusing to overwrite its version")
    major, minor, _ = map(int, current.split("."))
    next_version = f"{major}.{minor + 1}.0"
    manifest_path = root / "Cargo.toml"
    lock_path = root / "Cargo.lock"
    manifest = manifest_path.read_text()
    lock = lock_path.read_text()
    manifest, manifest_count = re.subn(
        rf'(?m)^(version\s*=\s*"){re.escape(current)}("\s*)$',
        rf'\g<1>{next_version}\2', manifest, count=1,
    )
    lock, lock_count = re.subn(
        rf'(\[\[package\]\]\nname = "matchbox"\nversion = "){re.escape(current)}(")',
        rf'\g<1>{next_version}\2', lock,
    )
    if manifest_count != 1 or lock_count != 1:
        raise ValueError("Could not update the root package version in both Cargo.toml and Cargo.lock")
    manifest_path.write_text(manifest)
    lock_path.write_text(lock)
    return next_version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["metadata", "package", "finalize", "bump", "prepare-changelog"])
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--snapshot", action="store_true")
    parser.add_argument("--build-id")
    parser.add_argument("--revision")
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--released-version")
    parser.add_argument("--original-changelog", type=Path)
    parser.add_argument("--released-changelog", type=Path)
    args = parser.parse_args()
    if args.command == "prepare-changelog":
        if args.released_version is None or args.original_changelog is None or args.released_changelog is None:
            parser.error("prepare-changelog requires --released-version, --original-changelog, and --released-changelog")
        path = args.root / "CHANGELOG.md"
        path.write_text(prepare_changelog(path.read_text(), args.original_changelog.read_text(), args.released_changelog.read_text(), args.released_version))
        return
    if args.command == "bump":
        if args.released_version is None:
            parser.error("bump requires --released-version")
        print(bump(args.root, args.released_version))
        return
    if args.command == "finalize":
        if args.released_version is None:
            parser.error("finalize requires --released-version")
        if args.released_version != read_version(args.root):
            parser.error("Released version must match Cargo.toml")
        path = args.root / "CHANGELOG.md"
        changelog, notes = finalize_changelog(path.read_text(), args.released_version, date.today().isoformat())
        path.write_text(changelog)
        (args.root / "release_notes.md").write_text(notes + "\n")
        return
    if args.build_id is None or args.revision is None:
        parser.error("metadata/package require --build-id and --revision")
    info = metadata(args.root, args.snapshot, args.build_id, args.revision)
    if args.command == "metadata":
        for key, value in info.items():
            print(f"{key}={value}")
    else:
        if args.directory is None:
            parser.error("package requires --directory")
        package(args.directory, info)


if __name__ == "__main__":
    main()
