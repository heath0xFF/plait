#!/usr/bin/env python3
"""Write the update manifest that installed Plait apps and hosts check.

Usage: update_manifest.py <release-dir> <tag>

The manifest names each artifact's size and SHA-256. The release workflow
signs it with the update key (crates/plait-core/examples/update-key.rs);
clients verify that signature before trusting any field.
"""
import hashlib
import json
import re
import sys
from pathlib import Path

PLATFORM = "macos-arm64"


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1 << 16), b""):
            hasher.update(block)
    return hasher.hexdigest()


def artifact(path):
    return {"name": path.name, "sha256": digest(path), "size": path.stat().st_size}


def build(release, tag):
    records = list(release.glob("Plait-Release-*.json"))
    if len(records) != 1:
        raise ValueError("Expected exactly one component release record")
    record = json.loads(records[0].read_text())
    version, revision = record["client"]["version"], record["revision"]
    if revision.endswith("-dirty"):
        raise ValueError("Releases must be built from a clean checkout")
    if not re.fullmatch(r"v" + re.escape(version) + r"(?:-preview\.[1-9][0-9]*)?", tag):
        raise ValueError("Release tag must match the packaged client version")
    if record["host"]["version"] != version:
        raise ValueError("Client and host versions differ")
    client = release / f"Plait-{version}-{revision}-{PLATFORM}.zip"
    host = release / f"Plait-Host-{version}-{revision}-{PLATFORM}.tar.gz"
    return {
        "schema_version": 1,
        "version": tag[1:],
        "tag": tag,
        "revision": revision,
        "host_build_id": record["host"]["build_id"],
        "artifacts": {f"client-{PLATFORM}": artifact(client), f"host-{PLATFORM}": artifact(host)},
    }


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    release = Path(sys.argv[1])
    manifest = build(release, sys.argv[2])
    (release / "update.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
