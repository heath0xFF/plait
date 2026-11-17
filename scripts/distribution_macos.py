"""Build a Finder-friendly disk image and the command-line installer's release index."""

import json
import os
from pathlib import Path
import re
import shutil

from release_macos import digest, run, write_json

ROOT = Path(__file__).resolve().parent.parent


def release_index(component_release, tag, app=None):
    version = component_release["client"]["version"]
    if not re.fullmatch(r"v" + re.escape(version) + r"(?:-preview\.[0-9]+)?", tag):
        raise ValueError("Release tag must match the packaged client version")
    revision = component_release["revision"]
    if not re.fullmatch(r"[0-9a-f]{12,40}", revision):
        raise ValueError("Only clean source builds can have a release index")
    downloads = {}
    for component, prefix in [("all", "Installer"), ("client", "Client"), ("host", "Host")]:
        matches = [item for item in component_release["artifacts"] if item["name"].startswith(f"Plait-{prefix}-")]
        if len(matches) != 1:
            raise ValueError("Expected exactly one package for each component")
        item = matches[0]
        if not re.fullmatch(r"Plait-(Installer|Client|Host)-[0-9a-fA-Z.-]+-macos-arm64\.pkg", item["name"]):
            raise ValueError("Invalid package filename")
        if not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
            raise ValueError("Invalid package checksum")
        downloads[component] = dict(item)
    if app is not None:
        expected = f"Plait-{version}-{revision}-macos-arm64.zip"
        if app["name"] != expected or not re.fullmatch(r"[0-9a-f]{64}", app["sha256"]):
            raise ValueError("Invalid app archive metadata")
        downloads["app"] = dict(app)
    build_id = component_release["host"]["build_id"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-[0-9a-f]{20}", build_id):
        raise ValueError("Invalid host build identity")
    return {
        "schema_version": 1, "tag": tag, "revision": revision,
        "signing": "unsigned-preview", "host_build_id": build_id, "downloads": downloads,
    }


def build_distribution(installers, version, revision, work, archive):
    image_root = work / "disk-image"
    image_root.mkdir()
    shutil.copy2(archive, image_root / "Plait.zip")
    command = image_root / "Install Plait.command"
    command.write_text('''#!/bin/bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
work=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/plait-app.XXXXXXXX")
trap '/bin/rm -rf -- "$work"' EXIT
/usr/bin/ditto -x -k "$here/Plait.zip" "$work"
/bin/bash "$work/Plait.app/Contents/Resources/install-user.sh" --app "$work/Plait.app"
''')
    command.chmod(0o755)
    (image_root / "Read me first.txt").write_text(
        "PLAIT — LOCAL PREVIEW\n\n"
        "Finish or Stop active tasks, then open Install Plait.command.\n"
        "It installs the prebuilt app and bundled host for your user account.\n"
        "No source checkout, Xcode, Rust, or Node installation is needed.\n"
        "An older system-wide install needs your administrator password once.\n\n"
        "This preview is ad-hoc signed and not notarized. Use macOS's explicit\n"
        "per-item approval only if you trust this release. Do not disable Gatekeeper.\n\n"
        "Open Plait from Applications, open a repository, and choose an authenticated\n"
        "model in Settings. The host uses your existing Pi account.\n"
        "Model requests need internet access. Project build tools are not bundled.\n\n"
        "Threads, drafts, and credentials are preserved. Future updates install from\n"
        "Plait > Check for Updates after active tasks finish.\n\n"
        "Setup, authentication, and updates:\n"
        "https://github.com/heath0xFF/plait/blob/main/docs/MACOS.md\n"
    )
    image = work / f"Plait-{version}-{revision}-macos-arm64.dmg"
    run("hdiutil", "create", "-volname", "Plait", "-srcfolder", image_root,
        "-fs", "HFS+", "-format", "UDZO", image)
    run("hdiutil", "verify", image)
    mount = work / "mounted-image"
    mount.mkdir()
    run("hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", mount, image)
    try:
        for source in image_root.iterdir():
            if digest(source) != digest(mount / source.name):
                raise ValueError("Disk image contents differ from the verified installer")
    finally:
        run("hdiutil", "detach", mount)
    script = work / "install.sh"
    shutil.copy2(ROOT / "scripts/install.sh", script)
    artifacts = [image, script]
    if not revision.endswith("-dirty"):
        component = next(path for path in installers if path.name.startswith("Plait-Release-"))
        tag = os.environ.get("PLAIT_RELEASE_TAG") or f"v{version}-preview.0"
        index = work / "release.json"
        app = {"name": archive.name, "sha256": digest(archive)}
        write_json(index, release_index(json.loads(component.read_text()), tag, app))
        artifacts.append(index)
    return artifacts
